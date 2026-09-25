use super::{
    EXTERIOR_Y, FIX_DISTANCE, IntersectionType, LayoutError, LoopBox, LoopConfig,
    MIN_NEGATIVE_ANGLE, MIN_POSITIVE_ANGLE, RECOGNIZE_DISTANCE, ROOT_NODE, RadiusPolicy,
    RnaPuzzlerOptions, StemBox, TreeNode, Vec2, apply_changes_to_config_and_bounding_boxes,
    expand_range, internal_invariant, optimize_tree,
};
use crate::drawing::geometry::{Circle, EPSILON_3, approx_eq, segments_intersect};
use std::f64::consts::{PI, TAU};

fn is_to_the_right(start: Vec2, end: Vec2, point: Vec2) -> bool {
    let line = end - start;
    let normal = Vec2::new(line.y, -line.x);
    (point - (end + normal)).length_squared() < (point - (end - normal)).length_squared()
}

/// Port of C's `projectPointOntoLine`. Projects `p` onto segment `a` to `b` and
/// clamps the result to the endpoints.
fn project_point_onto_line(start: Vec2, end: Vec2, point: Vec2) -> Vec2 {
    let to_point = point - start;
    let edge = end - start;
    let normal = Vec2::new(-edge.y, edge.x);
    let t =
        (to_point.y - to_point.x * normal.y / normal.x) / (edge.y - edge.x * normal.y / normal.x);
    if t < 0.0 {
        start
    } else if t > 1.0 {
        end
    } else {
        start + edge * t
    }
}

/// Port of C's `ClosestPtPointBulge`. Finds the closest point on acute bulge
/// triangle `a,b,c` using the reference's edge-projection method.
fn closest_pt_point_triangle(point: Vec2, a: Vec2, b: Vec2, c: Vec2) -> Vec2 {
    if is_to_the_right(a, b, c) != is_to_the_right(a, b, point) {
        project_point_onto_line(a, b, point)
    } else if is_to_the_right(b, c, a) != is_to_the_right(b, c, point) {
        project_point_onto_line(b, c, point)
    } else if is_to_the_right(c, a, b) != is_to_the_right(c, a, point) {
        project_point_onto_line(c, a, point)
    } else {
        point
    }
}

fn closest_pt_point_obb(stem: &StemBox, p: Vec2) -> Vec2 {
    let dv = p - stem.center;
    let dist_0 = dv.dot(stem.a);
    let dist_1 = dv.dot(stem.b);

    let clamped_0 = dist_0.clamp(-stem.half_extents.x, stem.half_extents.x);
    let clamped_1 = dist_1.clamp(-stem.half_extents.y, stem.half_extents.y);

    stem.center + stem.a * clamped_0 + stem.b * clamped_1
}

fn intersect_loop_loop(l1: &LoopBox, l2: &LoopBox) -> bool {
    let d = l1.center.distance(l2.center);
    let padding = 0.5 * RECOGNIZE_DISTANCE;
    let r1 = l1.radius + padding;
    let r2 = l2.radius + padding;
    d <= r1 + r2
}

fn intersect_stem_loop(stem: &StemBox, loop_box: &LoopBox) -> bool {
    let p = closest_pt_point_obb(stem, loop_box.center);
    let d_sq = (p - loop_box.center).length_squared();
    let r_eps = loop_box.radius + RECOGNIZE_DISTANCE;
    d_sq < r_eps * r_eps
}

fn intersect_stem_stem(s1: &StemBox, s2: &StemBox) -> bool {
    let s2_sides = s2.side_segments();
    s1.side_segments().iter().copied().any(|(a0, a1)| {
        s2_sides
            .iter()
            .copied()
            .any(|(b0, b1)| segments_intersect(a0, a1, b0, b1))
    })
}

fn intersect_loop_bulges(loop_box: &LoopBox, stem: &StemBox) -> Option<usize> {
    let r_eps = loop_box.radius + RECOGNIZE_DISTANCE;
    for (i, _) in stem.bulges.iter().enumerate() {
        let (a, b, c) = stem.bulge_coords(i, 0.0);
        let p = closest_pt_point_triangle(loop_box.center, a, b, c);
        if (p - loop_box.center).length_squared() <= r_eps * r_eps {
            return Some(i);
        }
    }
    None
}

fn intersect_bulges_bulges(stem1: &StemBox, stem2: &StemBox) -> Option<(usize, usize)> {
    let dist = 0.5 * RECOGNIZE_DISTANCE;
    for (i, _) in stem1.bulges.iter().enumerate() {
        let (first_prev, first_center, first_next) = stem1.bulge_coords(i, dist);
        for (j, _) in stem2.bulges.iter().enumerate() {
            let (second_prev, second_center, second_next) = stem2.bulge_coords(j, dist);
            if segments_intersect(first_prev, first_center, second_prev, second_center)
                || segments_intersect(first_prev, first_center, second_center, second_next)
                || segments_intersect(first_center, first_next, second_prev, second_center)
                || segments_intersect(first_center, first_next, second_center, second_next)
            {
                return Some((i, j));
            }
        }
    }
    None
}

fn intersect_stem_bulges(stem1: &StemBox, stem2: &StemBox) -> Option<usize> {
    if stem2.bulges.is_empty() {
        return None;
    }
    let stem_sides = stem1.side_segments();
    for (j, _) in stem2.bulges.iter().enumerate() {
        let (p_prev, p_this, p_next) = stem2.bulge_coords(j, RECOGNIZE_DISTANCE);
        if stem_sides.iter().copied().any(|(start, end)| {
            segments_intersect(start, end, p_prev, p_this)
                || segments_intersect(start, end, p_this, p_next)
        }) {
            return Some(j);
        }
    }
    None
}

pub(super) fn intersect_node_pair(
    node1_idx: usize,
    node1: &TreeNode,
    node2_idx: usize,
    node2: &TreeNode,
) -> IntersectionType {
    if node1_idx == node2_idx {
        return IntersectionType::NoIntersection;
    }
    let Some(geometry1) = node1.geometry() else {
        return IntersectionType::NoIntersection;
    };
    let Some(geometry2) = node2.geometry() else {
        return IntersectionType::NoIntersection;
    };
    let (s1, l1) = (&geometry1.stem_box, &geometry1.loop_box);
    let (s2, l2) = (&geometry2.stem_box, &geometry2.loop_box);

    let extra_distance = {
        let mut ed = RECOGNIZE_DISTANCE;
        let mut count = 0;
        if s1.bulge_dist > 0.0 {
            count += 1;
        }
        if s2.bulge_dist > 0.0 {
            count += 1;
        }
        if count > 0 {
            ed += (s1.bulge_dist + s2.bulge_dist) / f64::from(count);
        }
        ed
    };

    let b1 = geometry1.aabb;
    let b2 = geometry2.aabb;
    if !b1.overlaps_with_gap(b2, extra_distance) {
        return IntersectionType::NoIntersection;
    }

    let parent1 = node1.parent();
    let parent2 = node2.parent();
    let node1_is_parent_of_node2 = parent2 == Some(node1_idx);
    let node2_is_parent_of_node1 = parent1 == Some(node2_idx);
    let nodes_have_common_parent = parent1 == parent2 && parent1.is_some();

    // SxS
    if !node1_is_parent_of_node2
        && !node2_is_parent_of_node1
        && !nodes_have_common_parent
        && intersect_stem_stem(s1, s2)
    {
        return IntersectionType::SxS;
    }

    // LxL
    if !node1_is_parent_of_node2 && !node2_is_parent_of_node1 && intersect_loop_loop(l1, l2) {
        return IntersectionType::LxL;
    }

    // SxL
    if !node2_is_parent_of_node1 && intersect_stem_loop(s1, l2) {
        return IntersectionType::SxL;
    }

    // LxS
    if !node1_is_parent_of_node2 && intersect_stem_loop(s2, l1) {
        return IntersectionType::LxS;
    }

    // LxB
    if !node1_is_parent_of_node2 && intersect_loop_bulges(l1, s2).is_some() {
        return IntersectionType::LxB;
    }

    // BxL
    if !node2_is_parent_of_node1 && intersect_loop_bulges(l2, s1).is_some() {
        return IntersectionType::BxL;
    }

    // SxB
    if intersect_stem_bulges(s1, s2).is_some() {
        return IntersectionType::SxB;
    }

    // BxS
    if intersect_stem_bulges(s2, s1).is_some() {
        return IntersectionType::BxS;
    }

    // BxB
    if intersect_bulges_bulges(s1, s2).is_some() {
        return IntersectionType::BxB;
    }

    IntersectionType::NoIntersection
}

fn intersect_nodes(idx1: usize, idx2: usize, nodes: &[TreeNode]) -> IntersectionType {
    intersect_node_pair(idx1, &nodes[idx1], idx2, &nodes[idx2])
}

