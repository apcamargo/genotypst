use crate::drawing::geometry::{Vec2, circle_from_three_points, point_is_right_of_line};
use crate::drawing::output::BackboneArc;
use crate::drawing::validation::{LoopElement, PairTable};

/// Largest relative radial deviation accepted for a constant-radius circle.
const ARC_FIT_TOLERANCE: f64 = 0.02;
/// NAVIEW interpolates loop radii. Above this deviation, the fitted centre is
/// not a useful smooth fallback.
const VARIABLE_ARC_FIT_TOLERANCE: f64 = 0.20;

pub(crate) fn generate_backbone(
    pair_table: &PairTable,
    coords: &[Vec2],
    draw_arcs: bool,
) -> Vec<Option<BackboneArc>> {
    generate_backbone_with_variable_radius(pair_table, coords, draw_arcs, false)
}

pub(crate) fn generate_naview_backbone(
    pair_table: &PairTable,
    coords: &[Vec2],
    draw_arcs: bool,
) -> Vec<Option<BackboneArc>> {
    generate_backbone_with_variable_radius(pair_table, coords, draw_arcs, true)
}

fn generate_backbone_with_variable_radius(
    pair_table: &PairTable,
    coords: &[Vec2],
    draw_arcs: bool,
    allow_variable_radius: bool,
) -> Vec<Option<BackboneArc>> {
    let n = pair_table.len();
    let mut arcs = vec![None; n.saturating_sub(1)];

    if !draw_arcs {
        return arcs;
    }

    for (start, _) in pair_table.top_level_stems() {
        calc_stem_arcs(start, pair_table, coords, &mut arcs, allow_variable_radius);
    }

    arcs
}

fn is_bulge_of_size_1(pair_table: &PairTable, start: usize) -> bool {
    let counts = pair_table.loop_counts(start);
    counts.stems == 1 && counts.unpaired == 1
}

fn is_loop_start(pair_table: &PairTable, i: usize) -> bool {
    if !pair_table.is_paired(i) {
        return false;
    }
    let partner = pair_table.raw_partner(i);
    if partner <= i {
        return false;
    }
    let is_helix_end = pair_table.raw_partner(i + 1) != partner - 1;
    if !is_helix_end {
        return false;
    }
    !is_bulge_of_size_1(pair_table, i)
}

fn calc_stem_arcs(
    start: usize,
    pair_table: &PairTable,
    coords: &[Vec2],
    arcs: &mut [Option<BackboneArc>],
    allow_variable_radius: bool,
) {
    // Stop at this stem's closing partner. A zero-length hairpin defeats
    // `is_loop_start`, so an unbounded scan could enter a sibling or ancestor.
    let limit = pair_table.raw_partner(start);
    if let Some(loop_start) = (start..limit).find(|&index| is_loop_start(pair_table, index)) {
        calc_loop_arcs(loop_start, pair_table, coords, arcs, allow_variable_radius);
    }
}

fn calc_loop_arcs(
    start: usize,
    pair_table: &PairTable,
    coords: &[Vec2],
    arcs: &mut [Option<BackboneArc>],
    allow_variable_radius: bool,
) {
    let end = pair_table.raw_partner(start);
    if end == 0 || start >= end {
        return;
    }

    let mut points = Vec::new();
    // Arc endpoints for this loop. This also includes the loop opener and each
    // stem's 5' base, which are endpoints but not circle-fit samples.
    let mut endpoints = vec![coords[start - 1]];
    for element in pair_table.loop_elements(start) {
        match element {
            LoopElement::Unpaired(idx) => {
                points.push(coords[idx - 1]);
                endpoints.push(coords[idx - 1]);
            }
            LoopElement::Stem {
                start: stem_start,
                end: stem_end,
            } => {
                calc_stem_arcs(stem_start, pair_table, coords, arcs, allow_variable_radius);
                points.push(coords[stem_end - 1]);
                endpoints.push(coords[stem_start - 1]);
                endpoints.push(coords[stem_end - 1]);
            }
        }
    }
    points.push(coords[end - 1]);
    endpoints.push(coords[end - 1]);

    let num_points = points.len();
    if num_points < 3 {
        return;
    }

    let Some(circle) = circle_from_three_points(
        points[0],
        points[num_points / 3],
        points[2 * num_points / 3],
    ) else {
        return;
    };

    let maximum_deviation = endpoints
        .iter()
        .map(|point| (circle.center.distance(*point) - circle.radius).abs())
        .fold(0.0, f64::max);
    let constant_radius = maximum_deviation <= circle.radius * ARC_FIT_TOLERANCE;
    if !constant_radius
        && (!allow_variable_radius
            || maximum_deviation > circle.radius * VARIABLE_ARC_FIT_TOLERANCE)
    {
        return;
    }

    let clockwise =
        point_is_right_of_line(points[num_points - 1], points[0], points[num_points / 2]);

    for element in pair_table.loop_elements(start) {
        let to_idx = match element {
            LoopElement::Unpaired(idx) | LoopElement::Stem { start: idx, .. } => idx,
        };
        add_arc(
            circle.center,
            circle.radius,
            clockwise,
            to_idx,
            coords,
            constant_radius,
            arcs,
        );
    }
    add_arc(
        circle.center,
        circle.radius,
        clockwise,
        end,
        coords,
        constant_radius,
        arcs,
    );
}

