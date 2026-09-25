pub(crate) mod circular;
pub(crate) mod naview;
pub(crate) mod radial;
pub(crate) mod rnapuzzler;
pub(crate) mod rnaturtle;

use crate::drawing::config::LayoutConfig;
use crate::drawing::error::LayoutError;
use crate::drawing::output::AlgorithmLayout;
use crate::drawing::validation::PairTable;

pub(crate) fn layout(
    config: &LayoutConfig,
    pair_table: &PairTable,
) -> Result<AlgorithmLayout, LayoutError> {
    match config {
        LayoutConfig::Radial(options) => Ok(radial::layout(pair_table, options)),
        LayoutConfig::Naview(options) => naview::layout(pair_table, options),
        LayoutConfig::RnaTurtle(options) => rnaturtle::layout(pair_table, options),
        LayoutConfig::RnaPuzzler(options) => rnapuzzler::layout(pair_table, options),
        LayoutConfig::Circular(options) => Ok(circular::layout(pair_table, options)),
    }
}

#[cfg(test)]
mod tests {
    use crate::drawing::config::{
        CircularOptions, LayoutConfig, NaviewOptions, RadialOptions, RnaPuzzlerOptions,
        RnaTurtleOptions,
    };
    use crate::drawing::geometry::Vec2;
    use crate::drawing::testing::{balanced_structures_up_to_length, pair_table};
    use crate::drawing::{config, output};

    const ALGORITHMS: [&str; 5] = ["radial", "naview", "rna_turtle", "rna_puzzler", "circular"];

    fn run(algorithm_config: &str, structure: &str) -> super::AlgorithmLayout {
        let parsed = config::parse(algorithm_config.as_bytes()).expect("config must parse");
        super::layout(&parsed, &pair_table(structure)).expect("layout must succeed")
    }