fn intersect_node_exterior(
    node_idx: usize,
    options: &RnaPuzzlerOptions,
    nodes: &[TreeNode],
) -> bool {
    let node = &nodes[node_idx];
    let Some(parent_idx) = node.parent() else {
        return false;
    };
    if parent_idx == 0 || !options.checks.exterior {
        return false;
    }
    let Some(geometry) = node.geometry() else {
        return false;
    };
    (geometry.loop_box.center.y - (geometry.loop_box.radius + RECOGNIZE_DISTANCE)) <= EXTERIOR_Y
}

fn intersect_node_tree(node_idx: usize, tree_idx: usize, nodes: &[TreeNode]) -> bool {
    let tree_node = &nodes[tree_idx];
    if intersect_nodes(node_idx, tree_idx, nodes) != IntersectionType::NoIntersection {
        return true;
    }
    for &child in &tree_node.children {
        if intersect_node_tree(node_idx, child, nodes) {
            return true;
        }
    }
    false
}

pub(super) fn intersect_trees(idx1: usize, idx2: usize, nodes: &[TreeNode]) -> bool {
    if intersect_node_tree(idx1, idx2, nodes) {
        return true;
    }
    for &child in &nodes[idx1].children {
        if intersect_trees(child, idx2, nodes) {
            return true;
        }
    }
    false
}

pub(super) fn intersect_node_lists(
    list1: &[usize],
    list2: &[usize],
    options: &RnaPuzzlerOptions,
    nodes: &[TreeNode],
) -> bool {
    for &idx1 in list1 {
        let is_ext1 = idx1 == 0;
        for &idx2 in list2 {
            let is_ext2 = idx2 == 0;
            if is_ext1 {
                if intersect_node_exterior(idx2, options, nodes) {
                    return true;
                }
            } else if is_ext2 {
                if intersect_node_exterior(idx1, options, nodes) {
                    return true;
                }
            } else if intersect_nodes(idx1, idx2, nodes) != IntersectionType::NoIntersection {
                return true;
            }
        }
    }
    false
}

/// Inflation added to a node box for the sweep-and-prune check.
///
/// When `count > 0`, `intersect_node_pair` uses
/// `RECOGNIZE_DISTANCE + (b1 + b2) / count`, where `count` is the number of
/// non-zero bulge distances. The zero-count case uses only the fixed distance.
/// Two inflations cover the tested gap for all three count values.
fn node_inflation(node: &TreeNode) -> f64 {
    let bulge_dist = node
        .geometry()
        .map_or(0.0, |geometry| geometry.stem_box.bulge_dist);
    0.5 * RECOGNIZE_DISTANCE + bulge_dist
}

/// Inflated node bounds ordered by x for the sweep.
///
/// The exterior node is omitted because it uses a separate predicate.
#[derive(Default)]
struct BoxSet {
    node_indices: Vec<usize>,
    min_x: Vec<f64>,
    max_x: Vec<f64>,
    min_y: Vec<f64>,
    max_y: Vec<f64>,
    /// Slots into the arrays above, ordered by `min_x`.
    order: Vec<u32>,
}

impl BoxSet {
    /// Fills inflated bounds for each non-exterior node in `list`.
    ///
    /// Returns `false` when a node has no geometry, telling the caller to use the
    /// exhaustive scan.
    fn rebuild(&mut self, list: &[usize], nodes: &[TreeNode]) -> bool {
        let list_nodes = list.iter().copied().filter(|&idx| idx != ROOT_NODE);
        if !self.node_indices.iter().copied().eq(list_nodes) {
            self.node_indices.clear();
            self.node_indices
                .extend(list.iter().copied().filter(|&idx| idx != ROOT_NODE));
            let Ok(node_count) = u32::try_from(self.node_indices.len()) else {
                self.node_indices.clear();
                self.order.clear();
                return false;
            };
            self.order.clear();
            self.order.extend(0..node_count);
        }

        self.min_x.resize(self.node_indices.len(), 0.0);
        self.max_x.resize(self.node_indices.len(), 0.0);
        self.min_y.resize(self.node_indices.len(), 0.0);
        self.max_y.resize(self.node_indices.len(), 0.0);
        for (slot, &node_idx) in self.node_indices.iter().enumerate() {
            let node = &nodes[node_idx];
            let Some(geometry) = node.geometry() else {
                return false;
            };
            let inflation = node_inflation(node);
            self.min_x[slot] = geometry.aabb.min.x - inflation;
            self.max_x[slot] = geometry.aabb.max.x + inflation;
            self.min_y[slot] = geometry.aabb.min.y - inflation;
            self.max_y[slot] = geometry.aabb.max.y + inflation;
        }

        let min_x = &self.min_x;
        if self.order.windows(2).any(|slots| {
            min_x[slots[0] as usize]
                .total_cmp(&min_x[slots[1] as usize])
                .is_gt()
        }) {
            self.order.sort_unstable_by(|&left, &right| {
                min_x[left as usize].total_cmp(&min_x[right as usize])
            });
        }
        true
    }

    /// Whether the inflated boxes in slots `left` and `right` overlap on y.
    fn overlaps_y(&self, left: usize, right: usize) -> bool {
        self.min_y[right] <= self.max_y[left] && self.min_y[left] <= self.max_y[right]
    }
}

/// Buffers reused by the sweep-and-prune broad phase.
///
/// The optimizer checks the same node lists repeatedly, so it refills these
/// buffers instead of allocating them for every probe.
#[derive(Default)]
pub(super) struct IntersectionScratch {
    primary: BoxSet,
    secondary: BoxSet,
}

/// Returns whether any node in `list` reaches the exterior.
///
/// `intersect_node_lists` uses this when either list contains the exterior node.
/// The check depends on one node, so it is a single pass.
fn any_intersects_exterior(
    list: &[usize],
    options: &RnaPuzzlerOptions,
    nodes: &[TreeNode],
) -> bool {
    list.iter()
        .any(|&idx| idx != ROOT_NODE && intersect_node_exterior(idx, options, nodes))
}

/// Whether any two distinct nodes in one ordered box set intersect.
fn sweep_self(boxes: &BoxSet, nodes: &[TreeNode]) -> bool {
    for (position, &left_slot) in boxes.order.iter().enumerate() {
        let left = left_slot as usize;
        let left_max_x = boxes.max_x[left];

        for &right_slot in &boxes.order[position + 1..] {
            let right = right_slot as usize;
            // The boxes are ordered by inflated `min_x`, so later boxes cannot overlap.
            if boxes.min_x[right] > left_max_x {
                break;
            }
            if !boxes.overlaps_y(left, right) {
                continue;
            }
            if intersect_nodes(boxes.node_indices[left], boxes.node_indices[right], nodes)
                != IntersectionType::NoIntersection
            {
                return true;
            }
        }
    }
    false
}

/// Whether any node in the first ordered box set intersects the second.
fn sweep_cross(first: &BoxSet, second: &BoxSet, nodes: &[TreeNode]) -> bool {
    for &left_slot in &first.order {
        let left = left_slot as usize;
        let left_max_x = first.max_x[left];
        let left_min_x = first.min_x[left];
        let left_min_y = first.min_y[left];
        let left_max_y = first.max_y[left];

        for &right_slot in &second.order {
            let right = right_slot as usize;
            if second.min_x[right] > left_max_x {
                break;
            }
            if second.max_x[right] < left_min_x {
                continue;
            }
            if second.min_y[right] > left_max_y || left_min_y > second.max_y[right] {
                continue;
            }
            if intersect_nodes(first.node_indices[left], second.node_indices[right], nodes)
                != IntersectionType::NoIntersection
            {
                return true;
            }
        }
    }
    false
}

/// Checks a subtree for intersections within itself and with its ancestors.
///
/// Both scans share the subtree's ordered box set. If any node lacks geometry,
/// the function falls back to the original exhaustive scan.
pub(super) fn intersect_optimization_lists(
    subtree: &[usize],
    ancestor_list: &[usize],
    scratch: &mut IntersectionScratch,
    options: &RnaPuzzlerOptions,
    nodes: &[TreeNode],
) -> bool {
    let subtree_contains_exterior = subtree.contains(&ROOT_NODE);
    if subtree_contains_exterior && any_intersects_exterior(subtree, options, nodes) {
        return true;
    }
    if !scratch.primary.rebuild(subtree, nodes) {
        return intersect_node_lists(subtree, subtree, options, nodes)
            || intersect_node_lists(subtree, ancestor_list, options, nodes);
    }
    if sweep_self(&scratch.primary, nodes) {
        return true;
    }

    if ancestor_list.is_empty() {
        return false;
    }
    // Exterior pairs ignore geometry. Resolve them before the sweep and omit
    // them from the box sets.
    if ancestor_list.contains(&ROOT_NODE) && any_intersects_exterior(subtree, options, nodes) {
        return true;
    }
    if subtree_contains_exterior && any_intersects_exterior(ancestor_list, options, nodes) {
        return true;
    }
    if !scratch.secondary.rebuild(ancestor_list, nodes) {
        return intersect_node_lists(subtree, ancestor_list, options, nodes);
    }
    sweep_cross(&scratch.primary, &scratch.secondary, nodes)
}