fn add_arc(
    center: Vec2,
    radius: f64,
    clockwise: bool,
    to_idx: usize,
    coords: &[Vec2],
    constant_radius: bool,
    arcs: &mut [Option<BackboneArc>],
) {
    if to_idx < 2 {
        return;
    }
    let Some(slot) = arcs.get_mut(to_idx - 2) else {
        return;
    };
    // A loop's segments are disjoint from those of its children, so no slot is
    // ever written twice.
    debug_assert!(slot.is_none());
    let Some(&from) = coords.get(to_idx - 2) else {
        return;
    };
    let Some(&to) = coords.get(to_idx - 1) else {
        return;
    };
    let start_radius = center.distance(from);
    let end_radius = center.distance(to);
    if !start_radius.is_finite() || !end_radius.is_finite() || start_radius <= 0.0 {
        return;
    }
    let segment_clockwise = if constant_radius {
        clockwise
    } else {
        point_is_right_of_line(center, from, to)
    };
    *slot = Some(BackboneArc {
        center,
        radius: if constant_radius {
            radius
        } else {
            start_radius
        },
        end_radius: if constant_radius {
            None
        } else {
            Some(end_radius)
        },
        clockwise: segment_clockwise,
    });
}

#[cfg(test)]
mod tests {
    use super::{generate_backbone, generate_naview_backbone};
    use crate::drawing::geometry::Vec2;
    use crate::drawing::output::BackboneArc;
    use crate::drawing::testing::pair_table;
    use crate::drawing::validation::validate;
    use std::f64::consts::TAU;

    const LOOP_RADIUS: f64 = 100.0;

    /// Places every base of `structure` on one circle of `LOOP_RADIUS`, then
    /// pushes base `perturbed` outward by `deviation` times that radius. The
    /// circle-fit guards are expressed as a fraction of the radius, so this
    /// drives them directly.
    fn circular_coords(length: usize, perturbed: usize, deviation: f64) -> Vec<Vec2> {
        (0..length)
            .map(|index| {
                #[allow(clippy::cast_precision_loss)]
                let angle = TAU * (index as f64) / (length as f64);
                let radius = if index == perturbed {
                    LOOP_RADIUS * (1.0 + deviation)
                } else {
                    LOOP_RADIUS
                };
                Vec2::new(angle.cos() * radius, angle.sin() * radius)
            })
            .collect()
    }

    fn arcs_of(structure: &str, deviation: f64, naview: bool) -> Vec<Option<BackboneArc>> {
        let table = pair_table(structure);
        // Perturb a sampled interior base, not an endpoint.
        let coords = circular_coords(structure.len(), structure.len() / 2, deviation);
        if naview {
            generate_naview_backbone(&table, &coords, true)
        } else {
            generate_backbone(&table, &coords, true)
        }
    }

    fn emitted(arcs: &[Option<BackboneArc>]) -> usize {
        arcs.iter().filter(|arc| arc.is_some()).count()
    }

    /// A hairpin loop wide enough for the three-point fit to sample it.
    const LOOP: &str = "(..........)";

    #[test]
    fn a_circular_loop_becomes_constant_radius_arcs_on_that_circle() {
        let arcs = arcs_of(LOOP, 0.0, false);
        assert_eq!(arcs.len(), LOOP.len() - 1);
        assert!(emitted(&arcs) > 0, "a perfectly circular loop must fit");

        for arc in arcs.iter().flatten() {
            assert!(
                arc.center.distance(Vec2::zero()) < 1e-6,
                "fitted centre {:?} must be the circle's centre",
                arc.center
            );
            assert!((arc.radius - LOOP_RADIUS).abs() < 1e-6);
            assert!(
                arc.end_radius.is_none(),
                "a constant-radius fit must not emit a variable radius"
            );
        }
    }

    #[test]
    fn the_circularity_guard_bands_are_distinct_for_the_two_entry_points() {
        // Below 2%, both paths fit a constant-radius loop. Between 2% and 20%,
        // only NAVIEW keeps the loop as a variable-radius spiral. Past 20%,
        // neither path fits. This is why there are two entry points.
        let below = 0.01;
        let between = 0.10;
        let above = 0.40;

        for naview in [false, true] {
            let arcs = arcs_of(LOOP, below, naview);
            assert!(emitted(&arcs) > 0, "naview={naview}: 1% must still fit");
            assert!(
                arcs.iter().flatten().all(|arc| arc.end_radius.is_none()),
                "naview={naview}: 1% must stay constant radius"
            );
        }

        let plain = arcs_of(LOOP, between, false);
        assert_eq!(
            emitted(&plain),
            0,
            "10% deviation must be rejected without the variable-radius path"
        );
        let variable = arcs_of(LOOP, between, true);
        assert!(
            emitted(&variable) > 0,
            "10% deviation must survive the NAVIEW path"
        );
        assert!(
            variable
                .iter()
                .flatten()
                .all(|arc| arc.end_radius.is_some()),
            "a non-circular NAVIEW loop must report a varying radius"
        );

        for naview in [false, true] {
            assert_eq!(
                emitted(&arcs_of(LOOP, above, naview)),
                0,
                "naview={naview}: 40% deviation must be rejected outright"
            );
        }
    }

