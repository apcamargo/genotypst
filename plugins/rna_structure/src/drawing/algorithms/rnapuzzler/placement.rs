use super::{
    EXTERIOR_Y, LayoutError, PairTable, TreeNode, Vec2, distance_to_angle, intersect_trees,
    translate_bounding_boxes,
};
use crate::drawing::geometry::approx_eq;

pub(super) fn determine_nucleotide_coords(
    node_idx: usize,
    pair_table: &PairTable,
    paired_distance: f64,
    coords: &mut [Vec2],
    nodes: &[TreeNode],
) -> Result<(), LayoutError> {
    let node = &nodes[node_idx];

    if let Some(stem) = node.stem() {
        let stem_start = stem.stem_start;
        let sbox = node.require_stem_box(node_idx)?;

        let mut left_bulges = 0;
        let mut right_bulges = 0;
        for bulge in &sbox.bulges {
            if bulge.side < 0.0 {
                right_bulges += 1;
            } else {
                left_bulges += 1;
            }
        }

        let nt_start = stem_start;
        let nt_end = stem.loop_start;
        let nt_segments = nt_end - nt_start - left_bulges;

        let p_start = sbox.center - sbox.a * sbox.half_extents.x + sbox.b * sbox.half_extents.y;
        let p_end = sbox.center + sbox.a * sbox.half_extents.x + sbox.b * sbox.half_extents.y;

        let mut current_bulge = 0;
        for nt in nt_start..nt_end {
            if pair_table.raw_partner(nt) == 0 {
                coords[nt - 1] = sbox.bulge_xy(current_bulge);
                current_bulge += 1;
            } else {
                // Match C's operation order (multiply integer numerator by the
                // delta, then divide) so the seed layout is bit-identical.
                let k = (nt - nt_start - current_bulge) as f64;
                coords[nt - 1] = p_start + (p_end - p_start) * k / (nt_segments as f64);
            }
        }
        coords[nt_end - 1] = p_end;

        let nt_start_r = pair_table.raw_partner(stem.loop_start);
        let nt_end_r = pair_table.raw_partner(stem_start);
        let nt_segments_r = nt_end_r
            .saturating_sub(nt_start_r)
            .saturating_sub(right_bulges);

        let p_start_r = sbox.center + sbox.a * sbox.half_extents.x - sbox.b * sbox.half_extents.y;
        let p_end_r = sbox.center - sbox.a * sbox.half_extents.x - sbox.b * sbox.half_extents.y;

        for nt in nt_start_r..nt_end_r {
            if pair_table.raw_partner(nt) == 0 {
                coords[nt - 1] = sbox.bulge_xy(current_bulge);
                current_bulge += 1;
            } else {
                let right_bulges_seen = current_bulge.saturating_sub(left_bulges);
                let k = nt
                    .saturating_sub(nt_start_r)
                    .saturating_sub(right_bulges_seen) as f64;
                coords[nt - 1] = p_start_r + (p_end_r - p_start_r) * k / (nt_segments_r as f64);
            }
        }
        coords[nt_end_r - 1] = p_end_r;
    }

    if let Some(stem) = node.stem() {
        let cfg = &stem.cfg;
        let lbox = node.require_loop_box(node_idx)?;
        let center = lbox.center;
        let radius = cfg.radius;
        let paired_angle = distance_to_angle(paired_distance, radius);

        let sbox = node.require_stem_box(node_idx)?;
        let mut start_angle = (sbox.center - center).angle();
        start_angle -= paired_angle / 2.0;

        let mut nt = stem.loop_start;
        for arc in &cfg.arcs {
            let number_of_arc_segments = arc.segment_count;
            let arc_angle = arc.arc_angle;

            for arc_segment in 1..number_of_arc_segments {
                let segment_angle = start_angle
                    - (arc_segment as f64)
                        * ((arc_angle - paired_angle) / (number_of_arc_segments as f64));
                coords[nt] = center + Vec2::new(segment_angle.cos(), segment_angle.sin()) * radius;
                nt += 1;
            }
            nt = pair_table.raw_partner(nt + 1);
            start_angle -= arc_angle;
        }
    }

    for &child_idx in &node.children {
        determine_nucleotide_coords(child_idx, pair_table, paired_distance, coords, nodes)?;
    }
    Ok(())
}

