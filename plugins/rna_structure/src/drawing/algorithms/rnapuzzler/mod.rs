#![allow(clippy::cast_precision_loss)]

use super::rnaturtle::{
    BaseInfo, LoopConfig, affine_to_cartesian, build_turtle_state, compute_affine_coordinates,
};
use crate::drawing::config::RnaPuzzlerOptions;
use crate::drawing::error::LayoutError;
use crate::drawing::geometry::{Aabb, EPSILON_7, Vec2};
use crate::drawing::output::AlgorithmLayout;
use crate::drawing::validation::LoopElement;
use crate::drawing::validation::PairTable;

mod intersections;
mod optimization;
mod placement;
mod tree;

use intersections::{
    IntersectionScratch, check_and_fix_intersections, collect_ancestor_nodes,
    collect_subtree_nodes, get_bounding_wedge, intersect_optimization_lists, intersect_trees,
    stem_box_from_points,
};
use optimization::optimize_tree;
use placement::{
    determine_nucleotide_coords, place_exterior_bases, resolve_exterior_children_intersection_xy,
};
use tree::{
    RadiusPolicy, apply_changes_to_config_and_bounding_boxes, build_config_tree,
    translate_bounding_boxes, update_bounding_boxes, validate_config_tree,
};

type NodeId = usize;
const ROOT_NODE: NodeId = 0;

#[derive(Debug, Clone)]
struct TreeNode {
    children: Vec<NodeId>,
    kind: NodeKind,
}

#[derive(Debug, Clone)]
enum NodeKind {
    Exterior { geometry: Option<NodeGeometry> },
    Stem(StemNode),
}

#[derive(Debug, Clone)]
struct StemNode {
    parent: NodeId,
    loop_start: usize,
    stem_start: usize,
    cfg: LoopConfig,
    geometry: NodeGeometry,
}

impl TreeNode {
    fn require_cfg(&self, node_idx: usize) -> Result<&LoopConfig, LayoutError> {
        Ok(&self.require_stem(node_idx)?.cfg)
    }

    fn require_stem(&self, node_idx: usize) -> Result<&StemNode, LayoutError> {
        match &self.kind {
            NodeKind::Stem(stem) => Ok(stem),
            NodeKind::Exterior { .. } => Err(missing_node_field(node_idx, "stem data")),
        }
    }

    fn require_stem_mut(&mut self, node_idx: usize) -> Result<&mut StemNode, LayoutError> {
        match &mut self.kind {
            NodeKind::Stem(stem) => Ok(stem),
            NodeKind::Exterior { .. } => Err(missing_node_field(node_idx, "stem data")),
        }
    }

    fn stem(&self) -> Option<&StemNode> {
        match &self.kind {
            NodeKind::Exterior { .. } => None,
            NodeKind::Stem(stem) => Some(stem),
        }
    }

    const fn parent(&self) -> Option<NodeId> {
        match &self.kind {
            NodeKind::Exterior { .. } => None,
            NodeKind::Stem(stem) => Some(stem.parent),
        }
    }

    fn require_loop_box(&self, node_idx: usize) -> Result<&LoopBox, LayoutError> {
        Ok(&self.require_geometry(node_idx)?.loop_box)
    }

    fn require_stem_box(&self, node_idx: usize) -> Result<&StemBox, LayoutError> {
        Ok(&self.require_geometry(node_idx)?.stem_box)
    }

    fn require_geometry(&self, node_idx: usize) -> Result<&NodeGeometry, LayoutError> {
        self.geometry()
            .ok_or_else(|| missing_node_field(node_idx, "geometry"))
    }

    fn require_geometry_mut(&mut self, node_idx: usize) -> Result<&mut NodeGeometry, LayoutError> {
        self.geometry_mut()
            .ok_or_else(|| missing_node_field(node_idx, "geometry"))
    }

    fn require_parent(&self, node_idx: usize) -> Result<usize, LayoutError> {
        self.parent()
            .ok_or_else(|| missing_node_field(node_idx, "parent"))
    }

    fn geometry(&self) -> Option<&NodeGeometry> {
        match &self.kind {
            NodeKind::Exterior { geometry } => geometry.as_ref(),
            NodeKind::Stem(stem) => Some(&stem.geometry),
        }
    }

