use super::{
    BaseInfo, EXTERIOR_Y, LayoutError, LoopBox, LoopConfig, LoopElement, NodeGeometry, NodeKind,
    PairTable, ROOT_NODE, RnaPuzzlerOptions, StemNode, TreeNode, Vec2, apothem, internal_invariant,
    make_bulge, missing_node_field, stem_box_from_points,
};
use crate::drawing::geometry::{EPSILON_7, approx_eq};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy)]
pub(super) enum RadiusPolicy {
    Exact(f64),
    Grow,
}

pub(super) fn apply_changes_to_config_and_bounding_boxes(
    node_idx: usize,
    delta_cfg: &[f64],
    radius_policy: RadiusPolicy,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
) -> Result<(), LayoutError> {
    {
        let cfg = &mut nodes[node_idx].require_stem_mut(node_idx)?.cfg;
        for (arc, &delta) in cfg.arcs.iter_mut().zip(delta_cfg.iter()) {
            arc.arc_angle += delta;
        }

        let old_radius = cfg.radius;
        let min_radius =
            super::super::rnaturtle::approximate_config_radius(cfg, options.turtle.paired_distance);
        cfg.min_radius = min_radius;
        cfg.radius = match radius_policy {
            RadiusPolicy::Exact(radius) => radius.max(min_radius),
            RadiusPolicy::Grow if min_radius - 1.0 > old_radius => min_radius,
            RadiusPolicy::Grow => old_radius * 1.05,
        };
    }

    update_bounding_boxes(node_idx, options, nodes)?;
    Ok(())
}

pub(super) fn update_bounding_boxes(
    node_idx: usize,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
) -> Result<(), LayoutError> {
    let parent_idx = nodes[node_idx].parent();
    if let Some(parent_idx) = parent_idx {
        let parent_center = if parent_idx == 0 {
            let lbox = nodes[node_idx].require_loop_box(node_idx)?;
            Vec2::new(lbox.center.x, EXTERIOR_Y)
        } else {
            nodes[parent_idx].require_loop_box(parent_idx)?.center
        };

        let half_extents_x = nodes[node_idx].require_stem_box(node_idx)?.half_extents.x;
        let num_backbones = ((2.0 * half_extents_x) / options.turtle.unpaired_distance).round();
        let stem_length = options.turtle.unpaired_distance * num_backbones;

        let child_angle_rad = if parent_idx == 0 {
            PI
        } else {
            let p_node = &nodes[parent_idx];
            let child_idx_in_parent = p_node
                .children
                .iter()
                .position(|&idx| idx == node_idx)
                .ok_or_else(|| {
                    internal_invariant(format!(
                        "node {node_idx} is missing from parent {parent_idx} children"
                    ))
                })?;
            let p_cfg = p_node.require_cfg(parent_idx)?;
            let mut angle_sum = 0.0;
            for k in 0..=child_idx_in_parent {
                angle_sum += p_cfg.arcs[k].arc_angle;
            }
            angle_sum
        };

        // Compute the stem axis before mutably borrowing this node's box, so the
        // parent's box remains available.
        let stem_a = if parent_idx == 0 {
            Vec2::new(0.0, 1.0)
        } else {
            let parent_a = nodes[parent_idx].require_stem_box(parent_idx)?.a;
            parent_a.rotate(-(child_angle_rad - PI))
        };

        let s0 = if parent_idx == 0 {
            0.0
        } else {
            let r_parent = nodes[parent_idx].require_cfg(parent_idx)?.radius;
            apothem(r_parent, options.turtle.paired_distance)
        };

        {
            let sbox = &mut nodes[node_idx].require_geometry_mut(node_idx)?.stem_box;
            sbox.half_extents.x = 0.5 * stem_length;
            sbox.half_extents.y = 0.5 * options.turtle.paired_distance;

            sbox.a = stem_a;
            sbox.b = sbox.a.perp_left();

            let distance_stem_center = s0 + 0.5 * stem_length;
            sbox.center = parent_center + sbox.a * distance_stem_center;
            if approx_eq(stem_length, 0.0, 0.0, 2) {
                sbox.half_extents.x = EPSILON_7;
            }
        }

        let sbox_center = nodes[node_idx].require_stem_box(node_idx)?.center;
        let sbox_a = nodes[node_idx].require_stem_box(node_idx)?.a;

        let mut lbox = *nodes[node_idx].require_loop_box(node_idx)?;
        let r_curr = nodes[node_idx].require_cfg(node_idx)?.radius;
        let distance_stem_end_to_loop_center = apothem(r_curr, options.turtle.paired_distance);
        let distance_stem_center_to_loop_center =
            0.5 * stem_length + distance_stem_end_to_loop_center;

        lbox.center = sbox_center + sbox_a * distance_stem_center_to_loop_center;
        lbox.radius = r_curr;
        nodes[node_idx].require_geometry_mut(node_idx)?.loop_box = lbox;
        nodes[node_idx].refresh_aabb(node_idx)?;
    }

    let child_count = nodes[node_idx].children.len();
    for i in 0..child_count {
        let child_idx = nodes[node_idx].children[i];
        update_bounding_boxes(child_idx, options, nodes)?;
    }
    Ok(())
}

