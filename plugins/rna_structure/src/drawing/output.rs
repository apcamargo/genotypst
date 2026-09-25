use crate::drawing::error::LayoutError;
use crate::drawing::geometry::Vec2;
use crate::drawing::validation::{BasePair, PairTable};
use serde::Serialize;

#[derive(Debug)]
pub(crate) struct AlgorithmLayout {
    pub(crate) coordinates: Vec<Vec2>,
    pub(crate) backbone: Vec<Option<BackboneArc>>,
    /// Length of one nominal backbone step in this algorithm's coordinates.
    /// `into_response` divides by it so every algorithm uses the same unit.
    pub(crate) nominal_spacing: f64,
    /// Optional bow for each base pair, in the same order as the input pairs.
    /// Empty when the layout draws every pair as a straight chord.
    pub(crate) pair_curves: Vec<Option<PairCurve>>,
}

impl AlgorithmLayout {
    pub(crate) const fn new(
        coordinates: Vec<Vec2>,
        backbone: Vec<Option<BackboneArc>>,
        nominal_spacing: f64,
    ) -> Self {
        Self {
            coordinates,
            backbone,
            nominal_spacing,
            pair_curves: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn with_pair_curves(mut self, pair_curves: Vec<Option<PairCurve>>) -> Self {
        self.pair_curves = pair_curves;
        self
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn unpaired_line(pair_table: &PairTable, spacing: f64) -> Self {
        let coordinates: Vec<Vec2> = (0..pair_table.len())
            .map(|index| Vec2::new((index as f64) * spacing, 0.0))
            .collect();
        let backbone = vec![None; pair_table.len().saturating_sub(1)];
        Self::new(coordinates, backbone, spacing)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct LayoutResponse {
    coordinates: Vec<Vec2>,
    base_pairs: Vec<BasePair>,
    backbone: Vec<Option<BackboneArc>>,
    /// Geometry units in `coordinates` per nominal backbone step. Normalized
    /// layouts report `1.0`. Other layouts report their own unit.
    nominal_spacing: f64,
    /// Cubic control points for bowed pairs, in the same order as `base_pairs`.
    /// Omitted when the layout draws only straight chords.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pair_curves: Vec<Option<PairCurve>>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct BackboneArc {
    pub(crate) center: Vec2,
    /// Radius at the segment's first nucleotide.
    pub(crate) radius: f64,
    /// When present, NAVIEW linearly varies its radius while advancing around
    /// the arc. The fitter renders that polar curve as endpoint-exact cubics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end_radius: Option<f64>,
    pub(crate) clockwise: bool,
}

/// Control points for a cubic Bézier between a pair's two nucleotides.
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct PairCurve {
    pub(crate) control_i: Vec2,
    pub(crate) control_j: Vec2,
}

fn normalize_point_for_typst(point: &mut Vec2, divisor: f64) {
    point.x /= divisor;
    point.y = -point.y / divisor;
}

pub(crate) fn into_response(
    base_pairs: Vec<BasePair>,
    mut layout: AlgorithmLayout,
) -> Result<LayoutResponse, LayoutError> {
    // Every default spacing is positive. Keep this fallback for an internal
    // error, and report the original unit instead of claiming 1.0.
    let normalize = layout.nominal_spacing.is_finite() && layout.nominal_spacing > 0.0;
    let divisor = if normalize {
        layout.nominal_spacing
    } else {
        1.0
    };

    // Curves are positional, so the curve count must match the pair count.
    if !layout.pair_curves.is_empty() && layout.pair_curves.len() != base_pairs.len() {
        return Err(LayoutError::InternalInvariant {
            message: format!(
                "layout emitted {} pair curves for {} base pairs",
                layout.pair_curves.len(),
                base_pairs.len()
            ),
        });
    }

    for point in &mut layout.coordinates {
        normalize_point_for_typst(point, divisor);
    }
    for arc in layout.backbone.iter_mut().flatten() {
        normalize_point_for_typst(&mut arc.center, divisor);
        arc.radius /= divisor;
        if let Some(end_radius) = &mut arc.end_radius {
            *end_radius /= divisor;
        }
        arc.clockwise = !arc.clockwise;
    }
    for curve in layout.pair_curves.iter_mut().flatten() {
        for control in [&mut curve.control_i, &mut curve.control_j] {
            normalize_point_for_typst(control, divisor);
        }
    }

    Ok(LayoutResponse {
        coordinates: layout.coordinates,
        base_pairs,
        backbone: layout.backbone,
        nominal_spacing: if normalize {
            1.0
        } else {
            layout.nominal_spacing
        },
        pair_curves: layout.pair_curves,
    })
}

#[cfg(test)]
mod tests {
    use super::{AlgorithmLayout, BackboneArc, PairCurve, into_response};
    use crate::drawing::algorithms;
    use crate::drawing::config;
    use crate::drawing::error::LayoutError;
    use crate::drawing::geometry::Vec2;
    use crate::drawing::testing::pair_table;
    use crate::drawing::validation::{BasePair, validate};

    fn arc(center: Vec2, radius: f64, end_radius: Option<f64>, clockwise: bool) -> BackboneArc {
        BackboneArc {
            center,
            radius,
            end_radius,
            clockwise,
        }
    }

    #[test]
    fn normalization_divides_every_channel_and_flips_only_y() {
        // Typst's y axis points down. Mirror every point-bearing channel together
        // so arcs and bows stay attached to the nucleotides.
        let layout = AlgorithmLayout::new(
            vec![Vec2::new(10.0, 20.0), Vec2::new(30.0, -40.0)],
            vec![Some(arc(Vec2::new(50.0, 60.0), 25.0, Some(35.0), false))],
            5.0,
        )
        .with_pair_curves(vec![Some(PairCurve {
            control_i: Vec2::new(15.0, 25.0),
            control_j: Vec2::new(-5.0, 45.0),
        })]);

        let response = into_response(vec![BasePair { i: 0, j: 1 }], layout).unwrap();

        assert_eq!(response.coordinates[0], Vec2::new(2.0, -4.0));
        assert_eq!(response.coordinates[1], Vec2::new(6.0, 8.0));

        let arc = response.backbone[0].expect("the arc must survive normalization");
        assert_eq!(arc.center, Vec2::new(10.0, -12.0));
        assert!((arc.radius - 5.0).abs() < 1e-12);
        assert!((arc.end_radius.expect("end radius must survive") - 7.0).abs() < 1e-12);
        // Mirroring the plane reverses the sense of every rotation.
        assert!(arc.clockwise, "the y flip must invert the winding");

        let curve = response.pair_curves[0].expect("the curve must survive normalization");
        assert_eq!(curve.control_i, Vec2::new(3.0, -5.0));
        assert_eq!(curve.control_j, Vec2::new(-1.0, -9.0));

        assert!((response.nominal_spacing - 1.0).abs() < 1e-12);
    }

    #[test]
    fn an_unusable_spacing_reports_itself_instead_of_faking_normalization() {
        // Dividing by these would produce NaN geometry. No default produces
        // them, so reaching here means an internal error. The response
        // must still report the units it uses.
        for spacing in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            let layout = AlgorithmLayout::new(vec![Vec2::new(10.0, 20.0)], Vec::new(), spacing);
            let response = into_response(Vec::new(), layout).unwrap();

            assert_eq!(
                response.coordinates[0],
                Vec2::new(10.0, -20.0),
                "spacing {spacing} must leave the magnitude untouched"
            );
            assert_eq!(
                response.nominal_spacing.is_nan(),
                spacing.is_nan(),
                "spacing {spacing} must be reported back verbatim"
            );
            if !spacing.is_nan() {
                assert_eq!(response.nominal_spacing, spacing);
            }
        }
    }

    #[test]
    fn a_curve_count_that_does_not_match_the_pairs_is_an_internal_invariant() {
        // A mismatched count would bow the wrong pair instead of failing.
        let layout = AlgorithmLayout::new(vec![Vec2::zero(), Vec2::new(1.0, 0.0)], vec![None], 1.0)
            .with_pair_curves(vec![None, None]);

        let error = into_response(vec![BasePair { i: 0, j: 1 }], layout).unwrap_err();
        let LayoutError::InternalInvariant { message } = error else {
            panic!("a curve count mismatch must be an internal invariant");
        };
        assert!(
            message.contains("2 pair curves for 1 base pairs"),
            "{message}"
        );
    }

    #[test]
    fn an_unpaired_line_steps_by_its_spacing_and_draws_no_arcs() {
        let table = pair_table(".....");
        let layout = AlgorithmLayout::unpaired_line(&table, 7.0);

        assert_eq!(layout.coordinates.len(), 5);
        assert_eq!(layout.backbone.len(), 4);
        assert!(layout.backbone.iter().all(Option::is_none));
        assert!((layout.nominal_spacing - 7.0).abs() < 1e-12);
        for window in layout.coordinates.windows(2) {
            assert!((window[0].distance(window[1]) - 7.0).abs() < 1e-12);
            assert!((window[0].y).abs() < 1e-12);
        }

        // A single nucleotide has no step to take.
        let single = AlgorithmLayout::unpaired_line(&pair_table("."), 7.0);
        assert_eq!(single.coordinates.len(), 1);
        assert!(single.backbone.is_empty());
    }

    #[test]
    fn normalization_puts_one_nominal_backbone_step_in_one_geometry_unit() {
        let sequence = "GGGAAAUCC";
        let structure = "(((...)))";
        let input = validate(sequence, structure).unwrap();

        // `rna_turtle` steps by exactly `unpaired_distance` between unpaired
        // neighbours, so a normalized layout must step by exactly 1.
        let parsed = config::parse(br#"{"algorithm":"rna_turtle"}"#).unwrap();
        let layout = algorithms::layout(&parsed, &input.pair_table).unwrap();
        let response = into_response(input.base_pairs, layout).unwrap();

        assert!((response.nominal_spacing - 1.0).abs() < 1e-12);
        assert!(
            response
                .coordinates
                .iter()
                .all(|point| point.x.is_finite() && point.y.is_finite())
        );
        let step = response.coordinates[4].distance(response.coordinates[5]);
        assert!((step - 1.0).abs() < 1e-9, "hairpin step was {step}");
    }
}