pub(super) fn collect_subtree_nodes(node_idx: usize, nodes: &[TreeNode], list: &mut Vec<usize>) {
    list.push(node_idx);
    for &child in &nodes[node_idx].children {
        collect_subtree_nodes(child, nodes, list);
    }
}

pub(super) fn collect_ancestor_nodes(node_idx: usize, nodes: &[TreeNode], list: &mut Vec<usize>) {
    let mut curr = nodes[node_idx].parent();
    while let Some(p) = curr {
        list.push(p);
        curr = nodes[p].parent();
    }
}

fn get_child_angle(
    root_idx: usize,
    child_idx: usize,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let parent_node = &nodes[root_idx];
    let child_node = &nodes[child_idx];
    let parent_loop = parent_node.require_loop_box(root_idx)?;
    let parent_stem = parent_node.require_stem_box(root_idx)?;

    let parent_loop_stem_vector = parent_stem.center - parent_loop.center;
    let child_loop = child_node.require_loop_box(child_idx)?;

    let v2 = child_loop.center - parent_loop.center;
    let mut angle = parent_loop_stem_vector.angle_between(v2);

    if !crate::drawing::geometry::point_is_right_of_line(
        parent_loop.center,
        parent_loop.center + parent_loop_stem_vector,
        child_loop.center,
    ) {
        angle = TAU - angle;
    }
    Ok(angle)
}

fn get_child_angle_by_index(
    parent_idx: usize,
    child_index: usize,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    get_child_angle(parent_idx, nodes[parent_idx].children[child_index], nodes)
}

fn get_bounding_wedge_rec(
    root_idx: usize,
    node_idx: usize,
    parent_angle: f64,
    min_angle: &mut f64,
    max_angle: &mut f64,
    nodes: &[TreeNode],
) -> Result<(), LayoutError> {
    let parent_idx = nodes[node_idx].require_parent(node_idx)?;
    let center_root = nodes[root_idx].require_loop_box(root_idx)?.center;
    let center_node = nodes[node_idx].require_loop_box(node_idx)?.center;
    let v_root_node = center_node - center_root;

    let node_angle = if parent_idx == root_idx {
        get_child_angle(root_idx, node_idx, nodes)?
    } else {
        let center_parent = nodes[parent_idx].require_loop_box(parent_idx)?.center;
        let v_root_parent = center_parent - center_root;
        let mut diff_parent = v_root_parent.angle_between(v_root_node);
        if !crate::drawing::geometry::point_is_right_of_line(
            center_root,
            center_root + v_root_parent,
            center_node,
        ) {
            diff_parent *= -1.0;
        }
        parent_angle + diff_parent
    };

    let stem_node = nodes[node_idx].require_stem_box(node_idx)?;
    let loop_node = nodes[node_idx].require_loop_box(node_idx)?;

    let mut points: Vec<_> = (0..stem_node.bulges.len())
        .map(|i| {
            let (_, bulge_point, _) = stem_node.bulge_coords(i, FIX_DISTANCE);
            bulge_point
        })
        .collect();

    if parent_idx == root_idx {
        let p_left = stem_node.center - stem_node.a * stem_node.half_extents.x
            + stem_node.b * stem_node.half_extents.y;
        let p_right = stem_node.center
            - stem_node.a * stem_node.half_extents.x
            - stem_node.b * stem_node.half_extents.y;
        points.push(p_left);
        points.push(p_right);
    }

    let radius_node = loop_node.radius + FIX_DISTANCE;
    let distance_root_node = v_root_node.length();
    let angle1 = (radius_node / distance_root_node).asin();
    let angle2 = -angle1;

    for &diff in &[angle1, angle2] {
        expand_range(min_angle, max_angle, node_angle + diff);
    }

    for pt in points {
        let mut diff_angle = v_root_node.angle_between(pt - center_root);
        if !crate::drawing::geometry::point_is_right_of_line(
            center_root,
            center_root + v_root_node,
            pt,
        ) {
            diff_angle *= -1.0;
        }
        expand_range(min_angle, max_angle, node_angle + diff_angle);
    }

    for i in 0..nodes[node_idx].children.len() {
        let child = nodes[node_idx].children[i];
        get_bounding_wedge_rec(root_idx, child, node_angle, min_angle, max_angle, nodes)?;
    }
    Ok(())
}

pub(super) fn get_bounding_wedge(
    root_idx: usize,
    child_index: usize,
    nodes: &[TreeNode],
) -> Result<(f64, f64), LayoutError> {
    let child_idx = nodes[root_idx].children[child_index];
    let node_angle = get_child_angle(root_idx, child_idx, nodes)?;
    let mut min_angle = node_angle;
    let mut max_angle = node_angle;

    get_bounding_wedge_rec(
        root_idx,
        child_idx,
        node_angle,
        &mut min_angle,
        &mut max_angle,
        nodes,
    )?;
    Ok((min_angle, max_angle))
}

fn child_slot_for_node(parent_idx: usize, target_idx: usize, nodes: &[TreeNode]) -> Option<usize> {
    let children = &nodes[parent_idx].children;
    if children.is_empty() {
        return None;
    }

    let mut child_slot = children.len() - 1;
    for (current_child, &child_idx) in children.iter().enumerate() {
        if child_idx > target_idx {
            child_slot = current_child.checked_sub(1)?;
            break;
        }
    }
    Some(child_slot)
}

fn get_rotation_sign(path: &[usize], nodes: &[TreeNode]) -> Result<i16, LayoutError> {
    let path_len = path.len();
    if path_len < 2 {
        return Ok(0);
    }
    let mut angle = 0.0;
    for i in 1..path_len {
        angle += get_child_angle(path[i - 1], path[i], nodes)? - PI;
    }

    if angle > 0.0 {
        Ok(-1)
    } else {
        Ok(i16::from(angle < 0.0))
    }
}

fn is_interior_loop(node_idx: usize, nodes: &[TreeNode]) -> bool {
    node_idx != 0 && nodes[node_idx].children.len() == 1
}

fn is_multi_loop(node_idx: usize, nodes: &[TreeNode]) -> bool {
    node_idx != 0 && nodes[node_idx].children.len() > 1
}

fn is_straight_interior_loop(node_idx: usize, nodes: &[TreeNode]) -> Result<bool, LayoutError> {
    Ok(is_interior_loop(node_idx, nodes)
        && (get_child_angle_by_index(node_idx, 0, nodes)? - PI).abs() < EPSILON_3)
}

fn fix_intersection_of_circles(
    static_center: Vec2,
    static_radius: f64,
    mobile_center: Vec2,
    mobile_radius: f64,
    rotation_center: Vec2,
    rotation_sign: i16,
) -> f64 {
    if rotation_sign == 0 {
        return 0.0;
    }
    let rotation_radius = rotation_center.distance(mobile_center);
    let extended_static_radius = static_radius + mobile_radius + FIX_DISTANCE;

    let Some((first_cut, second_cut)) = crate::drawing::geometry::circle_circle_intersections(
        Circle {
            center: rotation_center,
            radius: rotation_radius,
        },
        Circle {
            center: static_center,
            radius: extended_static_radius,
        },
    ) else {
        return 0.0;
    };

    let v_ref = mobile_center - rotation_center;
    let mut angle1 = -v_ref.signed_angle_between(first_cut - rotation_center);
    if approx_eq(angle1, 0.0, 0.0, 2) {
        angle1 = if angle1.is_sign_negative() {
            MIN_NEGATIVE_ANGLE
        } else {
            MIN_POSITIVE_ANGLE
        };
    }

    let mut angle2;
    if let Some(second_cut) = second_cut {
        let v2 = second_cut - rotation_center;
        angle2 = -v_ref.signed_angle_between(v2);
        if approx_eq(angle2, 0.0, 0.0, 2) {
            angle2 = if angle2.is_sign_negative() {
                MIN_NEGATIVE_ANGLE
            } else {
                MIN_POSITIVE_ANGLE
            };
        }

        let is_cw1 = crate::drawing::geometry::point_is_right_of_line(
            rotation_center,
            rotation_center + v_ref,
            first_cut,
        );
        let is_cw2 = crate::drawing::geometry::point_is_right_of_line(
            rotation_center,
            rotation_center + v_ref,
            second_cut,
        );
        if is_cw1 == is_cw2 {
            if angle1.abs() < angle2.abs() {
                if is_cw2 {
                    angle2 -= TAU;
                } else {
                    angle2 = TAU - angle2;
                }
            } else {
                if is_cw1 {
                    angle1 -= TAU;
                } else {
                    angle1 = TAU - angle1;
                }
            }
        }
    } else {
        angle2 = angle1;
    }

    let mut rotation_angle = 0.0;
    if rotation_sign == 1 {
        rotation_angle = angle1.max(angle2);
    } else if rotation_sign == -1 {
        rotation_angle = angle1.min(angle2);
    }
    rotation_angle
}