    #[test]
    fn variable_radius_arcs_report_their_own_endpoint_radii() {
        let arcs = arcs_of(LOOP, 0.10, true);
        let coords = circular_coords(LOOP.len(), LOOP.len() / 2, 0.10);

        let mut varying = 0;
        for (index, arc) in arcs.iter().enumerate() {
            let Some(arc) = arc else { continue };
            let Some(end_radius) = arc.end_radius else {
                continue;
            };
            // Each endpoint radius must be that nucleotide's true distance
            // from the fitted centre, or the rendered spiral misses the base.
            assert!((arc.radius - arc.center.distance(coords[index])).abs() < 1e-9);
            assert!((end_radius - arc.center.distance(coords[index + 1])).abs() < 1e-9);
            if (arc.radius - end_radius).abs() > 1e-9 {
                varying += 1;
            }
        }
        assert!(
            varying > 0,
            "the perturbed base must produce at least one genuinely tapered span"
        );
    }

    #[test]
    fn winding_follows_the_direction_the_loop_is_traversed() {
        let table = pair_table(LOOP);
        let counter_clockwise = circular_coords(LOOP.len(), 0, 0.0);
        let clockwise: Vec<Vec2> = counter_clockwise
            .iter()
            .map(|point| Vec2::new(point.x, -point.y))
            .collect();

        let ccw = generate_backbone(&table, &counter_clockwise, true);
        let cw = generate_backbone(&table, &clockwise, true);
        assert!(emitted(&ccw) > 0 && emitted(&cw) > 0);

        // Mirroring the loop must flip every arc's winding, or the rendered
        // backbone takes the long way round the circle.
        for (first, second) in ccw.iter().zip(&cw) {
            match (first, second) {
                (Some(first), Some(second)) => {
                    assert_ne!(
                        first.clockwise, second.clockwise,
                        "mirroring the loop must reverse its winding"
                    );
                }
                (None, None) => {}
                _ => panic!("mirroring must not change which segments are arcs"),
            }
        }
    }

    #[test]
    fn a_bulge_of_size_one_is_not_treated_as_a_loop_start() {
        // `is_loop_start` skips single-base bulges, so the helix walk continues
        // to the hairpin. Every base sits on one circle, so a bulge treated as a
        // loop would fit and draw its own arcs.
        //
        // 1-based: bulges at 4 and 6, hairpin loop 11-16 closed by 10-17.
        let structure = "(((.(.((((......))))))))";
        let table = pair_table(structure);
        let coords = circular_coords(structure.len(), 0, 0.0);
        let arcs = generate_backbone(&table, &coords, true);

        // Slot `k` joins nucleotides `k + 1` and `k + 2`.
        assert!(
            arcs[2..=5].iter().all(Option::is_none),
            "backbone slots next to a bulge must stay straight: {arcs:?}"
        );
        assert!(
            arcs[9..=15].iter().all(Option::is_some),
            "hairpin slots must be drawn as arcs: {arcs:?}"
        );
    }

    #[test]
    fn the_circle_fit_needs_three_sampled_points() {
        // A loop contributes one point per element plus its closing base, so a
        // hairpin needs two unpaired bases before a circle can be fitted.
        for (structure, expect_arcs) in [("()", false), ("(.)", false), ("(..)", true)] {
            let table = pair_table(structure);
            let coords = circular_coords(structure.len(), 0, 0.0);
            let arcs = generate_backbone(&table, &coords, true);
            assert_eq!(
                emitted(&arcs) > 0,
                expect_arcs,
                "{structure}: wrong fit decision at the three-point threshold"
            );
        }
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn zero_length_hairpins_do_not_retraverse_sibling_loops() {
        // Zero-length hairpins defeat `is_loop_start`. With an unbounded scan
        // each one lets `calc_stem_arcs` escape its own stem, so loops are
        // traversed again, doubling the work every four bases. At this length
        // the call took seconds. Re-entering a loop also writes an arc slot
        // twice, which trips the `debug_assert!` in `add_arc`.
        let structure = "(()".repeat(24) + &")".repeat(24);
        let sequence = "A".repeat(structure.len());
        let input = validate(&sequence, &structure).unwrap();

        let n = input.pair_table.len();
        let coords: Vec<Vec2> = (0..n)
            .map(|i| {
                let angle = TAU * (i as f64) / (n as f64);
                Vec2::new(angle.cos() * 100.0, angle.sin() * 100.0)
            })
            .collect();

        // The `debug_assert!` and the runtime are the real guards. This only
        // proves the call returned.
        let arcs = generate_backbone(&input.pair_table, &coords, true);
        assert_eq!(arcs.len(), n - 1);
    }
}