pub(super) fn place_exterior_bases(
    pair_table: &PairTable,
    unpaired_distance: f64,
    coords: &mut [Vec2],
) {
    let length = pair_table.len();
    if length < 1 {
        return;
    }
    // Only set coords[0] if base 1 is genuinely exterior (unpaired).
    // If base 1 is paired, determine_nucleotide_coords already placed it correctly.
    if pair_table.raw_partner(1) == 0 {
        coords[0] = Vec2::new(EXTERIOR_Y, EXTERIOR_Y);
    }

    let start = if pair_table.raw_partner(1) != 0 {
        pair_table.raw_partner(1) + 1
    } else {
        2
    };

    let mut nt = start;
    while nt <= length {
        if pair_table.raw_partner(nt) == 0 {
            coords[nt - 1] = coords[nt - 2] + Vec2::new(unpaired_distance, 0.0);
            nt += 1;
        } else {
            nt = pair_table.raw_partner(nt) + 1;
        }
    }
}

pub(super) fn resolve_exterior_children_intersection_xy(
    exterior_idx: usize,
    pair_table: &PairTable,
    unpaired: f64,
    allow_flipping: bool,
    coords: &mut [Vec2],
    nodes: &mut [TreeNode],
) -> Result<(), LayoutError> {
    let subtree_count = nodes[exterior_idx].children.len();
    if subtree_count < 2 {
        return Ok(());
    }

    let child_tree_node = nodes[exterior_idx].children.clone();

    let mut first_base = vec![0; subtree_count];
    let mut backbone = vec![0; subtree_count];
    let mut distance = vec![0.0; subtree_count];

    let mut subtree = 0;
    let mut base = 1;
    let length = pair_table.len();
    while base < length && subtree < subtree_count {
        if pair_table.raw_partner(base) > base {
            first_base[subtree] = base;
            subtree += 1;
            base = pair_table.raw_partner(base);
        } else {
            base += 1;
            backbone[subtree] += 1;
        }
    }

    let mut upper = Vec::new();
    let mut lower = Vec::new();

    upper.push(0);

    let mut offset = 0.0;
    let mut accumulated_translation = 0.0;

    for subtree in 1..subtree_count {
        if offset > 0.0 {
            let translate = Vec2::new(offset, 0.0);
            translate_bounding_boxes(child_tree_node[subtree], translate, nodes)?;
        }

        loop {
            let this = child_tree_node[subtree];
            let intersect_upper = upper
                .iter()
                .any(|&other| intersect_trees(this, child_tree_node[other], nodes));
            let intersect_lower = allow_flipping
                && lower
                    .iter()
                    .any(|&other| intersect_trees(this, child_tree_node[other], nodes));

            if (intersect_lower || !allow_flipping) && intersect_upper {
                distance[subtree] += unpaired;
                let fix_overlap = unpaired * f64::from(backbone[subtree]);
                if approx_eq(fix_overlap, 0.0, 0.0, 2) {
                    break;
                }
                let translate = Vec2::new(fix_overlap, 0.0);
                translate_bounding_boxes(child_tree_node[subtree], translate, nodes)?;
                offset += fix_overlap;
                continue;
            }

            if allow_flipping && intersect_upper {
                lower.push(subtree);
            } else {
                upper.push(subtree);
            }
            break;
        }

        let range_start = pair_table.raw_partner(first_base[subtree - 1]);
        let range_end = first_base[subtree];
        for (idx, coord) in coords[range_start..range_end].iter_mut().enumerate() {
            *coord += Vec2::new(
                ((idx + 1) as f64) * distance[subtree] + accumulated_translation,
                0.0,
            );
        }
        accumulated_translation += distance[subtree] * f64::from(backbone[subtree]);
    }

    let last_range_start = pair_table.raw_partner(first_base[subtree_count - 1]);
    for coord in &mut coords[last_range_start..length] {
        *coord += Vec2::new(accumulated_translation, 0.0);
    }

    let mut translation = 0.0;
    for subtree in 1..subtree_count {
        translation += distance[subtree] * f64::from(backbone[subtree]);
        let start_base = first_base[subtree];
        let end_base = pair_table.raw_partner(first_base[subtree]);
        for coord in &mut coords[start_base..end_base] {
            *coord += Vec2::new(translation, 0.0);
        }

        if lower.contains(&subtree) {
            let exterior_y = coords[0].y;
            for coord in &mut coords[start_base..end_base] {
                coord.y = 2.0 * exterior_y - coord.y;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{ROOT_NODE, layout};
    use super::determine_nucleotide_coords;
    use crate::drawing::config::RnaPuzzlerOptions;
    use crate::drawing::geometry::Vec2;
    use crate::drawing::testing::pair_table;

    /// Places nucleotides from the new tree before exterior placement or
    /// optimization.
    fn placed(structure: &str) -> Vec<Vec2> {
        let (nodes, options) = super::super::test_tree(structure);
        let table = pair_table(structure);
        let mut coords = vec![Vec2::zero(); table.len()];
        determine_nucleotide_coords(
            ROOT_NODE,
            &table,
            options.turtle.paired_distance,
            &mut coords,
            &nodes,
        )
        .expect("placement must succeed");
        coords
    }

    #[test]
    fn every_loop_base_lands_on_its_own_loop_circle() {
        // Loop bases step around their loop circle, so each is one radius from
        // its centre.
        for structure in ["(((...)))", "((((...))..((...))))", "(.((...)).((...)).)"] {
            let coords = placed(structure);
            let (nodes, _) = super::super::test_tree(structure);

            let mut checked = 0;
            for (index, node) in nodes.iter().enumerate().skip(1) {
                let Some(stem) = node.stem() else { continue };
                let loop_box = &node
                    .geometry()
                    .unwrap_or_else(|| panic!("node {index} has no geometry"))
                    .loop_box;
                let table = pair_table(structure);

                // Check only this loop's unpaired bases. `loop_elements` also
                // visits child stems, which use their own circles.
                for element in table.loop_elements(stem.loop_start) {
                    let crate::drawing::validation::LoopElement::Unpaired(base) = element else {
                        continue;
                    };
                    let offset = coords[base - 1].distance(loop_box.center);
                    assert!(
                        (offset - loop_box.radius).abs() < 1e-6,
                        "{structure}: base {base} sits {offset} from its loop centre, \
                         expected {}",
                        loop_box.radius
                    );
                    checked += 1;
                }
            }
            assert!(checked > 0, "{structure}: no loop bases were checked");
        }
    }

    #[test]
    fn paired_bases_in_a_stem_stay_one_paired_distance_apart() {
        // A stem's two rails are `paired_distance` apart, so every pair on them
        // has that spacing.
        let paired = RnaPuzzlerOptions::default().turtle.paired_distance;
        for structure in ["(((...)))", "((((...))..((...))))", "((((((...))))))"] {
            let coords = placed(structure);
            let table = pair_table(structure);

            for (opener, closer) in table.pairs() {
                let distance = coords[opener - 1].distance(coords[closer - 1]);
                assert!(
                    (distance / paired - 1.0).abs() < 1e-6,
                    "{structure}: pair {opener}-{closer} spanned {distance}, expected {paired}"
                );
            }
        }
    }

    #[test]
    fn consecutive_stem_pairs_advance_along_the_stem_axis() {
        // `p_start` to `p_end` interpolation should make the helix rungs parallel
        // and evenly spaced.
        let structure = "((((((...))))))";
        let coords = placed(structure);
        let table = pair_table(structure);
        let pairs: Vec<(usize, usize)> = table.pairs().collect();

        let mut rises = Vec::new();
        for window in pairs.windows(2) {
            let first = coords[window[0].0 - 1];
            let second = coords[window[1].0 - 1];
            rises.push(first.distance(second));

            let rung_a = coords[window[0].1 - 1] - coords[window[0].0 - 1];
            let rung_b = coords[window[1].1 - 1] - coords[window[1].0 - 1];
            assert!(
                rung_a.angle_between(rung_b) < 1e-9,
                "stacked rungs must stay parallel"
            );
        }

        assert!(rises.len() >= 4);
        for rise in &rises {
            assert!(
                (rise - rises[0]).abs() < 1e-9,
                "helix rises must be uniform, got {rises:?}"
            );
        }
    }

    #[test]
    fn a_bulge_is_pushed_off_the_stem_rail() {
        // An unpaired base inside a helix becomes a bulge, off the rail and clear
        // of the paired bases.
        let structure = "(((.(((...))))))";
        let coords = placed(structure);
        let table = pair_table(structure);

        let bulge = (1..=table.len())
            .find(|&base| {
                table.raw_partner(base) == 0
                    && table.raw_partner(base - 1) != 0
                    && table.raw_partner(base + 1) != 0
            })
            .expect("the fixture must contain a single-base bulge");

        let previous = coords[bulge - 2];
        let next = coords[bulge];
        let chord_midpoint = (previous + next) * 0.5;
        let displacement = coords[bulge - 1].distance(chord_midpoint);
        assert!(
            displacement > 1.0,
            "the bulge must sit clear of the rail, was {displacement} away"
        );
    }

    #[test]
    fn the_finished_layout_keeps_the_placement_spacing() {
        // Exterior placement and optimization follow `determine_nucleotide_coords`
        // and must preserve its spacing.
        let structure = "(.((...)).((...)).)";
        let paired = RnaPuzzlerOptions::default().turtle.paired_distance;
        let table = pair_table(structure);
        let result = layout(&table, &RnaPuzzlerOptions::default()).expect("layout must succeed");

        for (opener, closer) in table.pairs() {
            let distance = result.coordinates[opener - 1].distance(result.coordinates[closer - 1]);
            assert!(
                (distance / paired - 1.0).abs() < 1e-6,
                "pair {opener}-{closer} spanned {distance} after the full pipeline"
            );
        }
    }
}