fn point_to_rotation_angle(
    center: Vec2,
    reference_vector: Vec2,
    rotation_sign: i16,
    point: Vec2,
) -> f64 {
    let point_vector = point - center;
    let angle = reference_vector.angle_between(point_vector);
    let is_clockwise =
        crate::drawing::geometry::point_is_right_of_line(center, center + reference_vector, point);

    match (rotation_sign.signum(), is_clockwise) {
        (1, true) => angle,
        (1, false) => TAU - angle,
        (-1, true) => angle - TAU,
        (-1, false) => -angle,
        _ => angle,
    }
}

fn fix_intersection_of_rectangle_and_circle(
    static_rect: &StemBox,
    mobile_circ_center: Vec2,
    mobile_circ_radius: f64,
    rotation_center: Vec2,
    rotation_sign: i16,
) -> f64 {
    if rotation_sign == 0 {
        return 0.0;
    }
    let distance = FIX_DISTANCE + mobile_circ_radius;
    let rotation_radius = rotation_center.distance(mobile_circ_center);

    let axis_offset = static_rect.half_extents.y + distance;
    let axis_direction = static_rect.a;
    let axis_anchor_pos = static_rect.center + static_rect.b * axis_offset;
    let axis_anchor_neg = static_rect.center - static_rect.b * axis_offset;

    let circle = Circle {
        center: rotation_center,
        radius: rotation_radius,
    };
    let mut cuts = crate::drawing::geometry::circle_line_intersections(
        circle,
        axis_anchor_pos,
        axis_anchor_pos + axis_direction,
    );
    cuts.extend(crate::drawing::geometry::circle_line_intersections(
        circle,
        axis_anchor_neg,
        axis_anchor_neg + axis_direction,
    ));

    if cuts.is_empty() {
        let axis_normal = axis_direction.perp_left();
        cuts.push(rotation_center + axis_normal * rotation_radius);
        cuts.push(rotation_center - axis_normal * rotation_radius);
    }

    let reference_vector = mobile_circ_center - rotation_center;
    let mut angle = f64::from(rotation_sign) * TAU;
    for cut in cuts {
        let mut candidate =
            point_to_rotation_angle(rotation_center, reference_vector, rotation_sign, cut);
        if approx_eq(candidate, 0.0, 0.0, 2) {
            candidate = if candidate.is_sign_negative() {
                MIN_NEGATIVE_ANGLE
            } else {
                MIN_POSITIVE_ANGLE
            };
        }
        if rotation_sign > 0 && candidate > 0.0 {
            angle = angle.min(candidate);
        }
        if rotation_sign < 0 && candidate < 0.0 {
            angle = angle.max(candidate);
        }
    }

    if approx_eq(angle, 0.0, 0.0, 2) || approx_eq(angle.abs(), TAU, 0.0, 2) {
        0.0
    } else {
        angle
    }
}

fn get_rotation_angle_lxl(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_loop = nodes[ancestor_idx].require_loop_box(ancestor_idx)?;
    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;
    let mobile_loop = nodes[intersector_idx].require_loop_box(intersector_idx)?;

    Ok(fix_intersection_of_circles(
        static_loop.center,
        static_loop.radius,
        mobile_loop.center,
        mobile_loop.radius,
        rotation_loop.center,
        rotation_sign,
    ))
}

fn get_rotation_angle_lxs(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_rect = nodes[intersector_idx].require_stem_box(intersector_idx)?;
    let mobile_circ = nodes[ancestor_idx].require_loop_box(ancestor_idx)?;
    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;

    let inverse_rotation_sign = -rotation_sign;
    let inverse_rotation_angle = fix_intersection_of_rectangle_and_circle(
        static_rect,
        mobile_circ.center,
        mobile_circ.radius,
        rotation_loop.center,
        inverse_rotation_sign,
    );
    Ok(-inverse_rotation_angle)
}

fn get_rotation_angle_sxl(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_rect = nodes[ancestor_idx].require_stem_box(ancestor_idx)?;
    let mobile_circ = nodes[intersector_idx].require_loop_box(intersector_idx)?;
    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;

    Ok(fix_intersection_of_rectangle_and_circle(
        static_rect,
        mobile_circ.center,
        mobile_circ.radius,
        rotation_loop.center,
        rotation_sign,
    ))
}

fn get_rotation_angle_lxb(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_loop = nodes[ancestor_idx].require_loop_box(ancestor_idx)?;
    let mobile_stem = nodes[intersector_idx].require_stem_box(intersector_idx)?;

    let mut dummy_loop = *static_loop;
    dummy_loop.radius += RECOGNIZE_DISTANCE;
    let Some(mobile_bulge_index) = intersect_loop_bulges(&dummy_loop, mobile_stem) else {
        return Ok(0.0);
    };

    let (mobile_center, mobile_radius) = mobile_stem.bulge_circle(mobile_bulge_index);

    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;
    Ok(fix_intersection_of_circles(
        static_loop.center,
        static_loop.radius,
        mobile_center,
        mobile_radius,
        rotation_loop.center,
        rotation_sign,
    ))
}

fn get_rotation_angle_bxl(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_stem = nodes[ancestor_idx].require_stem_box(ancestor_idx)?;
    let mobile_loop = nodes[intersector_idx].require_loop_box(intersector_idx)?;

    let mut dummy_loop = *mobile_loop;
    dummy_loop.radius += RECOGNIZE_DISTANCE;
    let Some(static_bulge_index) = intersect_loop_bulges(&dummy_loop, static_stem) else {
        return Ok(0.0);
    };

    let (static_center, static_radius) = static_stem.bulge_circle(static_bulge_index);

    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;
    Ok(fix_intersection_of_circles(
        static_center,
        static_radius,
        mobile_loop.center,
        mobile_loop.radius,
        rotation_loop.center,
        rotation_sign,
    ))
}

fn get_rotation_angle_sxb(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_stem = nodes[ancestor_idx].require_stem_box(ancestor_idx)?;
    let mobile_stem = nodes[intersector_idx].require_stem_box(intersector_idx)?;
    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;

    let Some(mobile_bulge_index) = intersect_stem_bulges(static_stem, mobile_stem) else {
        return Ok(0.0);
    };

    let (mobile_center, mobile_radius) = mobile_stem.bulge_circle(mobile_bulge_index);

    Ok(fix_intersection_of_rectangle_and_circle(
        static_stem,
        mobile_center,
        mobile_radius,
        rotation_loop.center,
        rotation_sign,
    ))
}

fn get_rotation_angle_bxs(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_stem = nodes[intersector_idx].require_stem_box(intersector_idx)?;
    let mobile_stem = nodes[ancestor_idx].require_stem_box(ancestor_idx)?;
    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;

    let Some(mobile_bulge_index) = intersect_stem_bulges(static_stem, mobile_stem) else {
        return Ok(0.0);
    };

    let (mobile_center, mobile_radius) = mobile_stem.bulge_circle(mobile_bulge_index);

    Ok(fix_intersection_of_rectangle_and_circle(
        static_stem,
        mobile_center,
        mobile_radius,
        rotation_loop.center,
        rotation_sign,
    ))
}

fn get_rotation_angle_bxb(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let static_stem = nodes[ancestor_idx].require_stem_box(ancestor_idx)?;
    let mobile_stem = nodes[intersector_idx].require_stem_box(intersector_idx)?;

    let Some((static_bulge_index, mobile_bulge_index)) =
        intersect_bulges_bulges(static_stem, mobile_stem)
    else {
        return Ok(0.0);
    };

    let (static_center, static_radius) = static_stem.bulge_circle(static_bulge_index);

    let (mobile_center, mobile_radius) = mobile_stem.bulge_circle(mobile_bulge_index);

    let rotation_loop = nodes[rotation_node_idx].require_loop_box(rotation_node_idx)?;
    Ok(fix_intersection_of_circles(
        static_center,
        static_radius,
        mobile_center,
        mobile_radius,
        rotation_loop.center,
        rotation_sign,
    ))
}

fn get_rotation_angle(
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    it: IntersectionType,
    rotation_sign: i16,
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    // All handlers share this signature. SxS reuses SxL for successive stem and
    // loop elements.
    type Handler = fn(usize, usize, usize, i16, &[TreeNode]) -> Result<f64, LayoutError>;
    let handler: Handler = match it {
        IntersectionType::LxL => get_rotation_angle_lxl,
        IntersectionType::LxS => get_rotation_angle_lxs,
        IntersectionType::LxB => get_rotation_angle_lxb,
        IntersectionType::SxL | IntersectionType::SxS => get_rotation_angle_sxl,
        IntersectionType::SxB => get_rotation_angle_sxb,
        IntersectionType::BxL => get_rotation_angle_bxl,
        IntersectionType::BxS => get_rotation_angle_bxs,
        IntersectionType::BxB => get_rotation_angle_bxb,
        IntersectionType::NoIntersection => return Ok(0.0),
    };
    handler(
        ancestor_idx,
        rotation_node_idx,
        intersector_idx,
        rotation_sign,
        nodes,
    )
}

