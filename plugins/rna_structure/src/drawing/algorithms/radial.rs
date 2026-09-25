#![allow(clippy::cast_precision_loss)]

use crate::drawing::config::RadialOptions;
use crate::drawing::geometry::Vec2;
use crate::drawing::output::AlgorithmLayout;
use crate::drawing::validation::PairTable;
use std::f64::consts::{FRAC_PI_2, PI};

/// Computes RNA secondary-structure coordinates with the radial algorithm.
pub(crate) fn layout(pair_table: &PairTable, options: &RadialOptions) -> AlgorithmLayout {
    let n = pair_table.len();
    if !pair_table.has_pairs() {
        return AlgorithmLayout::unpaired_line(pair_table, options.step_radius);
    }

    let mut angle = vec![0.0; n + 5];

    loop_recurse(pair_table, 0, n, &mut angle);

    let mut coordinates = Vec::with_capacity(n);
    coordinates.push(Vec2::zero());

    let mut alpha = options.initial_angle;
    for i in 1..n {
        let previous = coordinates[i - 1];
        coordinates.push(Vec2::new(
            previous.x + options.step_radius * alpha.cos(),
            previous.y + options.step_radius * alpha.sin(),
        ));
        if i + 1 < n {
            alpha += PI - angle[i + 1];
        }
    }

    let backbone =
        crate::drawing::arcs::generate_backbone(pair_table, &coordinates, options.draw_arcs);

    AlgorithmLayout::new(coordinates, backbone, options.step_radius)
}

