#![allow(clippy::cast_precision_loss)]

use crate::drawing::config::CircularOptions;
use crate::drawing::geometry::Vec2;
use crate::drawing::output::{AlgorithmLayout, BackboneArc, PairCurve};
use crate::drawing::validation::PairTable;
use std::f64::consts::{PI, TAU};

/// At or above this bow-ratio threshold, the bow is indistinguishable from its
/// chord. Report the pair as straight so the renderer draws a line.
const STRAIGHT_BOW: f64 = 1.0 - 1e-12;

/// Places nucleotides at equal distances on a circle, following `ViennaRNA`'s
/// `coords_circular` and the scaling applied by `rna_layout`.
///
/// The reference emits a straight polyline with no arc channel. Here each
/// backbone segment follows the same circle, so its arcs need no fitting.
pub(crate) fn layout(pair_table: &PairTable, options: &CircularOptions) -> AlgorithmLayout {
    let n = pair_table.len();
    let radius = options.radius_scale * n as f64;

    // A single nucleotide has no chord. Use the arc-length limit as its step.
    if n < 2 {
        return AlgorithmLayout::unpaired_line(pair_table, TAU * radius);
    }

    let step = TAU / n as f64;
    let coordinates: Vec<Vec2> = (0..n)
        .map(|index| {
            let (sin, cos) = (options.initial_angle + index as f64 * step).sin_cos();
            Vec2::new(radius * cos, radius * sin)
        })
        .collect();

    // Indices advance counter-clockwise. `into_response` flips the y axis and
    // the winding flag.
    let arc = options.draw_arcs.then_some(BackboneArc {
        center: Vec2::zero(),
        radius,
        end_radius: None,
        clockwise: false,
    });
    let backbone = vec![arc; n - 1];

    let pair_curves = if options.bow_pairs {
        bows(pair_table, &coordinates)
    } else {
        Vec::new()
    };

    AlgorithmLayout::new(coordinates, backbone, chord(radius, n)).with_pair_curves(pair_curves)
}

/// Distance between consecutive nucleotides, the nominal backbone step.
fn chord(radius: f64, n: usize) -> f64 {
    2.0 * radius * (PI / n as f64).sin()
}

fn bows(pair_table: &PairTable, coordinates: &[Vec2]) -> Vec<Option<PairCurve>> {
    let n = pair_table.len();
    pair_table
        .pairs()
        .map(|(opener, closer)| {
            let ratio = bow_ratio(opener, closer, n);
            (ratio < STRAIGHT_BOW).then(|| PairCurve {
                control_i: coordinates[opener - 1] * ratio,
                control_j: coordinates[closer - 1] * ratio,
            })
        })
        .collect()
}

/// Returns `ViennaRNA` `svg.c`'s control-point scale, `R = 1 - 2 * dr / n`,
/// about the circle's centre. Both endpoints use the same scale.
///
/// This follows `svg.c`, not the PostScript `arccoords` macro. Their short-range
/// branches use different `dr` values and thresholds.
fn bow_ratio(opener: usize, closer: usize, n: usize) -> f64 {
    let span = closer - opener;
    let dr = if span < n / 2 { span + 1 } else { n - span - 1 };
    1.0 - 2.0 * dr as f64 / n as f64
}

#[cfg(test)]
mod tests {
    use super::{bow_ratio, layout};
    use crate::drawing::config::CircularOptions;
    use crate::drawing::geometry::Vec2;
    use crate::drawing::output::AlgorithmLayout;
    use crate::drawing::testing::pair_table;
    use std::f64::consts::{FRAC_PI_2, TAU};

    fn circular(structure: &str) -> AlgorithmLayout {
        layout(&pair_table(structure), &CircularOptions::default())
    }

    fn radius_of(structure: &str) -> f64 {
        3.0 * structure.len() as f64
    }

    #[test]
    fn coordinates_match_the_closed_form() {
        // This pins the start angle, the radius formula, and the winding
        // direction all at once: the expected point for each index is exact, so
        // a reversed traversal or a shifted start angle moves it. Do not add
        // separate cardinal-point or winding tests on top of this one.
        for length in [2, 3, 4, 5, 9, 17, 64] {
            let structure = ".".repeat(length);
            let result = circular(&structure);
            let radius = radius_of(&structure);
            assert_eq!(result.coordinates.len(), length);
            for (index, point) in result.coordinates.iter().enumerate() {
                let angle = -FRAC_PI_2 + TAU * index as f64 / length as f64;
                let expected = Vec2::new(radius * angle.cos(), radius * angle.sin());
                assert!(
                    point.distance(expected) < 1e-12,
                    "length {length} index {index}: {point:?} != {expected:?}"
                );
            }
        }
    }

    #[test]
    fn coordinates_ignore_the_pair_table() {
        // Circular layout depends only on sequence length.
        let unpaired = circular(".........");
        let paired = circular("(((...)))");
        for (left, right) in unpaired.coordinates.iter().zip(&paired.coordinates) {
            assert!(left.distance(*right) < 1e-12);
        }
    }