fn setup_exterior_bounding_boxes(
    exterior_idx: usize,
    top_level_ancestor_idx: usize,
    intersector_idx: usize,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
) -> Result<IntersectionType, LayoutError> {
    let top_level_x = nodes[top_level_ancestor_idx]
        .require_loop_box(top_level_ancestor_idx)?
        .center
        .x;

    let upper_y = EXTERIOR_Y;
    let lower_y = upper_y - options.turtle.paired_distance;

    let radius = 0.5 * (upper_y - lower_y);
    let ext_loop = LoopBox {
        center: Vec2::new(top_level_x, upper_y - radius),
        radius,
    };

    let intersector_aabb = nodes[intersector_idx]
        .require_geometry(intersector_idx)?
        .aabb;
    let loop_x = top_level_x;

    let stem = if intersector_aabb.max.x < loop_x {
        stem_box_from_points(
            Vec2::new(intersector_aabb.min.x, upper_y),
            Vec2::new(loop_x, upper_y),
            Vec2::new(intersector_aabb.min.x, lower_y),
        )
    } else if loop_x < intersector_aabb.min.x {
        stem_box_from_points(
            Vec2::new(intersector_aabb.max.x, lower_y),
            Vec2::new(loop_x, lower_y),
            Vec2::new(intersector_aabb.max.x, upper_y),
        )
    } else {
        let left_stem = stem_box_from_points(
            Vec2::new(intersector_aabb.min.x, upper_y),
            Vec2::new(loop_x, upper_y),
            Vec2::new(intersector_aabb.min.x, lower_y),
        );
        let temp = make_temp_exterior_node(ext_loop, left_stem);
        let it = intersect_node_pair(
            exterior_idx,
            &temp,
            intersector_idx,
            &nodes[intersector_idx],
        );
        if it != IntersectionType::NoIntersection {
            commit_exterior_node(exterior_idx, &mut nodes[exterior_idx], temp)?;
            return Ok(it);
        }
        stem_box_from_points(
            Vec2::new(intersector_aabb.max.x, lower_y),
            Vec2::new(loop_x, lower_y),
            Vec2::new(intersector_aabb.max.x, upper_y),
        )
    };

    let temp = make_temp_exterior_node(ext_loop, stem);
    let it = intersect_node_pair(
        exterior_idx,
        &temp,
        intersector_idx,
        &nodes[intersector_idx],
    );
    if it != IntersectionType::NoIntersection {
        commit_exterior_node(exterior_idx, &mut nodes[exterior_idx], temp)?;
    }
    Ok(it)
}

fn make_temp_exterior_node(loop_box: LoopBox, stem_box: StemBox) -> TreeNode {
    TreeNode {
        children: Vec::new(),
        kind: super::NodeKind::Exterior {
            geometry: Some(super::NodeGeometry::new(loop_box, stem_box)),
        },
    }
}

/// Move a temporary exterior node's bounding geometry onto the exterior node.
fn commit_exterior_node(
    node_idx: usize,
    node: &mut TreeNode,
    temp: TreeNode,
) -> Result<(), LayoutError> {
    let super::NodeKind::Exterior { geometry } = temp.kind else {
        return Err(internal_invariant("temporary exterior node is a stem"));
    };
    node.set_exterior_geometry(node_idx, geometry)
}

/// Builds a stem's oriented bounding box from three points. `start` to `end` is
/// the axis, and `start` to `pair` is the paired edge. This ports C's
/// `createStemBox`, including its zero-length-axis fix. The bulge list is empty
/// until a caller fills it.
pub(super) fn stem_box_from_points(start: Vec2, end: Vec2, pair: Vec2) -> StemBox {
    let mut axis = (end - start) * 0.5;
    let normal = (start - pair) * 0.5;
    let mut axis_length = axis.length();
    let normal_length = normal.length();
    if approx_eq(axis_length, 0.0, 0.0, 2) {
        // C's normal(b) == perp_right in the degenerate `createStemBox` case.
        // `perp_left` flips the stem's distal end to the wrong side.
        axis = normal.perp_right().normalized().unwrap_or(Vec2::zero()) * 0.1;
        axis_length = 0.1;
    }
    StemBox {
        a: axis.normalized().unwrap_or(Vec2::zero()),
        b: normal.normalized().unwrap_or(Vec2::zero()),
        center: start + axis - normal,
        half_extents: Vec2::new(axis_length, normal_length),
        bulge_dist: 0.0,
        bulges: Vec::new(),
    }
}

fn check_node_against_ancestors(
    node_idx: usize,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<Option<usize>, LayoutError> {
    let mut ancestor = nodes[node_idx].parent();
    let mut top_level_ancestor = node_idx;

    while let Some(ancestor_idx) = ancestor {
        if ancestor_idx == 0 {
            break;
        }
        top_level_ancestor = ancestor_idx;

        let it = intersect_nodes(ancestor_idx, node_idx, nodes);
        if it != IntersectionType::NoIntersection
            && let Some(changed_idx) = handle_intersection_with_ancestor(
                ancestor_idx,
                node_idx,
                it,
                options,
                nodes,
                changes_applied,
            )?
        {
            return Ok(Some(changed_idx));
        }
        ancestor = nodes[ancestor_idx].parent();
    }

    if options.checks.exterior && intersect_node_exterior(node_idx, options, nodes) {
        let exterior_idx = 0;
        let it = setup_exterior_bounding_boxes(
            exterior_idx,
            top_level_ancestor,
            node_idx,
            options,
            nodes,
        )?;
        if it != IntersectionType::NoIntersection
            && let Some(changed_idx) = handle_intersection_with_ancestor(
                exterior_idx,
                node_idx,
                it,
                options,
                nodes,
                changes_applied,
            )?
        {
            return Ok(Some(changed_idx));
        }
    }

    Ok(None)
}

fn handle_intersection_with_ancestor(
    ancestor_idx: usize,
    intersector_idx: usize,
    it: IntersectionType,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<Option<usize>, LayoutError> {
    let mut path = Vec::new();
    let mut curr = Some(intersector_idx);
    while let Some(curr_idx) = curr {
        if curr_idx == ancestor_idx {
            break;
        }
        if curr_idx == intersector_idx || !is_straight_interior_loop(curr_idx, nodes)? {
            path.push(curr_idx);
        }
        curr = nodes[curr_idx].parent();
    }
    path.reverse();

    let include_ancestor = match it {
        IntersectionType::LxL | IntersectionType::LxS | IntersectionType::LxB => false,
        _ => !is_straight_interior_loop(ancestor_idx, nodes)?,
    };
    if include_ancestor {
        path.insert(0, ancestor_idx);
    }

    let path_len = path.len();
    if path_len < 2 {
        return Ok(None);
    }

    let child_indices = (0..(path_len - 1))
        .map(|i| child_slot_for_node(path[i], path[i + 1], nodes))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            internal_invariant(format!(
                "failed to map ancestor path for node {intersector_idx} under ancestor {ancestor_idx}"
            ))
        })?;

    let rotation_sign = get_rotation_sign(&path, nodes)?;
    if rotation_sign != 0 {
        // Walk from the intersector to its ancestors, checking interior loops
        // first and multiloops second.
        type LoopKind = fn(usize, &[TreeNode]) -> bool;
        for is_kind in [is_interior_loop as LoopKind, is_multi_loop] {
            for i in (0..(path_len - 1)).rev() {
                let node_idx = path[i];
                if !is_kind(node_idx, nodes) {
                    continue;
                }
                let fix = AncestorIntersectionFix {
                    ancestor_idx,
                    rotation_node_idx: node_idx,
                    intersector_idx,
                    rotation_index: child_indices[i],
                    rotation_sign,
                    it,
                };
                if let Some(changed_idx) =
                    fix_intersection_with_ancestor(fix, options, nodes, changes_applied)?
                {
                    return Ok(Some(changed_idx));
                }
            }
        }
    }

    Ok(None)
}

#[derive(Debug, Clone, Copy)]
struct AncestorIntersectionFix {
    ancestor_idx: usize,
    rotation_node_idx: usize,
    intersector_idx: usize,
    rotation_index: usize,
    rotation_sign: i16,
    it: IntersectionType,
}