    fn geometry_mut(&mut self) -> Option<&mut NodeGeometry> {
        match &mut self.kind {
            NodeKind::Exterior { geometry } => geometry.as_mut(),
            NodeKind::Stem(stem) => Some(&mut stem.geometry),
        }
    }

    fn set_exterior_geometry(
        &mut self,
        node_idx: usize,
        geometry: Option<NodeGeometry>,
    ) -> Result<(), LayoutError> {
        match &mut self.kind {
            NodeKind::Exterior {
                geometry: exterior_geometry,
            } => {
                *exterior_geometry = geometry;
                Ok(())
            }
            NodeKind::Stem(_) => Err(internal_invariant(format!(
                "node {node_idx} is not the exterior node"
            ))),
        }
    }

    fn refresh_aabb(&mut self, node_idx: usize) -> Result<(), LayoutError> {
        self.require_geometry_mut(node_idx)?.refresh_aabb();
        Ok(())
    }

    fn translate_geometry(&mut self, vector: Vec2) {
        if let Some(geometry) = self.geometry_mut() {
            geometry.translate(vector);
        }
    }
}

#[derive(Debug, Clone)]
struct NodeGeometry {
    loop_box: LoopBox,
    stem_box: StemBox,
    aabb: Aabb,
}

impl NodeGeometry {
    fn new(loop_box: LoopBox, stem_box: StemBox) -> Self {
        let aabb = compute_node_aabb(&stem_box, &loop_box);
        Self {
            loop_box,
            stem_box,
            aabb,
        }
    }

    fn refresh_aabb(&mut self) {
        self.aabb = compute_node_aabb(&self.stem_box, &self.loop_box);
    }

    fn translate(&mut self, vector: Vec2) {
        self.stem_box.center += vector;
        self.loop_box.center += vector;
        self.refresh_aabb();
    }
}

#[derive(Debug, Clone, Copy)]
struct LoopBox {
    center: Vec2,
    radius: f64,
}

#[derive(Debug, Clone)]
struct StemBox {
    a: Vec2,
    b: Vec2,
    center: Vec2,
    half_extents: Vec2,
    bulge_dist: f64,
    bulges: Vec<Bulge>,
}

impl StemBox {
    fn corners(&self) -> [Vec2; 4] {
        let axis = self.a * self.half_extents.x;
        let normal = self.b * self.half_extents.y;
        [
            self.center - axis + normal,
            self.center + axis + normal,
            self.center + axis - normal,
            self.center - axis - normal,
        ]
    }

    fn side_segments(&self) -> [(Vec2, Vec2); 2] {
        let [a, b, c, d] = self.corners();
        [(a, b), (c, d)]
    }

    fn bulge_coords(&self, index: usize, extra_dist: f64) -> (Vec2, Vec2, Vec2) {
        let bulge = &self.bulges[index];
        let prev =
            self.center + self.a * bulge.prev_offset + self.b * (bulge.side * self.half_extents.y);
        let this = self.center
            + self.a * bulge.center_offset
            + self.b * (bulge.side * (self.half_extents.y + extra_dist + self.bulge_dist));
        let next =
            self.center + self.a * bulge.next_offset + self.b * (bulge.side * self.half_extents.y);
        (prev, this, next)
    }

    fn bulge_xy(&self, index: usize) -> Vec2 {
        self.bulge_coords(index, 0.0).1
    }

    fn bulge_circle(&self, index: usize) -> (Vec2, f64) {
        let (p0, p1, p2) = self.bulge_coords(index, 0.0);
        crate::drawing::geometry::circle_from_three_points(p0, p1, p2)
            .map_or((p1, 1.0), |circle| (circle.center, circle.radius))
    }
}

#[derive(Debug, Clone, Copy)]
struct Bulge {
    side: f64,
    prev_offset: f64,
    center_offset: f64,
    next_offset: f64,
}

const RECOGNIZE_DISTANCE: f64 = 14.0;
const FIX_DISTANCE: f64 = 19.0;
const EXTERIOR_Y: f64 = 0.0;
const MIN_POSITIVE_ANGLE: f64 = 1e-10;
const MIN_NEGATIVE_ANGLE: f64 = -1e-10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntersectionType {
    NoIntersection,
    LxL,
    LxS,
    LxB,
    SxL,
    SxS,
    SxB,
    BxL,
    BxS,
    BxB,
}

