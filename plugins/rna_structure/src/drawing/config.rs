use serde::Deserialize;

#[derive(Debug)]
pub(crate) enum LayoutConfig {
    Radial(RadialOptions),
    Naview(NaviewOptions),
    RnaTurtle(RnaTurtleOptions),
    RnaPuzzler(RnaPuzzlerOptions),
    Circular(CircularOptions),
}

/// Typst selects only the algorithm. Each layout runs with its defaults.
// Serde ignores `deny_unknown_fields` on unit variants, so these are empty
// struct variants.
#[derive(Debug, Deserialize)]
#[serde(tag = "algorithm", rename_all = "snake_case", deny_unknown_fields)]
enum LayoutRequest {
    Radial {},
    Naview {},
    RnaTurtle {},
    RnaPuzzler {},
    Circular {},
}

pub(crate) fn parse(config: &[u8]) -> serde_json::Result<LayoutConfig> {
    let request: LayoutRequest = serde_json::from_slice(config)?;

    Ok(match request {
        LayoutRequest::Radial {} => LayoutConfig::Radial(RadialOptions::default()),
        LayoutRequest::Naview {} => LayoutConfig::Naview(NaviewOptions::default()),
        LayoutRequest::RnaTurtle {} => LayoutConfig::RnaTurtle(RnaTurtleOptions::default()),
        LayoutRequest::RnaPuzzler {} => LayoutConfig::RnaPuzzler(RnaPuzzlerOptions::default()),
        LayoutRequest::Circular {} => LayoutConfig::Circular(CircularOptions::default()),
    })
}

#[derive(Debug, Clone)]
pub(crate) struct RadialOptions {
    pub(crate) initial_angle: f64,
    pub(crate) step_radius: f64,
    pub(crate) draw_arcs: bool,
}

impl Default for RadialOptions {
    fn default() -> Self {
        Self {
            initial_angle: 0.0,
            step_radius: 15.0,
            draw_arcs: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NaviewOptions {
    pub(crate) lencut: f64,
    pub(crate) scale: f64,
    pub(crate) draw_arcs: bool,
}

impl Default for NaviewOptions {
    fn default() -> Self {
        Self {
            lencut: 0.5,
            scale: 15.0,
            draw_arcs: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RnaTurtleOptions {
    pub(crate) paired_distance: f64,
    pub(crate) unpaired_distance: f64,
    pub(crate) draw_arcs: bool,
}

impl Default for RnaTurtleOptions {
    fn default() -> Self {
        Self {
            paired_distance: 35.0,
            unpaired_distance: 25.0,
            draw_arcs: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CircularOptions {
    /// Angle of the first nucleotide, matching `coords_circular`'s `- PIHALF`.
    /// It starts at the top of the raw circle. `into_response` flips the y axis
    /// for Typst, putting it at the bottom.
    pub(crate) initial_angle: f64,
    /// Circle radius as `radius_scale * length`, matching `ViennaRNA`'s `r = 3 * n`.
    /// Normalization removes this scale from the rendered drawing.
    pub(crate) radius_scale: f64,
    /// Draws the backbone as arcs of the layout circle. When false, uses the
    /// reference's straight polyline.
    pub(crate) draw_arcs: bool,
    /// Bows base pairs toward the circle's centre. When false, draws straight
    /// chords.
    pub(crate) bow_pairs: bool,
}

impl Default for CircularOptions {
    fn default() -> Self {
        Self {
            initial_angle: -std::f64::consts::FRAC_PI_2,
            radius_scale: 3.0,
            draw_arcs: true,
            bow_pairs: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RnaPuzzlerOptions {
    pub(crate) turtle: RnaTurtleOptions,
    pub(crate) checks: PuzzlerCheckOptions,
    pub(crate) behavior: PuzzlerBehaviorOptions,
    pub(crate) max_config_changes: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PuzzlerCheckOptions {
    pub(crate) ancestors: bool,
    pub(crate) siblings: bool,
    pub(crate) exterior: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PuzzlerBehaviorOptions {
    pub(crate) allow_flipping: bool,
    pub(crate) optimize: bool,
}

impl Default for RnaPuzzlerOptions {
    fn default() -> Self {
        Self {
            turtle: RnaTurtleOptions::default(),
            checks: PuzzlerCheckOptions {
                ancestors: true,
                siblings: true,
                exterior: true,
            },
            behavior: PuzzlerBehaviorOptions {
                allow_flipping: false,
                optimize: true,
            },
            max_config_changes: 25_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LayoutConfig, parse};

    type IsVariant = fn(&LayoutConfig) -> bool;

    #[test]
    fn unknown_algorithms_and_fields_are_rejected() {
        let unknown_algo =
            parse(br#"{"algorithm":"unknown"}"#).expect_err("unknown algorithm must fail");
        assert!(
            unknown_algo.to_string().contains("unknown variant"),
            "unexpected error: {unknown_algo}"
        );
        for config in [
            br#"{"algorithm":"radial","bogus":1}"#.as_slice(),
            // Layout options are not part of the wire format.
            br#"{"algorithm":"circular","options":{"radius_scale":3.0}}"#.as_slice(),
        ] {
            let error = parse(config).expect_err("unknown field must be rejected");
            assert!(
                error.to_string().contains("unknown field"),
                "unexpected error for {config:?}: {error}"
            );
        }
    }

    #[test]
    fn every_algorithm_tag_selects_its_own_layout() {
        let cases: [(&[u8], IsVariant); 5] = [
            (br#"{"algorithm":"radial"}"#, |c| {
                matches!(c, LayoutConfig::Radial(_))
            }),
            (br#"{"algorithm":"naview"}"#, |c| {
                matches!(c, LayoutConfig::Naview(_))
            }),
            (br#"{"algorithm":"rna_turtle"}"#, |c| {
                matches!(c, LayoutConfig::RnaTurtle(_))
            }),
            (br#"{"algorithm":"rna_puzzler"}"#, |c| {
                matches!(c, LayoutConfig::RnaPuzzler(_))
            }),
            (br#"{"algorithm":"circular"}"#, |c| {
                matches!(c, LayoutConfig::Circular(_))
            }),
        ];
        for (config, is_expected) in cases {
            let parsed = parse(config).expect("an algorithm tag must parse");
            assert!(is_expected(&parsed), "{config:?} selected {parsed:?}");
        }
    }
}