fn fix_intersection_with_ancestor(
    fix: AncestorIntersectionFix,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<Option<usize>, LayoutError> {
    if fix.rotation_node_idx == fix.ancestor_idx
        && (fix.it == IntersectionType::LxL
            || fix.it == IntersectionType::LxS
            || fix.it == IntersectionType::LxB)
    {
        return Ok(None);
    }

    let mut internal_child_angle = 0.0;
    if is_interior_loop(fix.rotation_node_idx, nodes) {
        internal_child_angle = get_child_angle_by_index(fix.rotation_node_idx, 0, nodes)?;
        let mut allowed_rotation_sign = 0;
        if internal_child_angle > PI {
            allowed_rotation_sign = -1;
        } else if internal_child_angle < PI {
            allowed_rotation_sign = 1;
        }

        if fix.rotation_sign != allowed_rotation_sign {
            return Ok(None);
        }
    }

    let mut rotation_angle = get_rotation_angle(
        fix.ancestor_idx,
        fix.rotation_node_idx,
        fix.intersector_idx,
        fix.it,
        fix.rotation_sign,
        nodes,
    )?;

    if is_interior_loop(fix.rotation_node_idx, nodes) {
        let diff_to_straight = PI - internal_child_angle;
        if rotation_angle.abs() > diff_to_straight.abs() {
            rotation_angle = diff_to_straight;
        }
    }

    let changed = if rotation_angle == 0.0 {
        false
    } else {
        let rotation_node_children_len = nodes[fix.rotation_node_idx].children.len();
        let mut deltas = vec![0.0; rotation_node_children_len + 1];
        let delta_angle = rotation_angle.abs();

        let config_size = rotation_node_children_len + 1;
        let child_boundary = Boundary::for_child(fix.rotation_index, config_size)?;
        let (index_left, index_right) = if rotation_angle > 0.0 {
            (Boundary::parent(), child_boundary)
        } else {
            (child_boundary, Boundary::parent())
        };

        let delta_request = DeltaRequest {
            node_idx: fix.rotation_node_idx,
            recursive_end: Some(fix.ancestor_idx),
            index_left,
            index_right,
            delta_angle,
        };

        calc_deltas(delta_request, options, &mut deltas, nodes)?;

        check_and_apply_config_changes(
            fix.rotation_node_idx,
            &deltas,
            options,
            nodes,
            changes_applied,
        )?
    };

    if changed {
        Ok(Some(fix.rotation_node_idx))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy)]
struct DeltaRequest {
    node_idx: usize,
    recursive_end: Option<usize>,
    index_left: Boundary,
    index_right: Boundary,
    delta_angle: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Boundary(usize);

impl Boundary {
    const fn parent() -> Self {
        Self(0)
    }

    fn for_child(child: usize, config_size: usize) -> Result<Self, LayoutError> {
        let boundary = child
            .checked_add(1)
            .ok_or_else(|| internal_invariant(format!("child boundary {child} overflows usize")))?;
        if boundary >= config_size {
            return Err(internal_invariant(format!(
                "child boundary {child} is out of range for config size {config_size}"
            )));
        }
        Ok(Self(boundary))
    }

    const fn is_parent(self) -> bool {
        self.0 == 0
    }

    const fn is_child(self, child: usize) -> bool {
        self.0 == child + 1
    }

    fn next(self, config_size: usize) -> Self {
        debug_assert!(config_size > 0 && self.0 < config_size);
        Self((self.0 + 1) % config_size)
    }

    fn previous(self, config_size: usize) -> Self {
        debug_assert!(config_size > 0 && self.0 < config_size);
        Self((self.0 + config_size - 1) % config_size)
    }

    const fn arc_after(self) -> usize {
        self.0
    }

    fn arc_before(self, config_size: usize) -> usize {
        debug_assert!(config_size > 0 && self.0 < config_size);
        (self.0 + config_size - 1) % config_size
    }
}

struct DeltaWorkspace {
    config_size: usize,
    space: Vec<f64>,
    delta_cfg: Vec<f64>,
    increase: Vec<bool>,
    decrease: Vec<bool>,
    current_angles: Vec<f64>,
}

fn calc_deltas(
    request: DeltaRequest,
    options: &RnaPuzzlerOptions,
    deltas: &mut [f64],
    nodes: &[TreeNode],
) -> Result<f64, LayoutError> {
    let node_idx = request.node_idx;
    let node_cfg = nodes[node_idx].require_cfg(node_idx)?;
    let min_outer_angle = (options.turtle.paired_distance / (2.0 * node_cfg.radius)).asin();
    let mut workspace = build_delta_workspace(request, node_cfg, min_outer_angle, nodes)?;

    let mut target_angle = request.delta_angle;
    calc_deltas_equidistant_increase(target_angle, &workspace.increase, &mut workspace.delta_cfg);

    target_angle = calc_deltas_nearest_neighbors_first_decrease(
        target_angle,
        request.index_left,
        request.index_right,
        workspace.config_size,
        &workspace.decrease,
        &workspace.space,
        &mut workspace.delta_cfg,
    );

    if target_angle != 0.0 {
        let mut parent = nodes[node_idx].parent();
        let mut can_go_higher = false;

        while let Some(p_idx) = parent {
            if parent == request.recursive_end || p_idx == 0 {
                break;
            }
            let parent_is_multi_loop = is_multi_loop(p_idx, nodes);
            if parent_is_multi_loop {
                can_go_higher = true;
                break;
            }
            let parent_cfg = nodes[p_idx].require_cfg(p_idx)?;
            let child_angle = parent_cfg.arcs[0].arc_angle;
            if (child_angle - PI).abs() >= EPSILON_3 {
                if child_angle > PI {
                    if request.index_left.is_child(0) {
                        can_go_higher = true;
                        break;
                    }
                } else if child_angle < PI && request.index_left.is_parent() {
                    can_go_higher = true;
                    break;
                }
            }
            parent = nodes[p_idx].parent();
        }

        if !can_go_higher {
            target_angle = calc_deltas_maximum_first_decrease(
                target_angle,
                request.index_left,
                request.index_right,
                workspace.config_size,
                &mut workspace.delta_cfg,
                &workspace.current_angles,
                min_outer_angle,
            );
        }
    }

    calc_deltas_equidistant_increase(-target_angle, &workspace.increase, &mut workspace.delta_cfg);

    deltas[..workspace.config_size].copy_from_slice(&workspace.delta_cfg[..workspace.config_size]);

    let checksum: f64 = deltas.iter().sum();
    if checksum.abs() > EPSILON_3 {
        deltas[..workspace.config_size].fill(0.0);
        target_angle = request.delta_angle;
    }

    if !cfg_is_valid(node_cfg, deltas) {
        deltas[..workspace.config_size].fill(0.0);
        target_angle = request.delta_angle;
    }

    Ok(request.delta_angle - target_angle)
}

fn build_delta_workspace(
    request: DeltaRequest,
    node_cfg: &LoopConfig,
    min_outer_angle: f64,
    nodes: &[TreeNode],
) -> Result<DeltaWorkspace, LayoutError> {
    let child_count = nodes[request.node_idx].children.len();
    let config_size = child_count + 1;
    let mut space = vec![0.0; config_size];
    let delta_cfg = vec![0.0; config_size];
    let mut increase = vec![false; config_size];
    let mut decrease = vec![false; config_size];
    let current_angles = (0..config_size)
        .map(|idx| node_cfg.arcs.get(idx).map_or(0.0, |arc| arc.arc_angle))
        .collect::<Vec<_>>();

    let mut angles_min = Vec::with_capacity(child_count);
    let mut angles_max = Vec::with_capacity(child_count);
    for current_child in 0..child_count {
        let (min_angle, max_angle) = get_bounding_wedge(request.node_idx, current_child, nodes)?;
        angles_min.push(min_angle);
        angles_max.push(max_angle);
    }
    populate_delta_space(
        &mut space,
        &angles_min,
        &angles_max,
        &current_angles,
        min_outer_angle,
    );
    mark_delta_directions(request, &space, &mut increase, &mut decrease);

    Ok(DeltaWorkspace {
        config_size,
        space,
        delta_cfg,
        increase,
        decrease,
        current_angles,
    })
}

fn populate_delta_space(
    space: &mut [f64],
    angles_min: &[f64],
    angles_max: &[f64],
    current_angles: &[f64],
    min_outer_angle: f64,
) {
    let config_size = space.len();
    space[0] = angles_min[0] - min_outer_angle;
    for i in 1..(config_size - 1) {
        space[i] = angles_min[i] - angles_max[i - 1];
    }
    if config_size > 1 {
        space[config_size - 1] = (TAU - min_outer_angle) - angles_max[config_size - 2];
    } else {
        space[0] = TAU - 2.0 * min_outer_angle;
    }
    for (available, &current_angle) in space.iter_mut().zip(current_angles) {
        *available = (*available).min(current_angle - 2.0 * min_outer_angle);
    }
}

fn mark_delta_directions(
    request: DeltaRequest,
    space: &[f64],
    increase: &mut [bool],
    decrease: &mut [bool],
) {
    let config_size = space.len();
    let mut current_index = request.index_left;
    while current_index != request.index_right {
        let idx = current_index.arc_after();
        increase[idx] = true;
        decrease[idx] = false;
        current_index = current_index.next(config_size);
    }
    while current_index != request.index_left {
        let idx = current_index.arc_after();
        increase[idx] = false;
        decrease[idx] = space[idx] > 0.0;
        current_index = current_index.next(config_size);
    }
}

fn nearest_neighbor_step_count(
    index_left: Boundary,
    index_right: Boundary,
    config_size: usize,
) -> usize {
    // The reference walk counts the parent boundary and the last arc as
    // distinct positions while measuring the outside interval. Keep that
    // visitation count even though both map into normalized `Boundary`/arc
    // space below. An even interval intentionally visits its middle arc twice.
    if index_left.0 >= index_right.0 {
        index_left.0 - index_right.0
    } else {
        config_size - (index_right.0 - index_left.0) + 1
    }
}

fn fill_nearest_neighbor_indices(
    index_left: Boundary,
    index_right: Boundary,
    config_size: usize,
    decrease: &[bool],
    index: &mut [usize],
) -> usize {
    debug_assert_eq!(decrease.len(), config_size);
    debug_assert_eq!(
        index.len(),
        nearest_neighbor_step_count(index_left, index_right, config_size)
    );

    let steps = index.len();
    let paired_visits = steps / 2;
    let mut count = 0;
    let mut left_cursor = index_left;
    let mut right_cursor = index_right;
    for _ in 0..paired_visits {
        let left_arc = left_cursor.arc_before(config_size);
        if decrease[left_arc] {
            index[count] = left_arc;
            count += 1;
        }

        let right_arc = right_cursor.arc_after();
        if decrease[right_arc] {
            index[count] = right_arc;
            count += 1;
        }

        left_cursor = left_cursor.previous(config_size);
        right_cursor = right_cursor.next(config_size);
    }

    if !steps.is_multiple_of(2) {
        index[count] = left_cursor.arc_before(config_size);
        count += 1;
    }

    count
}

fn calc_deltas_equidistant_increase(target_angle: f64, increase: &[bool], delta_cfg: &mut [f64]) {
    let mut increase_count = 0;
    for &inc in increase {
        if inc {
            increase_count += 1;
        }
    }
    if increase_count > 0 {
        let delta_per_increase = target_angle / f64::from(increase_count);
        for i in 0..increase.len() {
            if increase[i] {
                delta_cfg[i] += delta_per_increase;
            }
        }
    }
}

fn calc_deltas_nearest_neighbors_first_decrease(
    target_angle_in: f64,
    index_left: Boundary,
    index_right: Boundary,
    config_size: usize,
    decrease: &[bool],
    space: &[f64],
    delta_cfg: &mut [f64],
) -> f64 {
    let mut target_angle = target_angle_in;
    let steps = nearest_neighbor_step_count(index_left, index_right, config_size);
    let mut index = vec![0; steps];

    let mut changed = true;
    while changed {
        changed = false;
        let count = fill_nearest_neighbor_indices(
            index_left,
            index_right,
            config_size,
            decrease,
            &mut index,
        );

        if count > 0 {
            let part_angle = target_angle / (count as f64);
            for &j in &index[..count] {
                if decrease[j] {
                    let diff = -((space[j] + delta_cfg[j]).min(part_angle));
                    delta_cfg[j] += diff;
                    target_angle += diff;
                    if diff != 0.0 {
                        changed = true;
                    }
                }
            }
        }
    }

    target_angle
}

fn calc_deltas_maximum_first_decrease(
    target_angle_in: f64,
    index_left: Boundary,
    index_right: Boundary,
    config_size: usize,
    delta_cfg: &mut [f64],
    current_angles: &[f64],
    min_angle_half: f64,
) -> f64 {
    let mut target_angle = target_angle_in;
    let mut do_loop = true;

    while do_loop {
        let mut max_space = 0.0;
        let mut max_space_index = None;

        if index_left.is_parent() {
            let mut sum_angles = 0.0;
            let mut cursor = Boundary::parent();
            while cursor != index_right {
                cursor = cursor.next(config_size);
                let idx = cursor.arc_before(config_size);
                let cfg = current_angles[idx] + delta_cfg[idx] - 2.0 * min_angle_half;
                sum_angles += cfg;
            }
            while !cursor.is_parent() {
                cursor = cursor.next(config_size);
                let idx = cursor.arc_before(config_size);
                let cfg = current_angles[idx] + delta_cfg[idx] - 2.0 * min_angle_half;
                if sum_angles < PI {
                    if cfg > max_space {
                        max_space = cfg;
                        max_space_index = Some(idx);
                    }
                } else {
                    break;
                }
                sum_angles += cfg;
            }
        } else if index_right.is_parent() {
            let mut sum_angles = 0.0;
            let mut cursor = Boundary::parent();
            while cursor != index_left {
                let idx = cursor.arc_before(config_size);
                let cfg = current_angles[idx] + delta_cfg[idx] - 2.0 * min_angle_half;
                sum_angles += cfg;
                cursor = cursor.previous(config_size);
            }
            while !cursor.is_parent() {
                let idx = cursor.arc_before(config_size);
                let cfg = current_angles[idx] + delta_cfg[idx] - 2.0 * min_angle_half;
                if sum_angles < PI {
                    if cfg > max_space {
                        max_space = cfg;
                        max_space_index = Some(idx);
                    }
                } else {
                    break;
                }
                sum_angles += cfg;
                cursor = cursor.previous(config_size);
            }
        } else {
            let mut cursor = index_right;
            while cursor != index_left {
                let next_idx = cursor.arc_after();
                let cfg = current_angles[next_idx] + delta_cfg[next_idx] - 2.0 * min_angle_half;
                if cfg > max_space {
                    max_space = cfg;
                    max_space_index = Some(next_idx);
                }
                cursor = cursor.next(config_size);
            }
        }

        let mut diff = 0.0;
        if let Some(max_space_index) = max_space_index {
            let factor = if target_angle < 0.1 * target_angle_in {
                1.0
            } else {
                0.5
            };
            diff = -((factor * max_space).min(target_angle));
            delta_cfg[max_space_index] += diff;
            target_angle += diff;
        }

        do_loop = target_angle > 0.0 && diff.abs() > EPSILON_3;
    }

    target_angle
}

fn cfg_is_valid(cfg: &LoopConfig, delta_cfg: &[f64]) -> bool {
    let mut sum_angles = 0.0;
    let mut valid_single_angles = true;
    for (arc, &delta) in cfg.arcs.iter().zip(delta_cfg.iter()) {
        let angle = arc.arc_angle + delta;
        sum_angles += angle;
        let valid_angle = angle > 0.0 && angle < TAU;
        valid_single_angles = valid_single_angles && valid_angle;
    }
    let valid_sum_angles = (sum_angles - TAU).abs() < EPSILON_3;
    valid_single_angles && valid_sum_angles
}

fn check_and_apply_config_changes(
    node_idx: usize,
    delta_cfg: &[f64],
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<bool, LayoutError> {
    let mut final_deltas = delta_cfg.to_vec();
    let num_arcs = nodes[node_idx].require_cfg(node_idx)?.arcs.len();

    for _ in 0..100 {
        if final_deltas[..num_arcs]
            .iter()
            .any(|delta| delta.abs() >= EPSILON_3)
        {
            break;
        }
        for delta in &mut final_deltas[..num_arcs] {
            *delta *= 2.0;
        }
    }

    *changes_applied += 1;
    let cfg = nodes[node_idx].require_cfg(node_idx)?;
    if cfg_is_valid(cfg, &final_deltas) {
        apply_changes_to_config_and_bounding_boxes(
            node_idx,
            &final_deltas,
            RadiusPolicy::Grow,
            options,
            nodes,
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(super) fn check_and_fix_intersections(
    node_idx: usize,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<Option<usize>, LayoutError> {
    let mut check_tree = true;
    while check_tree {
        check_tree = false;

        if options.checks.ancestors
            && node_idx != 0
            && let Some(changed_node_idx) =
                check_node_against_ancestors(node_idx, options, nodes, changes_applied)?
        {
            return Ok(Some(changed_node_idx));
        }

        for i in 0..nodes[node_idx].children.len() {
            let child_idx = nodes[node_idx].children[i];
            if let Some(changed_idx) =
                check_and_fix_intersections(child_idx, options, nodes, changes_applied)?
            {
                if changed_idx < node_idx {
                    return Ok(Some(changed_idx));
                } else if changed_idx == node_idx {
                    check_tree = true;
                    break;
                }
            }
        }

        if check_tree {
            continue;
        }

        if options.checks.siblings && node_idx != 0 {
            match check_siblings(node_idx, options, nodes, changes_applied)? {
                SiblingCheck::Stable => {}
                SiblingCheck::Changed => check_tree = true,
                SiblingCheck::ChangeLimitReached => return Ok(None),
            }
        }
    }

    if options.behavior.optimize && node_idx != 0 {
        let parent_idx = nodes[node_idx].require_parent(node_idx)?;
        let should_optimize = parent_idx == 0 || {
            let node_cfg = nodes[node_idx].require_cfg(node_idx)?;
            node_cfg.radius > 10.0 * node_cfg.default_radius
        };
        if should_optimize {
            optimize_tree(node_idx, options, nodes, changes_applied)?;
        }
    }

    Ok(None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SiblingCheck {
    Stable,
    Changed,
    ChangeLimitReached,
}

fn check_siblings(
    node_idx: usize,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<SiblingCheck, LayoutError> {
    let child_count = nodes[node_idx].children.len();
    if child_count < 2 {
        return Ok(SiblingCheck::Stable);
    }

    let mut intersections = Vec::new();
    for i in 0..child_count {
        for j in (i + 1)..child_count {
            let child_i = nodes[node_idx].children[i];
            let child_j = nodes[node_idx].children[j];
            if intersect_trees(child_i, child_j, nodes) {
                intersections.push((i, j));
            }
        }
    }

    if intersections.is_empty() {
        return Ok(SiblingCheck::Stable);
    }

    if *changes_applied > options.max_config_changes {
        return Ok(SiblingCheck::ChangeLimitReached);
    }

    let mut changed = false;
    for (left, right) in intersections {
        if fix_intersection_of_siblings(node_idx, left, right, options, nodes, changes_applied)? {
            changed = true;
            break;
        }
    }

    Ok(if changed {
        SiblingCheck::Changed
    } else {
        SiblingCheck::Stable
    })
}

fn fix_intersection_of_siblings(
    node_idx: usize,
    left: usize,
    right: usize,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<bool, LayoutError> {
    let (min_angle, _) = get_bounding_wedge(node_idx, right, nodes)?;
    let (_, max_angle) = get_bounding_wedge(node_idx, left, nodes)?;
    let mut target_angle = min_angle - max_angle;

    if target_angle < 0.0 {
        target_angle = target_angle.max(-PI / 2.0);
        let mut deltas = vec![0.0; nodes[node_idx].children.len() + 1];
        let parent_idx = nodes[node_idx].parent();
        let config_size = deltas.len();

        let delta_request = DeltaRequest {
            node_idx,
            recursive_end: parent_idx,
            index_left: Boundary::for_child(left, config_size)?,
            index_right: Boundary::for_child(right, config_size)?,
            delta_angle: -target_angle,
        };
        let changed_angle = calc_deltas(delta_request, options, &mut deltas, nodes)?;

        if changed_angle != 0.0 {
            return check_and_apply_config_changes(
                node_idx,
                &deltas,
                options,
                nodes,
                changes_applied,
            );
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{
        Boundary, DeltaRequest, IntersectionScratch, IntersectionType, RECOGNIZE_DISTANCE,
        calc_deltas_nearest_neighbors_first_decrease, collect_ancestor_nodes,
        collect_subtree_nodes, intersect_node_lists, intersect_node_pair,
        intersect_optimization_lists, mark_delta_directions, node_inflation,
    };
    use crate::drawing::geometry::approx_eq;
    use crate::drawing::testing::RNASE_P2_STRUCTURE;

    /// Structures with enough branching for neighbouring nodes to interact.
    /// The short ones have no intersecting node pair in their seed tree, so
    /// only `RNase` P2 gives the predicates below a positive verdict to agree on.
    const TREE_CASES: [&str; 5] = [
        "(((...)))",
        "((((.....))))..((((....))))",
        "(((..(((...)))..(((...)))..)))",
        "((((((...))))..((..((...))..))..((...))...))",
        RNASE_P2_STRUCTURE,
    ];

    #[test]
    fn node_pair_intersection_verdict_is_symmetric() {
        // `intersect_list_self` tests each unordered pair once, which is only
        // sound because swapping the arguments cannot change whether the pair
        // intersects: SxL mirrors LxS, LxB mirrors BxL, SxB mirrors BxS, and
        // the remaining cases and parent guards are symmetric already.
        for structure in TREE_CASES {
            let (nodes, _) = super::super::test_tree(structure);
            for left in 0..nodes.len() {
                for right in 0..nodes.len() {
                    let forward = intersect_node_pair(left, &nodes[left], right, &nodes[right]);
                    let backward = intersect_node_pair(right, &nodes[right], left, &nodes[left]);
                    assert_eq!(
                        forward == IntersectionType::NoIntersection,
                        backward == IntersectionType::NoIntersection,
                        "asymmetric verdict for nodes {left}/{right} of {structure}: \
                         {forward:?} vs {backward:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn broad_phase_self_scan_matches_the_exhaustive_scan() {
        // The sweep skips pairs whose inflated boxes are disjoint. Those are
        // exactly the pairs `intersect_node_pair` rejects on its own bounding
        // box test, so the verdict has to be identical.
        let mut scratch = IntersectionScratch::default();
        for structure in TREE_CASES {
            let (nodes, options) = super::super::test_tree(structure);
            let mut subtree = Vec::new();
            for root_child in &nodes[0].children {
                subtree.clear();
                collect_subtree_nodes(*root_child, &nodes, &mut subtree);

                let fast =
                    intersect_optimization_lists(&subtree, &[], &mut scratch, &options, &nodes);
                let exhaustive = intersect_node_lists(&subtree, &subtree, &options, &nodes);
                assert_eq!(
                    fast, exhaustive,
                    "broad phase disagreed on subtree {subtree:?} of {structure}"
                );
            }
        }
    }

    #[test]
    fn broad_phase_cross_scan_matches_the_exhaustive_scan() {
        // This is the exact subtree/ancestor relationship used by the optimizer.
        // Ancestor lists include the exterior node, so it also verifies that its
        // special predicate retains the exhaustive scan's behavior.
        let mut scratch = IntersectionScratch::default();
        for structure in TREE_CASES {
            let (nodes, options) = super::super::test_tree(structure);
            for node_idx in 0..nodes.len() {
                let mut subtree = Vec::new();
                let mut ancestors = Vec::new();
                collect_subtree_nodes(node_idx, &nodes, &mut subtree);
                collect_ancestor_nodes(node_idx, &nodes, &mut ancestors);

                let fast = intersect_optimization_lists(
                    &subtree,
                    &ancestors,
                    &mut scratch,
                    &options,
                    &nodes,
                );
                let exhaustive = intersect_node_lists(&subtree, &subtree, &options, &nodes)
                    || intersect_node_lists(&subtree, &ancestors, &options, &nodes);
                assert_eq!(
                    fast, exhaustive,
                    "cross-list broad phase disagreed for node {node_idx} of {structure}"
                );
            }
        }
    }

    #[test]
    fn node_inflation_covers_the_pair_gap_it_stands_in_for() {
        // Pruning is only sound while inflation(a) + inflation(b) is never
        // smaller than the gap `intersect_node_pair` actually tests with.
        for structure in TREE_CASES {
            let (nodes, _) = super::super::test_tree(structure);
            for left in &nodes {
                for right in &nodes {
                    let (Some(first), Some(second)) = (left.geometry(), right.geometry()) else {
                        continue;
                    };
                    let mut expected = RECOGNIZE_DISTANCE;
                    let mut count = 0;
                    if first.stem_box.bulge_dist > 0.0 {
                        count += 1;
                    }
                    if second.stem_box.bulge_dist > 0.0 {
                        count += 1;
                    }
                    if count > 0 {
                        expected += (first.stem_box.bulge_dist + second.stem_box.bulge_dist)
                            / f64::from(count);
                    }
                    let inflated = node_inflation(left) + node_inflation(right);
                    assert!(
                        inflated >= expected,
                        "inflation {inflated} under-covers gap {expected} in {structure}"
                    );
                }
            }
        }
    }

    fn child_boundary(child: usize, config_size: usize) -> Boundary {
        Boundary::for_child(child, config_size).expect("test child boundary must be valid")
    }

    fn direction_masks(
        index_left: Boundary,
        index_right: Boundary,
        space: &[f64],
    ) -> (Vec<bool>, Vec<bool>) {
        let mut increase = vec![false; space.len()];
        let mut decrease = vec![false; space.len()];
        mark_delta_directions(
            DeltaRequest {
                node_idx: 0,
                recursive_end: None,
                index_left,
                index_right,
                delta_angle: 0.0,
            },
            space,
            &mut increase,
            &mut decrease,
        );
        (increase, decrease)
    }

    #[test]
    fn direction_masks_select_requested_and_available_complementary_arcs() {
        let config_size = 5;
        let space = [1.0, 0.0, 1.0, 0.0, 1.0];

        let (increase, decrease) =
            direction_masks(Boundary::parent(), child_boundary(1, config_size), &space);
        assert_eq!(increase, [true, true, false, false, false]);
        assert_eq!(decrease, [false, false, true, false, true]);

        let (increase, decrease) = direction_masks(
            child_boundary(2, config_size),
            child_boundary(0, config_size),
            &space,
        );
        assert_eq!(increase, [true, false, false, true, true]);
        assert_eq!(decrease, [false, false, true, false, false]);
    }

    #[test]
    fn nearest_neighbor_decrease_prioritizes_closest_arcs() {
        let config_size = 5;
        let decrease = [false, false, true, true, true];
        let space = [1.0; 5];
        let mut deltas = [0.0; 5];

        let remaining = calc_deltas_nearest_neighbors_first_decrease(
            1.0,
            Boundary::parent(),
            child_boundary(1, config_size),
            config_size,
            &decrease,
            &space,
            &mut deltas,
        );

        assert!(approx_eq(remaining, 0.0, f64::EPSILON, 4));
        assert!(
            deltas
                .iter()
                .zip([0.0, 0.0, -0.25, -0.5, -0.25])
                .all(|(&actual, expected)| approx_eq(actual, expected, f64::EPSILON, 4))
        );
    }
}