/// Distance from a circle's centre to the midpoint of a chord of length
/// `chord`, clamped to zero for degenerate cases.
fn apothem(radius: f64, chord: f64) -> f64 {
    (radius * radius - 0.25 * chord * chord).max(0.0).sqrt()
}

/// Port of C's `distanceToAngle`: central angle for a chord of length `distance`
/// on a circle with the given `radius`.
fn distance_to_angle(distance: f64, radius: f64) -> f64 {
    (distance / (2.0 * radius)).asin() * 2.0
}

/// Expands `[min, max]` to include `value`. The explicit comparisons preserve
/// the reference's NaN behaviour.
fn expand_range(min: &mut f64, max: &mut f64, value: f64) {
    if value < *min {
        *min = value;
    }
    if value > *max {
        *max = value;
    }
}

fn make_bulge(
    a_dir: &Vec2,
    b_dir: &Vec2,
    stem_center: Vec2,
    coords: &[Vec2],
    i: usize,
    side: f64,
) -> Bulge {
    let p_prev = coords[i - 1 - 1];
    let p_this = coords[i - 1];
    let p_next = coords[i + 1 - 1];

    let get_a = |p: Vec2| -> f64 {
        let p_rel = p - stem_center;
        let denom = a_dir.cross(*b_dir);
        if denom.abs() < EPSILON_7 {
            p_rel.dot(*a_dir)
        } else {
            (p_rel.x * b_dir.y - p_rel.y * b_dir.x) / denom
        }
    };

    Bulge {
        side,
        prev_offset: get_a(p_prev),
        center_offset: get_a(p_this),
        next_offset: get_a(p_next),
    }
}

fn compute_node_aabb(sbox: &StemBox, lbox: &LoopBox) -> Aabb {
    let mut bounds = Aabb::empty();
    for corner in sbox.corners() {
        bounds.include(corner);
    }
    let loop_radius = Vec2::new(lbox.radius, lbox.radius);
    bounds.include(lbox.center - loop_radius);
    bounds.include(lbox.center + loop_radius);

    for i in 0..sbox.bulges.len() {
        bounds.include(sbox.bulge_xy(i));
    }

    bounds
}

fn internal_invariant(message: impl Into<String>) -> LayoutError {
    LayoutError::InternalInvariant {
        message: message.into(),
    }
}

fn missing_node_field(node_idx: usize, field: &str) -> LayoutError {
    internal_invariant(format!("node {node_idx} is missing {field}"))
}

/// Computes RNA secondary-structure coordinates with the `RNApuzzler` algorithm.
///
/// # Errors
///
/// Returns [`LayoutError`] when the structure cannot be laid out.
pub(crate) fn layout(
    pair_table: &PairTable,
    options: &RnaPuzzlerOptions,
) -> Result<AlgorithmLayout, LayoutError> {
    let n = pair_table.len();
    if !pair_table.has_pairs() {
        return Ok(AlgorithmLayout::unpaired_line(
            pair_table,
            options.turtle.unpaired_distance,
        ));
    }

    let mut state = build_turtle_state(
        pair_table,
        options.turtle.paired_distance,
        options.turtle.unpaired_distance,
    );
    compute_affine_coordinates(
        pair_table,
        options.turtle.paired_distance,
        options.turtle.unpaired_distance,
        &mut state.base_info,
        &state.configs,
    )?;

    let initial_coords = affine_to_cartesian(&state.base_info);
    let dist_bulge = apothem(
        options.turtle.unpaired_distance,
        options.turtle.unpaired_distance,
    );

    let mut nodes = build_config_tree(
        pair_table,
        &state.base_info,
        &state.configs,
        &initial_coords,
        dist_bulge,
    )?;
    validate_config_tree(&nodes, pair_table)?;

    let mut changes_applied = 0;
    if options.checks.exterior || options.checks.siblings || options.checks.ancestors {
        update_bounding_boxes(ROOT_NODE, options, &mut nodes)?;
        check_and_fix_intersections(ROOT_NODE, options, &mut nodes, &mut changes_applied)?;
    }

    let mut coordinates = vec![Vec2::zero(); n];
    determine_nucleotide_coords(
        ROOT_NODE,
        pair_table,
        options.turtle.paired_distance,
        &mut coordinates,
        &nodes,
    )?;

    place_exterior_bases(
        pair_table,
        options.turtle.unpaired_distance,
        &mut coordinates,
    );

    resolve_exterior_children_intersection_xy(
        ROOT_NODE,
        pair_table,
        options.turtle.unpaired_distance,
        options.behavior.allow_flipping,
        &mut coordinates,
        &mut nodes,
    )?;

    let backbone =
        crate::drawing::arcs::generate_backbone(pair_table, &coordinates, options.turtle.draw_arcs);

    Ok(AlgorithmLayout::new(
        coordinates,
        backbone,
        options.turtle.unpaired_distance,
    ))
}