fn loop_recurse(pair_table: &PairTable, mut i: usize, mut j: usize, angle: &mut [f64]) {
    let mut count = 2; // polygon vertices
    let mut remember = Vec::new();

    let i_old = i.saturating_sub(1);
    j += 1; // Terminates correctly matching C logic

    while i != j {
        let partner = pair_table.raw_partner(i);
        if partner == 0 || i == 0 {
            i += 1;
            count += 1;
        } else {
            count += 2;
            let mut k = i;
            let mut l = partner;
            remember.push(k);
            remember.push(l);
            i = partner + 1;

            let start_k = k;
            let start_l = l;
            let mut ladder = 0;
            loop {
                k += 1;
                l -= 1;
                ladder += 1;
                let partner = pair_table.raw_partner(k);
                if partner != l || partner <= k {
                    break;
                }
            }

            if ladder >= 2 {
                let fill = ladder - 2;
                angle[start_k + 1 + fill] += FRAC_PI_2;
                angle[start_l - 1 - fill] += FRAC_PI_2;
                angle[start_k] += FRAC_PI_2;
                angle[start_l] += FRAC_PI_2;
                for f in 1..=fill {
                    angle[start_k + f] = PI;
                    angle[start_l - f] = PI;
                }
            }

            if k <= l {
                loop_recurse(pair_table, k, l, angle);
            }
        }
    }

    let polygon = PI * f64::from(count - 2) / f64::from(count);
    remember.push(j);

    let mut begin = i_old;
    for chunk in remember.chunks(2) {
        let limit = chunk[0];
        for item in &mut angle[begin..=limit] {
            *item += polygon;
        }
        if let Some(&next_begin) = chunk.get(1) {
            begin = next_begin;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::layout;
    use crate::drawing::config::RadialOptions;
    use crate::drawing::geometry::Vec2;
    use crate::drawing::output::AlgorithmLayout;
    use crate::drawing::testing::{MOTIF_STRUCTURES, balanced_structures_up_to_length, pair_table};
    use crate::drawing::validation::LoopElement;
    use std::f64::consts::PI;

    fn radial(structure: &str) -> AlgorithmLayout {
        layout(&pair_table(structure), &RadialOptions::default())
    }

    fn with(structure: &str, options: &RadialOptions) -> AlgorithmLayout {
        layout(&pair_table(structure), options)
    }

    #[test]
    fn every_backbone_step_is_exactly_one_step_radius() {
        // The layout takes fixed-length steps and changes only direction. An
        // angle error changes the step length.
        let step = RadialOptions::default().step_radius;
        let structures = balanced_structures_up_to_length(7);
        assert_eq!(structures.len(), 214);

        for structure in structures
            .iter()
            .map(String::as_str)
            .chain(MOTIF_STRUCTURES)
        {
            let result = radial(structure);
            assert_eq!(result.coordinates.len(), structure.len());
            for (index, window) in result.coordinates.windows(2).enumerate() {
                let distance = window[0].distance(window[1]);
                assert!(
                    (distance - step).abs() < 1e-9,
                    "{structure}: step {index} was {distance}, expected {step}"
                );
            }
        }
    }

    #[test]
    fn a_hairpin_loop_closes_into_a_regular_polygon() {
        // `loop_recurse` accumulates the interior angle of a `count`-gon. A
        // hairpin with k unpaired bases and one closing pair is a (k + 2)-gon.
        for unpaired in 3..=8_usize {
            let structure = format!("({})", ".".repeat(unpaired));
            let result = radial(&structure);
            let count = unpaired + 2;
            #[allow(clippy::cast_precision_loss)]
            let expected_turn = PI - PI * ((count - 2) as f64) / (count as f64);

            for index in 1..result.coordinates.len() - 1 {
                let incoming = result.coordinates[index] - result.coordinates[index - 1];
                let outgoing = result.coordinates[index + 1] - result.coordinates[index];
                let turn = incoming.angle_between(outgoing);
                assert!(
                    (turn - expected_turn).abs() < 1e-9,
                    "{structure}: turn at {index} was {turn}, expected {expected_turn}"
                );
            }
        }
    }

    #[test]
    fn the_initial_angle_rotates_the_whole_drawing_rigidly() {
        let structure = "(((...)))..((...))";
        let base = radial(structure);
        let rotated = with(
            structure,
            &RadialOptions {
                initial_angle: 0.7,
                ..RadialOptions::default()
            },
        );

        for (index, point) in base.coordinates.iter().enumerate() {
            assert!(
                point.rotate(0.7).distance(rotated.coordinates[index]) < 1e-9,
                "nucleotide {index} did not rotate rigidly"
            );
        }
    }

    #[test]
    fn the_step_radius_scales_the_drawing_and_its_reported_unit() {
        let structure = "(((...)))..((...))";
        let base = radial(structure);
        let scaled = with(
            structure,
            &RadialOptions {
                step_radius: RadialOptions::default().step_radius * 3.0,
                ..RadialOptions::default()
            },
        );

        for (index, point) in base.coordinates.iter().enumerate() {
            assert!(
                (*point * 3.0).distance(scaled.coordinates[index]) < 1e-9,
                "nucleotide {index} did not scale"
            );
        }
        // Normalization divides by this, so it has to track the geometry or
        // the rendered pitch would change with the option.
        assert!((scaled.nominal_spacing / base.nominal_spacing - 3.0).abs() < 1e-12);
    }

    #[test]
    fn a_multiloop_closes_into_a_regular_polygon_that_counts_both_branch_bases() {
        // Each branch adds its opener and closer to the enclosing loop, so a
        // multiloop is a regular polygon over its closing pair, its unpaired
        // bases, and two bases per branch. All of them sit on its circumcircle.
        let step = RadialOptions::default().step_radius;
        for (structure, count) in [
            ("((...)(...))", 6_usize),
            ("(.((...)).(...).)", 9),
            ("(..(...)..(...)..)", 12),
        ] {
            let result = radial(structure);
            let table = pair_table(structure);
            let mut vertices = vec![1, structure.len()];
            for element in table.loop_elements(1) {
                match element {
                    LoopElement::Unpaired(base) => vertices.push(base),
                    LoopElement::Stem { start, end } => vertices.extend([start, end]),
                }
            }
            assert_eq!(vertices.len(), count, "{structure}: wrong loop size");

            let points: Vec<_> = vertices
                .iter()
                .map(|&base| result.coordinates[base - 1])
                .collect();
            #[allow(clippy::cast_precision_loss)]
            let centre = points.iter().fold(Vec2::zero(), |sum, &point| sum + point) / count as f64;
            #[allow(clippy::cast_precision_loss)]
            let circumradius = step / (2.0 * (PI / count as f64).sin());
            for (base, point) in vertices.iter().zip(&points) {
                let radius = point.distance(centre);
                assert!(
                    (radius - circumradius).abs() < 1e-9,
                    "{structure}: base {base} sits {radius} from the loop centre, \
                     expected {circumradius}"
                );
            }
        }
    }

    #[test]
    fn deeply_nested_and_wide_multiloops_stay_in_bounds() {
        // `loop_recurse` writes into an `n + 5` scratch buffer using indices
        // derived from pair partners, so both extremes of shape are exercised.
        let deep = format!("{}...{}", "(".repeat(40), ")".repeat(40));
        let wide = format!("({})", "(...)".repeat(20));

        for structure in [deep, wide] {
            let result = radial(&structure);
            assert_eq!(result.coordinates.len(), structure.len());
            assert!(
                result
                    .coordinates
                    .iter()
                    .all(|point| point.x.is_finite() && point.y.is_finite())
            );
        }
    }
}