    #[test]
    fn initial_angle_rotates_and_radius_scale_scales() {
        let table = pair_table("(((...)))");
        let base = layout(&table, &CircularOptions::default());
        let rotated = layout(
            &table,
            &CircularOptions {
                initial_angle: -FRAC_PI_2 + FRAC_PI_2,
                ..CircularOptions::default()
            },
        );
        let scaled = layout(
            &table,
            &CircularOptions {
                radius_scale: 6.0,
                ..CircularOptions::default()
            },
        );

        for (index, point) in base.coordinates.iter().enumerate() {
            assert!(point.rotate(FRAC_PI_2).distance(rotated.coordinates[index]) < 1e-9);
            assert!((*point * 2.0).distance(scaled.coordinates[index]) < 1e-9);
        }
        // Doubling the radius doubles the step it is normalized by, so the
        // drawing Typst receives is unchanged.
        assert!((scaled.nominal_spacing / base.nominal_spacing - 2.0).abs() < 1e-12);
    }

    #[test]
    fn backbone_is_one_exact_arc_per_step_on_the_layout_circle() {
        // Indices advance counter-clockwise, so every raw arc is
        // counter-clockwise. `into_response` flips the flag with the y axis.
        for structure in ["(((...)))..", &".".repeat(16)] {
            let result = circular(structure);
            let radius = radius_of(structure);

            assert_eq!(result.backbone.len(), structure.len() - 1);
            for (index, arc) in result.backbone.iter().enumerate() {
                let arc = arc.expect("every circular backbone segment is an arc");
                assert!(arc.center.distance(Vec2::zero()) < 1e-12);
                assert!((arc.radius - radius).abs() < 1e-12);
                assert!(!arc.clockwise);
                for endpoint in [result.coordinates[index], result.coordinates[index + 1]] {
                    assert!((endpoint.length() - radius).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn a_nine_mer_hairpin_has_the_pinned_reference_bows() {
        // Use literal values so branch-selection changes appear in the result.
        let result = circular("(((...)))");
        assert_eq!(result.pair_curves.len(), 3);
        assert!(
            result.pair_curves[0].is_none(),
            "1-9 wraps the circle and stays straight"
        );

        // Both controls sit on their own nucleotide's radius, scaled toward the
        // centre by the same ratio, so the bow is symmetric.
        for (index, (opener, closer), ratio) in [(1, (2, 8), 5.0 / 9.0), (2, (3, 7), 1.0 / 9.0)] {
            let curve = result.pair_curves[index].expect("inner pairs bow");
            for (control, anchor) in [
                (curve.control_i, result.coordinates[opener - 1]),
                (curve.control_j, result.coordinates[closer - 1]),
            ] {
                assert!(
                    (control.length() / (3.0 * 9.0) - ratio).abs() < 1e-9,
                    "pair {opener}-{closer}: wrong bow depth"
                );
                assert!(
                    control.cross(anchor).abs() < 1e-9,
                    "pair {opener}-{closer}: control left its nucleotide's radius"
                );
            }
        }

        // Nucleotides 1 and n are neighbours on the circle, so the outermost
        // pair is a chord already and needs no curve at any length.
        for structure in ["(....)", "()"] {
            assert!(
                circular(structure).pair_curves[0].is_none(),
                "{structure}: outermost pair should be straight"
            );
        }
    }

    #[test]
    fn the_widest_pair_of_a_four_mer_reaches_the_centre() {
        // `svg.c`'s short branch is aggressive at small lengths: the inner pair
        // of `(())` gets `R = 0`, so both controls sit on the circle's centre.
        let result = circular("(())");
        let inner = result.pair_curves[1].expect("inner pair bows");
        assert!(inner.control_i.length() < 1e-12);
        assert!(inner.control_j.length() < 1e-12);
    }

    #[test]
    fn bow_ratio_never_leaves_the_unit_interval() {
        // Guards the branch threshold: a ratio above 1 would push controls
        // outside the circle, and one below 0 would flip the bow outward.
        for n in 2..=64_usize {
            for opener in 1..=n {
                for closer in (opener + 1)..=n {
                    let ratio = bow_ratio(opener, closer, n);
                    assert!(
                        (0.0..=1.0).contains(&ratio),
                        "n {n} pair {opener}-{closer} gave {ratio}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_bow_depth_branch_flips_exactly_at_half_the_circle() {
        // `svg.c` picks `dr = span + 1` for short-range pairs and
        // `dr = n - span - 1` otherwise. The unit-interval sweep catches a
        // switch that moves past half the circle. This pins the switch itself.
        let n = 20;
        let half = n / 2;

        // The two branches form a V. Bows deepen as the span approaches half
        // the circle, then become shallower. The minimum is at the last span
        // in the short branch, so an off-by-one moves the vertex.
        for span in 1..half {
            assert!(
                bow_ratio(1, 1 + span, n) < bow_ratio(1, span, n),
                "the short branch must deepen through span {span}"
            );
        }
        for span in half..n - 1 {
            assert!(
                bow_ratio(1, 2 + span, n) > bow_ratio(1, 1 + span, n),
                "the long branch must shallow out past span {span}"
            );
        }
        let vertex = bow_ratio(1, half, n);
        assert!(
            (1..n).all(|span| bow_ratio(1, 1 + span, n) >= vertex),
            "the deepest bow must sit at span {}",
            half - 1
        );
    }

    #[test]
    fn disabling_bows_emits_no_curves() {
        let result = layout(
            &pair_table("(((...)))"),
            &CircularOptions {
                bow_pairs: false,
                ..CircularOptions::default()
            },
        );
        assert!(result.pair_curves.is_empty());
    }
}