/// Builds the same config tree as `layout`, stopping before intersection checks.
#[cfg(test)]
fn test_tree(structure: &str) -> (Vec<TreeNode>, RnaPuzzlerOptions) {
    let sequence = "A".repeat(structure.len());
    let input =
        crate::drawing::validation::validate(&sequence, structure).expect("valid test input");
    let pair_table = &input.pair_table;
    let options = match crate::drawing::config::parse(br#"{"algorithm":"rna_puzzler"}"#)
        .expect("default puzzler config must parse")
    {
        crate::drawing::config::LayoutConfig::RnaPuzzler(options) => options,
        other => panic!("expected a puzzler config, got {other:?}"),
    };

    let mut state = build_turtle_state(
        pair_table,
        options.turtle.paired_distance,
        options.turtle.unpaired_distance,
    );
    compute_affine_coordinates(
        pair_table,
        options.turtle.paired_distance,
        options.turtle.unpaired_distance,
        &mut state.base_info,
        &state.configs,
    )
    .expect("affine coordinates");
    let initial_coords = affine_to_cartesian(&state.base_info);
    let dist_bulge = apothem(
        options.turtle.unpaired_distance,
        options.turtle.unpaired_distance,
    );
    let mut nodes = build_config_tree(
        pair_table,
        &state.base_info,
        &state.configs,
        &initial_coords,
        dist_bulge,
    )
    .expect("config tree");
    update_bounding_boxes(ROOT_NODE, &options, &mut nodes).expect("bounding boxes");
    (nodes, options)
}

/// Like `test_tree`, then resolves intersections as `layout` does.
#[cfg(test)]
fn test_tree_resolved(structure: &str) -> (Vec<TreeNode>, RnaPuzzlerOptions) {
    let (mut nodes, options) = test_tree(structure);
    let mut changes_applied = 0;
    check_and_fix_intersections(ROOT_NODE, &options, &mut nodes, &mut changes_applied)
        .expect("intersection resolution");
    (nodes, options)
}

#[cfg(test)]
mod tests {
    use super::intersections::intersect_node_pair;
    use super::{
        IntersectionType, LoopBox, StemBox, TreeNode, apothem, compute_node_aabb,
        distance_to_angle, expand_range, layout, test_tree, test_tree_resolved,
    };
    use crate::drawing::config::RnaPuzzlerOptions;
    use crate::drawing::geometry::Vec2;
    use crate::drawing::output::AlgorithmLayout;
    use crate::drawing::testing::{RNASE_P2_SEQUENCE, RNASE_P2_STRUCTURE, pair_table};
    use crate::drawing::validation::validate;

    fn puzzler(structure: &str, options: &RnaPuzzlerOptions) -> AlgorithmLayout {
        layout(&pair_table(structure), options).expect("RNApuzzler layout must succeed")
    }

    /// Structures whose branches genuinely compete for space.
    const CROWDED: [&str; 4] = [
        "((((((...))))..((..((...))..))..((...))...))",
        "(((..(((...)))..(((...)))..)))",
        "((((.....))))..((((....))))",
        "(.((...)).((...)).((...)).((...)).)",
    ];

    #[test]
    fn the_apothem_and_chord_angle_are_mutual_inverses() {
        // Together these convert between a chord length and its place on the
        // loop circle. A mismatch between them shears the whole loop.
        for radius in [1.0_f64, 7.5, 40.0] {
            for fraction in [0.1_f64, 0.5, 0.9, 1.0] {
                let chord = 2.0 * radius * fraction;
                let angle = distance_to_angle(chord, radius);
                // The chord subtending `angle` on this circle is the one we started with.
                let recovered = 2.0 * radius * (angle / 2.0).sin();
                assert!(
                    (recovered - chord).abs() < 1e-9,
                    "radius {radius} chord {chord}: recovered {recovered}"
                );

                // Apothem, chord half-length and radius form a right triangle.
                let apothem = apothem(radius, chord);
                assert!(
                    (apothem.hypot(chord / 2.0) - radius).abs() < 1e-9,
                    "radius {radius} chord {chord}: apothem {apothem} breaks the triangle"
                );
            }
        }

        // A chord wider than the circle is degenerate. Clamp it before the
        // square root to avoid NaN.
        assert!((apothem(1.0, 10.0)).abs() < 1e-12);

        // A half-circle chord is the diameter, subtending pi.
        assert!((distance_to_angle(2.0, 1.0) - std::f64::consts::PI).abs() < 1e-9);
        assert!((distance_to_angle(1.0, 1.0) - std::f64::consts::FRAC_PI_3).abs() < 1e-9);
    }

    #[test]
    fn expand_range_widens_only_in_the_direction_that_needs_it() {
        let (mut min, mut max) = (0.0, 10.0);
        expand_range(&mut min, &mut max, 5.0);
        assert!((min - 0.0).abs() < 1e-12 && (max - 10.0).abs() < 1e-12);

        expand_range(&mut min, &mut max, -3.0);
        assert!((min + 3.0).abs() < 1e-12 && (max - 10.0).abs() < 1e-12);

        expand_range(&mut min, &mut max, 12.0);
        assert!((min + 3.0).abs() < 1e-12 && (max - 12.0).abs() < 1e-12);

        // The reference compares with `<`/`>`, so a NaN widens nothing.
        expand_range(&mut min, &mut max, f64::NAN);
        assert!((min + 3.0).abs() < 1e-12 && (max - 12.0).abs() < 1e-12);
    }

    #[test]
    fn a_node_aabb_contains_its_stem_corners_and_whole_loop_disk() {
        let sbox = StemBox {
            center: Vec2::new(10.0, 4.0),
            a: Vec2::new(1.0, 0.0),
            b: Vec2::new(0.0, 1.0),
            half_extents: Vec2::new(3.0, 2.0),
            bulges: Vec::new(),
            bulge_dist: 0.0,
        };
        let lbox = LoopBox {
            center: Vec2::new(20.0, 4.0),
            radius: 5.0,
        };

        let bounds = compute_node_aabb(&sbox, &lbox);
        for corner in sbox.corners() {
            assert!(
                corner.x >= bounds.min.x - 1e-9
                    && corner.x <= bounds.max.x + 1e-9
                    && corner.y >= bounds.min.y - 1e-9
                    && corner.y <= bounds.max.y + 1e-9,
                "corner {corner:?} escaped {bounds:?}"
            );
        }
        // The loop is a disk, so its extremes are centre plus or minus radius.
        assert!((bounds.max.x - 25.0).abs() < 1e-9);
        assert!((bounds.min.y + 1.0).abs() < 1e-9);
        assert!((bounds.max.y - 9.0).abs() < 1e-9);
        assert!((bounds.min.x - 7.0).abs() < 1e-9);
    }

    #[test]
    fn the_config_tree_mirrors_the_stem_tree_of_the_structure() {
        // Node 0 is the exterior loop. Every stem contributes exactly one node.
        let cases = [
            ("(((...)))", 1_usize, 1_usize),
            ("(((...))).((...))", 2, 2),
            ("((((...))..((...))))", 3, 1),
            ("(.((...)).((...)).((...)).)", 4, 1),
        ];

        for (structure, expected_stems, expected_root_children) in cases {
            let (nodes, _) = test_tree(structure);
            let table = pair_table(structure);
            assert_eq!(
                nodes.len(),
                expected_stems + 1,
                "{structure}: one exterior node plus one node per helix"
            );
            assert_eq!(
                nodes[0].children.len(),
                expected_root_children,
                "{structure}: wrong number of top-level helices"
            );
            // Each stem node starts at a distinct helix opener.
            let mut starts: Vec<usize> = nodes
                .iter()
                .filter_map(|node| node.stem().map(|stem| stem.stem_start))
                .collect();
            starts.sort_unstable();
            starts.dedup();
            assert_eq!(
                starts.len(),
                expected_stems,
                "{structure}: a helix was registered twice or not at all"
            );
            for start in starts {
                assert!(
                    table.raw_partner(start) > start,
                    "{structure}: stem start {start} is not a helix opener"
                );
            }
            // Every non-root node must be reachable from the root exactly once.
            let mut seen = vec![0_usize; nodes.len()];
            let mut stack = vec![0_usize];
            while let Some(index) = stack.pop() {
                for &child in &nodes[index].children {
                    seen[child] += 1;
                    stack.push(child);
                }
            }
            assert!(
                seen.iter().skip(1).all(|&count| count == 1),
                "{structure}: the config tree must be a tree, saw {seen:?}"
            );
        }
    }

    /// Nucleotides that are far apart along the backbone but overlap in the
    /// drawing. This is the visible failure to test.
    fn overlapping_nucleotides(layout: &AlgorithmLayout, threshold: f64) -> usize {
        let mut count = 0;
        for (first, left) in layout.coordinates.iter().enumerate() {
            for right in layout.coordinates.iter().skip(first + 3) {
                if left.distance(*right) < threshold {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn rna_puzzler_resolves_the_overlaps_left_by_its_rnaturtle_seed() {
        // RNApuzzler starts from an RNAturtle layout and must separate these
        // overlapping branches.
        let threshold = 0.8 * RnaPuzzlerOptions::default().turtle.unpaired_distance;
        let cases = [
            ".((...))((...))((...))((...))((...))((...)).",
            RNASE_P2_STRUCTURE,
        ];

        for structure in cases {
            let table = pair_table(structure);
            let seed = crate::drawing::algorithms::rnaturtle::layout(
                &table,
                &RnaPuzzlerOptions::default().turtle,
            )
            .expect("the RNAturtle seed must lay out");
            let resolved =
                layout(&table, &RnaPuzzlerOptions::default()).expect("RNApuzzler must lay out");

            let before = overlapping_nucleotides(&seed, threshold);
            let after = overlapping_nucleotides(&resolved, threshold);

            assert!(
                before > 0,
                "the fixture must actually overlap under RNAturtle, \
                 or this test proves nothing"
            );
            assert_eq!(
                after, 0,
                "RNApuzzler left {after} overlapping nucleotide pairs \
                 (RNAturtle had {before})"
            );
        }
    }

    fn intersecting_node_pairs(nodes: &[TreeNode]) -> usize {
        let mut count = 0;
        for left in 0..nodes.len() {
            for right in (left + 1)..nodes.len() {
                if intersect_node_pair(left, &nodes[left], right, &nodes[right])
                    != IntersectionType::NoIntersection
                {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn resolution_leaves_the_node_tree_free_of_intersections() {
        // The seed tree must intersect, or this proves nothing. After
        // resolution, no two nodes may overlap under the algorithm's predicate.
        // RNase P2's seed has competing branches inside multiloops, which is
        // what the tree-level pass resolves. Exterior-loop siblings are placed
        // later, on coordinates.
        let (seed, _) = test_tree(RNASE_P2_STRUCTURE);
        let (resolved, _) = test_tree_resolved(RNASE_P2_STRUCTURE);

        assert!(
            intersecting_node_pairs(&seed) > 0,
            "the seed tree must intersect for this test to mean anything"
        );
        assert_eq!(intersecting_node_pairs(&resolved), 0);
    }

    #[test]
    fn every_check_and_behavior_switch_is_live() {
        // Each flag must affect at least one structure. Short structures do not
        // exercise placement, so this set includes several shapes and a real
        // molecule with competing branches.
        let structures: [&str; 7] = [
            CROWDED[0],
            CROWDED[1],
            CROWDED[2],
            CROWDED[3],
            ".((...))((...))((...))((...))((...))((...)).",
            "(.(...).(...).(...).(...).(...).(...).)",
            RNASE_P2_STRUCTURE,
        ];
        let baseline = RnaPuzzlerOptions::default();

        let variants: [(&str, RnaPuzzlerOptions); 5] = [
            ("ancestors", {
                let mut o = RnaPuzzlerOptions::default();
                o.checks.ancestors = false;
                o
            }),
            ("siblings", {
                let mut o = RnaPuzzlerOptions::default();
                o.checks.siblings = false;
                o
            }),
            ("exterior", {
                let mut o = RnaPuzzlerOptions::default();
                o.checks.exterior = false;
                o
            }),
            ("allow_flipping", {
                let mut o = RnaPuzzlerOptions::default();
                o.behavior.allow_flipping = true;
                o
            }),
            ("optimize", {
                let mut o = RnaPuzzlerOptions::default();
                o.behavior.optimize = false;
                o
            }),
        ];

        for (name, variant) in variants {
            let mut changed_somewhere = false;
            for structure in structures {
                let base = puzzler(structure, &baseline);
                let other = puzzler(structure, &variant);
                assert_eq!(base.coordinates.len(), other.coordinates.len());
                assert!(
                    other
                        .coordinates
                        .iter()
                        .all(|point| point.x.is_finite() && point.y.is_finite()),
                    "{name} on {structure}: produced non-finite geometry"
                );
                if base
                    .coordinates
                    .iter()
                    .zip(&other.coordinates)
                    .any(|(left, right)| left.distance(*right) > 1e-6)
                {
                    changed_somewhere = true;
                }
            }
            assert!(
                changed_somewhere,
                "{name} never changed a layout, so the switch is inert"
            );
        }
    }

    #[test]
    fn exhausting_the_config_change_budget_still_yields_a_drawing() {
        // The budget limits the search. Exhausting it must keep the best
        // placement, not return an error or NaN geometry.
        let starved = RnaPuzzlerOptions {
            max_config_changes: 0,
            ..RnaPuzzlerOptions::default()
        };

        let mut budget_bound = false;
        for structure in CROWDED {
            let result = puzzler(structure, &starved);
            assert_eq!(result.coordinates.len(), structure.len());
            assert!(
                result
                    .coordinates
                    .iter()
                    .all(|point| point.x.is_finite() && point.y.is_finite())
            );
            let unlimited = puzzler(structure, &RnaPuzzlerOptions::default());
            budget_bound |= result
                .coordinates
                .iter()
                .zip(&unlimited.coordinates)
                .any(|(left, right)| left.distance(*right) > 1e-6);
        }
        // Without this, an ignored budget would pass the checks above.
        assert!(
            budget_bound,
            "a zero budget must stop the search early on at least one structure"
        );
    }

    #[test]
    fn a_nested_zero_length_hairpin_keeps_its_bulge_local() {
        // The degenerate loop must not leak into neighbouring backbone edges.
        let structure = "(((...()...)).)";
        let result = puzzler(structure, &RnaPuzzlerOptions::default());
        let unpaired = RnaPuzzlerOptions::default().turtle.unpaired_distance;

        for (first, second) in [(12, 13), (13, 14)] {
            let distance = result.coordinates[first].distance(result.coordinates[second]);
            assert!(
                (distance / unpaired - 1.0).abs() < 1e-6,
                "bulge edge {}-{} must use unpaired spacing, got {distance}",
                first + 1,
                second + 1
            );
        }
    }

    #[test]
    fn rnase_p2_does_not_displace_the_bulge_remote_from_its_degenerate_loop() {
        // Before the fix, a zero-length hairpin nearly 80 nucleotides away moved
        // this bulge by more than 400 layout units.
        let input = validate(RNASE_P2_SEQUENCE, RNASE_P2_STRUCTURE)
            .expect("the RNase P2 fixture must validate");
        let result = layout(&input.pair_table, &RnaPuzzlerOptions::default())
            .expect("RNase P2 must lay out");
        let unpaired = RnaPuzzlerOptions::default().turtle.unpaired_distance;

        for (first, second) in [(383, 384), (384, 385)] {
            let distance = result.coordinates[first].distance(result.coordinates[second]);
            assert!(
                (distance / unpaired - 1.0).abs() < 1e-6,
                "RNase P2 bulge edge {}-{} must use unpaired spacing, got {distance}",
                first + 1,
                second + 1
            );
        }
    }
}