pub(super) fn translate_bounding_boxes(
    node_idx: usize,
    vector: Vec2,
    nodes: &mut [TreeNode],
) -> Result<(), LayoutError> {
    nodes[node_idx].translate_geometry(vector);

    for i in 0..nodes[node_idx].children.len() {
        let child = nodes[node_idx].children[i];
        translate_bounding_boxes(child, vector, nodes)?;
    }
    Ok(())
}

fn build_node_geometry(
    stem_start: usize,
    loop_start: usize,
    loop_config: &LoopConfig,
    pair_table: &PairTable,
    coords: &[Vec2],
    bulge_dist: f64,
) -> NodeGeometry {
    let end = pair_table.raw_partner(loop_start);
    let current = coords[loop_start - 1];
    let next = coords[loop_start];
    let last = coords[end - 1];

    let go_clockwise = crate::drawing::geometry::point_is_right_of_line(current, next, last);
    let v_pair = current - last;
    let v_normal = v_pair.perp_right();
    let pair_length = v_pair.length();
    let loop_radius = loop_config.radius;
    let center_dist = apothem(loop_radius, pair_length);
    let dir = if go_clockwise { 1.0 } else { -1.0 };
    let center =
        last + v_pair * 0.5 + v_normal.normalized().unwrap_or(Vec2::zero()) * (dir * center_dist);

    let loop_box = LoopBox {
        center,
        radius: loop_radius,
    };

    let stem_pair_idx = pair_table.raw_partner(stem_start);
    let stem_open = coords[stem_start - 1];
    let loop_open = coords[loop_start - 1];
    let stem_pair = coords[stem_pair_idx - 1];

    let mut stem_box = stem_box_from_points(stem_open, loop_open, stem_pair);
    stem_box.bulge_dist = bulge_dist;
    let (axis_dir, normal_dir, stem_center) = (stem_box.a, stem_box.b, stem_box.center);
    stem_box.bulges = (stem_start..loop_start)
        .filter(|&i| pair_table.raw_partner(i) == 0)
        .map(|i| make_bulge(&axis_dir, &normal_dir, stem_center, coords, i, 1.0))
        .chain(
            (pair_table.raw_partner(loop_start)..pair_table.raw_partner(stem_start))
                .filter(|&i| pair_table.raw_partner(i) == 0)
                .map(|i| make_bulge(&axis_dir, &normal_dir, stem_center, coords, i, -1.0)),
        )
        .collect();

    NodeGeometry::new(loop_box, stem_box)
}

struct TreeBuildContext<'a> {
    pair_table: &'a PairTable,
    base_info: &'a [BaseInfo],
    configs: &'a [LoopConfig],
    initial_coords: &'a [Vec2],
    bulge_dist: f64,
}

fn add_stem_node(
    parent_idx: usize,
    stem_start: usize,
    context: &TreeBuildContext<'_>,
    nodes: &mut Vec<TreeNode>,
) -> Result<usize, LayoutError> {
    let loop_start = (stem_start..context.base_info.len())
        .find(|&idx| context.base_info[idx].config.is_some())
        .ok_or_else(|| {
            internal_invariant(format!("stem at base {stem_start} has no loop config"))
        })?;
    let config_idx = context.base_info[loop_start]
        .config
        .ok_or_else(|| missing_node_field(nodes.len(), "cfg"))?;
    let cfg = context
        .configs
        .get(config_idx)
        .ok_or_else(|| internal_invariant(format!("loop config {config_idx} is out of bounds")))?
        .clone();
    let geometry = build_node_geometry(
        stem_start,
        loop_start,
        &cfg,
        context.pair_table,
        context.initial_coords,
        context.bulge_dist,
    );

    let node_idx = nodes.len();

    nodes.push(TreeNode {
        children: Vec::new(),
        kind: NodeKind::Stem(StemNode {
            parent: parent_idx,
            loop_start,
            stem_start,
            cfg,
            geometry,
        }),
    });

    let mut children = Vec::new();
    for element in context.pair_table.loop_elements(loop_start) {
        if let LoopElement::Stem {
            start: stem_start, ..
        } = element
        {
            let child_idx = add_stem_node(node_idx, stem_start, context, nodes)?;
            children.push(child_idx);
        }
    }

    nodes[node_idx].children = children;
    Ok(node_idx)
}

pub(super) fn build_config_tree(
    pair_table: &PairTable,
    base_info: &[BaseInfo],
    configs: &[LoopConfig],
    initial_coords: &[Vec2],
    bulge_dist: f64,
) -> Result<Vec<TreeNode>, LayoutError> {
    let mut nodes = Vec::with_capacity(pair_table.len() / 2 + 1);
    let context = TreeBuildContext {
        pair_table,
        base_info,
        configs,
        initial_coords,
        bulge_dist,
    };

    nodes.push(TreeNode {
        children: Vec::new(),
        kind: NodeKind::Exterior { geometry: None },
    });

    let mut root_children = Vec::new();
    for (stem_start, _) in pair_table.top_level_stems() {
        let child_idx = add_stem_node(ROOT_NODE, stem_start, &context, &mut nodes)?;
        root_children.push(child_idx);
    }

    nodes[ROOT_NODE].children = root_children;

    Ok(nodes)
}