    #[test]
    fn dot_bracket_structures_up_to_length_8_layout_cleanly() {
        // The failures in this sweep all contained `()`, a zero-length hairpin
        // that defeated the unbounded helix walk shared by `rna_turtle` and
        // `rna_puzzler`. It either panicked or corrupted geometry. Test every
        // balanced structure to cover this class of walk-escape bugs.
        let structures = balanced_structures_up_to_length(8);
        assert_eq!(
            structures.len(),
            537,
            "expected every balanced structure of length 2..=8"
        );

        for structure in &structures {
            for algorithm in ALGORITHMS {
                let config = format!(r#"{{"algorithm":"{algorithm}"}}"#);
                let layout = run(&config, structure);

                assert_eq!(
                    layout.coordinates.len(),
                    structure.len(),
                    "{structure} / {algorithm}: wrong coordinate count"
                );
                assert_eq!(
                    layout.backbone.len(),
                    structure.len() - 1,
                    "{structure} / {algorithm}: wrong backbone count"
                );
                for point in &layout.coordinates {
                    assert!(
                        point.x.is_finite() && point.y.is_finite(),
                        "{structure} / {algorithm}: non-finite coordinate {point:?}"
                    );
                }
                for arc in layout.backbone.iter().flatten() {
                    assert!(
                        arc.center.x.is_finite()
                            && arc.center.y.is_finite()
                            && arc.radius.is_finite()
                            && arc.end_radius.is_none_or(f64::is_finite),
                        "{structure} / {algorithm}: non-finite arc"
                    );
                }
                assert!(
                    layout.nominal_spacing.is_finite() && layout.nominal_spacing > 0.0,
                    "{structure} / {algorithm}: unusable nominal spacing"
                );
            }
        }
    }

    #[test]
    fn a_single_nucleotide_normalizes_under_every_algorithm() {
        // A one-base layout has no backbone step. It must not divide by zero or
        // send NaN geometry to Typst. The circular chord in particular
        // degenerates to zero at length 1, so the reported step has to come
        // from somewhere else.
        for algorithm in ALGORITHMS {
            let config = format!(r#"{{"algorithm":"{algorithm}"}}"#);
            let layout = run(&config, ".");
            assert_eq!(layout.coordinates, [Vec2::zero()], "{algorithm}");
            assert!(layout.backbone.is_empty(), "{algorithm}");
            assert!(
                layout.nominal_spacing.is_finite() && layout.nominal_spacing > 0.0,
                "{algorithm}: a single nucleotide must keep a positive nominal step"
            );

            let response = output::into_response(Vec::new(), layout)
                .expect("a single nucleotide must serialize");
            let json = serde_json::to_value(&response).expect("response must serialize");
            assert!(
                crate::drawing::testing::all_numbers_are_finite(&json),
                "{algorithm}: single-nucleotide response had non-finite numbers"
            );
        }
    }

    #[test]
    fn unpaired_strands_step_by_nominal_spacing_and_draw_no_arcs() {
        // Unpaired coordinates use the algorithm's nominal spacing.
        // The four linear layouts emit a polyline. Circular emits a circle.
        for algorithm in ALGORITHMS {
            let config = format!(r#"{{"algorithm":"{algorithm}"}}"#);
            let layout = run(&config, "........");
            assert_eq!(layout.coordinates.len(), 8, "{algorithm}");
            assert!(
                layout.nominal_spacing.is_finite() && layout.nominal_spacing > 0.0,
                "{algorithm}: unpaired spacing must be positive finite"
            );
            for window in layout.coordinates.windows(2) {
                let step = window[0].distance(window[1]);
                assert!(
                    (step - layout.nominal_spacing).abs() < 1e-9,
                    "{algorithm}: unpaired step {step} != spacing {}",
                    layout.nominal_spacing
                );
            }
            if algorithm == "circular" {
                assert!(
                    layout.backbone.iter().all(Option::is_some),
                    "circular unpaired strand must emit its circle arcs"
                );
            } else {
                assert!(
                    layout.backbone.iter().all(Option::is_none),
                    "{algorithm}: unpaired strand must not emit arcs"
                );
                // The four linear layouts have no loop to turn around, so an
                // unpaired strand must be a straight horizontal run.
                for point in &layout.coordinates {
                    assert!(
                        point.y.abs() < 1e-12,
                        "{algorithm}: an unpaired strand must stay flat, got {point:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn disabling_arcs_suppresses_every_arc_without_moving_a_nucleotide() {
        // `draw_arcs` is a rendering switch. It must remove every arc and leave
        // the coordinates byte-for-byte where they were, or turning arcs off
        // would silently reshape the drawing.
        let structure = "(((.......)))";
        let turtle = RnaTurtleOptions {
            draw_arcs: false,
            ..RnaTurtleOptions::default()
        };
        let cases = [
            (
                "radial",
                LayoutConfig::Radial(RadialOptions {
                    draw_arcs: false,
                    ..RadialOptions::default()
                }),
            ),
            (
                "naview",
                LayoutConfig::Naview(NaviewOptions {
                    draw_arcs: false,
                    ..NaviewOptions::default()
                }),
            ),
            ("rna_turtle", LayoutConfig::RnaTurtle(turtle.clone())),
            (
                "rna_puzzler",
                LayoutConfig::RnaPuzzler(RnaPuzzlerOptions {
                    turtle,
                    ..RnaPuzzlerOptions::default()
                }),
            ),
            (
                "circular",
                LayoutConfig::Circular(CircularOptions {
                    draw_arcs: false,
                    ..CircularOptions::default()
                }),
            ),
        ];

        for (algorithm, without_arcs) in cases {
            let with_arcs = run(&format!(r#"{{"algorithm":"{algorithm}"}}"#), structure);
            let without =
                super::layout(&without_arcs, &pair_table(structure)).expect("layout must succeed");

            assert!(
                with_arcs.backbone.iter().any(Option::is_some),
                "{algorithm}: a wide hairpin loop must produce arcs by default"
            );
            assert!(
                without.backbone.iter().all(Option::is_none),
                "{algorithm}: draw_arcs=false must suppress every arc"
            );
            assert_eq!(without.backbone.len(), structure.len() - 1, "{algorithm}");
            for (index, point) in with_arcs.coordinates.iter().enumerate() {
                assert!(
                    point.distance(without.coordinates[index]) < 1e-12,
                    "{algorithm}: nucleotide {index} moved when arcs were disabled"
                );
            }
        }
    }
}