pub(super) fn validate_config_tree(
    nodes: &[TreeNode],
    pair_table: &PairTable,
) -> Result<(), LayoutError> {
    if nodes.is_empty() {
        return Err(internal_invariant("tree has no root node"));
    }

    for (node_idx, node) in nodes.iter().enumerate() {
        match node.parent() {
            Some(parent_idx) if parent_idx >= nodes.len() => {
                return Err(internal_invariant(format!(
                    "node {node_idx} has out-of-bounds parent {parent_idx}"
                )));
            }
            Some(parent_idx) if !nodes[parent_idx].children.contains(&node_idx) => {
                return Err(internal_invariant(format!(
                    "node {node_idx} is missing from parent {parent_idx} children"
                )));
            }
            Some(_) => {}
            None if node_idx == ROOT_NODE => {}
            None => {
                return Err(internal_invariant(format!(
                    "non-root node {node_idx} has no parent"
                )));
            }
        }

        for &child_idx in &node.children {
            if child_idx >= nodes.len() {
                return Err(internal_invariant(format!(
                    "node {node_idx} has out-of-bounds child {child_idx}"
                )));
            }
            if nodes[child_idx].parent() != Some(node_idx) {
                return Err(internal_invariant(format!(
                    "child {child_idx} does not point back to parent {node_idx}"
                )));
            }
        }

        if node_idx == ROOT_NODE {
            if !matches!(node.kind, NodeKind::Exterior { .. }) {
                return Err(internal_invariant("root node is not exterior"));
            }
            continue;
        }

        let stem = node.require_stem(node_idx)?;
        if stem.loop_start == 0 || stem.loop_start > pair_table.len() {
            return Err(internal_invariant(format!(
                "node {node_idx} has invalid loop_start {}",
                stem.loop_start
            )));
        }
        let stem_start = stem.stem_start;
        if stem_start == 0 || stem_start > pair_table.len() {
            return Err(internal_invariant(format!(
                "node {node_idx} has invalid stem_start {stem_start}"
            )));
        }
        node.require_geometry(node_idx)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_tree;

    #[test]
    fn every_node_owns_a_loop_whose_disk_covers_its_stem_junction() {
        // `update_bounding_boxes` derives these boxes from the config. The stem
        // must meet its loop or the helix will detach from its circle.
        for structure in [
            "(((...)))",
            "((((...))..((...))))",
            "(.((...)).((...)).((...)).)",
        ] {
            let (nodes, _) = test_tree(structure);
            for (index, node) in nodes.iter().enumerate().skip(1) {
                let geometry = node
                    .geometry()
                    .unwrap_or_else(|| panic!("{structure}: node {index} has no geometry"));
                let stem = &geometry.stem_box;
                let loop_box = &geometry.loop_box;

                assert!(
                    loop_box.radius > 0.0,
                    "{structure}: node {index} has a degenerate loop"
                );
                assert!(
                    stem.half_extents.x > 0.0 && stem.half_extents.y > 0.0,
                    "{structure}: node {index} has a degenerate stem box"
                );
                // The loop-side edge carries the closing pair. Its endpoints lie
                // on the loop circle, so it is a chord whose midpoint is one
                // apothem inside the centre.
                let axis = stem.a * stem.half_extents.x;
                let normal = stem.b * stem.half_extents.y;
                for corner in [stem.center + axis + normal, stem.center + axis - normal] {
                    let offset = corner.distance(loop_box.center);
                    assert!(
                        (offset - loop_box.radius).abs() < 1e-6,
                        "{structure}: node {index} closing-pair base sits {offset} from \
                         the loop centre, expected the radius {}",
                        loop_box.radius
                    );
                }

                let midpoint = (stem.center + axis).distance(loop_box.center);
                let expected = super::super::apothem(loop_box.radius, 2.0 * stem.half_extents.y);
                assert!(
                    (midpoint - expected).abs() < 1e-6,
                    "{structure}: node {index} chord midpoint sits {midpoint} from the \
                     loop centre, expected the apothem {expected}"
                );
            }
        }
    }

    #[test]
    fn each_loop_config_spans_a_full_turn() {
        // A loop's arcs partition its circle, so their angles must close.
        for structure in [
            "(((...)))",
            "((((...))..((...))))",
            "(.((...)).((...)).((...)).)",
        ] {
            let (nodes, _) = test_tree(structure);
            for (index, node) in nodes.iter().enumerate().skip(1) {
                let Some(stem) = node.stem() else { continue };
                let total: f64 = stem.cfg.arcs.iter().map(|arc| arc.arc_angle).sum();
                assert!(
                    (total - std::f64::consts::TAU).abs() < 1e-6,
                    "{structure}: node {index} arcs sum to {total}, not a full turn"
                );
                assert!(
                    stem.cfg.radius >= stem.cfg.min_radius - 1e-9,
                    "{structure}: node {index} radius fell below its minimum"
                );
            }
        }
    }
}
