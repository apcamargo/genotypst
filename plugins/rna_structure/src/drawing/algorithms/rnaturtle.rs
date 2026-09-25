#![allow(clippy::cast_precision_loss)]

use crate::drawing::config::RnaTurtleOptions;
use crate::drawing::error::LayoutError;
use crate::drawing::geometry::Vec2;
use crate::drawing::output::AlgorithmLayout;
use crate::drawing::validation::LoopElement;
use crate::drawing::validation::PairTable;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

#[derive(Debug, Clone)]
pub(super) struct BaseInfo {
    pub(super) angle: f64,
    pub(super) distance: f64,
    pub(super) config: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct LoopConfig {
    pub(super) radius: f64,
    pub(super) min_radius: f64,
    pub(super) default_radius: f64,
    pub(super) backbone_distance: f64,
    pub(super) arcs: Vec<ConfigArc>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ConfigArc {
    pub(super) segment_count: usize,
    pub(super) arc_angle: f64,
}

#[derive(Debug, Clone)]
pub(super) struct TurtleState {
    pub(super) base_info: Vec<BaseInfo>,
    pub(super) configs: Vec<LoopConfig>,
}

pub(super) fn build_turtle_state(
    pair_table: &PairTable,
    paired: f64,
    unpaired: f64,
) -> TurtleState {
    let length = pair_table.len();
    let mut base_info = vec![
        BaseInfo {
            angle: 0.0,
            distance: unpaired,
            config: None,
        };
        length + 1
    ];

    let mut configs = Vec::with_capacity(length / 2);

    for (start, _) in pair_table.top_level_stems() {
        cfg_gen_handle_stem(
            pair_table,
            start,
            &mut base_info,
            &mut configs,
            unpaired,
            paired,
        );
    }

    TurtleState { base_info, configs }
}

fn cfg_gen_handle_stem(
    pair_table: &PairTable,
    base_nr: usize,
    base_info: &mut [BaseInfo],
    configs: &mut Vec<LoopConfig>,
    unpaired: f64,
    paired: f64,
) {
    cfg_gen_handle_loop(
        pair_table,
        pair_table.helix_loop_start(base_nr),
        base_info,
        configs,
        unpaired,
        paired,
    );
}

fn cfg_gen_handle_loop(
    pair_table: &PairTable,
    start: usize,
    base_info: &mut [BaseInfo],
    configs: &mut Vec<LoopConfig>,
    unpaired: f64,
    paired: f64,
) {
    let end = pair_table.raw_partner(start);
    if end == 0 || start >= end {
        return;
    }

    let counts = pair_table.loop_counts(start);
    let stem_count = counts.stems + 1;
    let unpaired_count = counts.unpaired;

    let is_bulge = stem_count == 2 && unpaired_count == 1;
    if is_bulge {
        if pair_table.raw_partner(start + 1) == 0 {
            cfg_gen_handle_stem(pair_table, start + 2, base_info, configs, unpaired, paired);
        } else {
            cfg_gen_handle_stem(pair_table, start + 1, base_info, configs, unpaired, paired);
        }
    } else {
        let cfg_loop = if end == start + 1 {
            cfg_generate_zero_length_hairpin_config(paired)
        } else {
            let n = unpaired_count + stem_count;
            let default_radius =
                approximate_config_arc_radius(paired, unpaired, stem_count, n, TAU);
            cfg_generate_default_config(pair_table, start, unpaired, paired, default_radius)
        };

        let cfg_idx = configs.len();
        configs.push(cfg_loop);
        base_info[start].config = Some(cfg_idx);

        for element in pair_table.loop_elements(start) {
            if let LoopElement::Stem {
                start: stem_start, ..
            } = element
            {
                cfg_gen_handle_stem(pair_table, stem_start, base_info, configs, unpaired, paired);
            }
        }
    }
}

fn cfg_generate_default_config(
    pair_table: &PairTable,
    start: usize,
    unpaired: f64,
    paired: f64,
    radius: f64,
) -> LoopConfig {
    let angle_paired = 2.0 * (paired / (2.0 * radius)).clamp(-1.0, 1.0).asin();
    let angle_unpaired = 2.0 * (unpaired / (2.0 * radius)).clamp(-1.0, 1.0).asin();

    let mut unpaired_count = 0;
    let mut arcs = Vec::new();

    for element in pair_table.loop_elements(start) {
        match element {
            LoopElement::Unpaired(_) => unpaired_count += 1,
            LoopElement::Stem { .. } => {
                push_config_arc(&mut arcs, unpaired_count, angle_paired, angle_unpaired);
                unpaired_count = 0;
            }
        }
    }
    push_config_arc(&mut arcs, unpaired_count, angle_paired, angle_unpaired);

    LoopConfig {
        radius,
        min_radius: radius,
        default_radius: radius,
        backbone_distance: unpaired,
        arcs,
    }
}

fn cfg_generate_zero_length_hairpin_config(paired: f64) -> LoopConfig {
    // The pair and sole backbone segment are the same chord.
    let radius = paired * 0.5;
    LoopConfig {
        radius,
        min_radius: radius,
        default_radius: radius,
        backbone_distance: paired,
        arcs: vec![ConfigArc {
            segment_count: 1,
            arc_angle: TAU,
        }],
    }
}

fn push_config_arc(
    arcs: &mut Vec<ConfigArc>,
    unpaired_count: usize,
    angle_paired: f64,
    angle_unpaired: f64,
) {
    let segment_count = unpaired_count + 1;
    arcs.push(ConfigArc {
        segment_count,
        arc_angle: angle_paired + (segment_count as f64) * angle_unpaired,
    });
}

#[must_use]
pub(super) fn approximate_config_arc_radius(a: f64, b: f64, m: usize, n: usize, angle: f64) -> f64 {
    let max_iterations = 1000;
    let sum_mn = (m + n) as f64;
    let half_ang = angle / sum_mn * 0.5;

    let sin_half_ang = half_ang.sin();
    let lower_bound = if sin_half_ang.abs() > 1e-9 {
        (b * 0.5) / sin_half_ang
    } else {
        b * 0.5
    };
    let upper_bound = if sin_half_ang.abs() > 1e-9 {
        (a * 0.5) / sin_half_ang
    } else {
        a * 0.5
    };

    let mut rtn = 0.5 * (lower_bound + upper_bound);
    rtn = rtn.max(0.5 * a).max(0.5 * b);

    for _ in 0..max_iterations {
        let term_a = rtn * rtn - a * a * 0.25;
        let term_b = rtn * rtn - b * b * 0.25;

        let val_a = if term_a > 1e-9 { term_a.sqrt() } else { 1e-9 };
        let val_b = if term_b > 1e-9 { term_b.sqrt() } else { 1e-9 };

        let num = 2.0
            * ((m as f64) * (a / (2.0 * rtn)).clamp(-1.0, 1.0).asin()
                + (n as f64) * (b / (2.0 * rtn)).clamp(-1.0, 1.0).asin()
                - angle * 0.5);
        let den = -((a * (m as f64)) / (rtn * val_a) + (b * (n as f64)) / (rtn * val_b));

        if den.abs() < 1e-9 {
            break;
        }
        let dx = num / den;
        rtn -= dx;
        if dx.abs() < 1e-3 {
            break;
        }
    }

    let (min_b, max_b) = if lower_bound < upper_bound {
        (lower_bound, upper_bound)
    } else {
        (upper_bound, lower_bound)
    };
    rtn.clamp(min_b, max_b)
}

#[must_use]
pub(super) fn approximate_config_radius(cfg: &LoopConfig, paired: f64) -> f64 {
    let mut r = 0.0;
    for arc in &cfg.arcs {
        let stems = 1;
        let num_segments = arc.segment_count;
        let temp_r = approximate_config_arc_radius(
            paired,
            cfg.backbone_distance,
            stems,
            num_segments,
            arc.arc_angle,
        );
        if temp_r > r {
            r = temp_r;
        }
    }
    r
}

pub(super) fn handle_exterior_bases(
    pair_table: &PairTable,
    mut current_base: usize,
    base_info: &mut [BaseInfo],
    direction: f64,
) -> usize {
    let length = pair_table.len();
    if current_base > 1 {
        base_info[current_base].angle += direction * FRAC_PI_2;
    }

    while current_base < length && pair_table.raw_partner(current_base) == 0 {
        base_info[current_base + 1].angle = 0.0;
        current_base += 1;
    }

    if current_base < length {
        base_info[current_base + 1].angle = direction * FRAC_PI_2;
    }

    current_base
}

fn handle_loop(
    pair_table: &PairTable,
    start: usize,
    paired: f64,
    unpaired: f64,
    base_info: &mut [BaseInfo],
    configs: &[LoopConfig],
    direction: f64,
) -> Result<(), LayoutError> {
    let end = pair_table.raw_partner(start);
    if end == 0 || start >= end {
        return Ok(());
    }

    let loop_pairs = count_loop_pairs_reference(pair_table, start);
    let consecutive_pair_diff = loop_pairs.consecutive_pairs - loop_pairs.base_pairs;
    let is_bulge = detect_bulge(pair_table, start).is_some() && consecutive_pair_diff == 1;
    let context = TurtleLoopContext {
        paired,
        unpaired,
        direction,
    };
    let alpha = if is_bulge {
        // Match the C reference's integer truncation for this bulge arc length.
        #[allow(clippy::cast_possible_truncation)]
        let length = ((unpaired * ((consecutive_pair_diff + 1) as f64)) / 2.0).trunc() as i32;
        (length > 0).then(|| {
            (unpaired / (2.0 * f64::from(length)))
                .clamp(-1.0, 1.0)
                .acos()
        })
    } else {
        None
    };

    if let Some(alpha) = alpha {
        handle_bulge_loop(pair_table, start, base_info, configs, context, alpha)?;
    } else {
        handle_regular_loop(pair_table, start, end, base_info, configs, context)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct LoopPairCounts {
    base_pairs: usize,
    consecutive_pairs: usize,
}

fn count_loop_pairs_reference(pair_table: &PairTable, start: usize) -> LoopPairCounts {
    let end = pair_table.raw_partner(start);
    let mut base_pairs = 1;
    let mut consecutive_pairs = 1;
    let mut current_base = start + 1;

    while current_base < end {
        let partner = pair_table.raw_partner(current_base);
        if partner == 0 || partner < current_base {
            consecutive_pairs += 1;
            current_base += 1;
        } else {
            base_pairs += 1;
            current_base = partner;
        }
    }

    LoopPairCounts {
        base_pairs,
        consecutive_pairs,
    }
}

#[derive(Debug, Clone, Copy)]
enum BulgeScanState {
    ConfirmingPartner { expected_partner: usize },
    AfterUnpaired,
}

fn detect_bulge(pair_table: &PairTable, start: usize) -> Option<usize> {
    let end = pair_table.raw_partner(start);
    let mut state = BulgeScanState::ConfirmingPartner {
        expected_partner: 0,
    };
    let mut current_base = start + 1;

    while current_base > start {
        let partner = pair_table.raw_partner(current_base);
        if partner > 0 {
            match state {
                BulgeScanState::ConfirmingPartner { expected_partner } => {
                    if partner == expected_partner {
                        current_base += 1;
                    } else {
                        return (partner == start || partner == end.saturating_sub(2))
                            .then_some(partner);
                    }
                }
                BulgeScanState::AfterUnpaired => {
                    state = BulgeScanState::ConfirmingPartner {
                        expected_partner: current_base,
                    };
                    current_base = partner;
                }
            }
        } else {
            match state {
                BulgeScanState::ConfirmingPartner { .. } => {
                    state = BulgeScanState::AfterUnpaired;
                    current_base += 1;
                }
                BulgeScanState::AfterUnpaired => current_base += 1,
            }
        }
    }

    None
}

#[derive(Debug, Clone, Copy)]
struct TurtleLoopContext {
    paired: f64,
    unpaired: f64,
    direction: f64,
}

fn handle_bulge_loop(
    pair_table: &PairTable,
    start: usize,
    base_info: &mut [BaseInfo],
    configs: &[LoopConfig],
    context: TurtleLoopContext,
    alpha: f64,
) -> Result<(), LayoutError> {
    if pair_table.raw_partner(start + 1) == 0 {
        handle_left_bulge(pair_table, start, base_info, configs, context, alpha)
    } else {
        handle_right_bulge(pair_table, start, base_info, configs, context, alpha)
    }
}

fn handle_left_bulge(
    pair_table: &PairTable,
    start: usize,
    base_info: &mut [BaseInfo],
    configs: &[LoopConfig],
    context: TurtleLoopContext,
    alpha: f64,
) -> Result<(), LayoutError> {
    base_info[start + 1].angle += context.direction * alpha;

    let next_base = start + 1;
    base_info[next_base + 1].angle = -context.direction * alpha * 2.0;

    let next_next = next_base + 1;
    if next_next + 1 < base_info.len() {
        base_info[next_next + 1].angle = context.direction * alpha;
    }

    handle_stem(
        pair_table,
        next_next,
        context.paired,
        context.unpaired,
        base_info,
        configs,
        context.direction,
    )
}

fn handle_right_bulge(
    pair_table: &PairTable,
    start: usize,
    base_info: &mut [BaseInfo],
    configs: &[LoopConfig],
    context: TurtleLoopContext,
    alpha: f64,
) -> Result<(), LayoutError> {
    let mut i_idx = start + 1;

    handle_stem(
        pair_table,
        i_idx,
        context.paired,
        context.unpaired,
        base_info,
        configs,
        context.direction,
    )?;

    i_idx = pair_table.raw_partner(i_idx);
    base_info[i_idx + 1].angle += context.direction * alpha;
    i_idx += 1;

    base_info[i_idx + 1].angle = -context.direction * alpha * 2.0;
    i_idx += 1;

    if i_idx + 1 < base_info.len() {
        base_info[i_idx + 1].angle = context.direction * alpha;
    }
    Ok(())
}

fn handle_regular_loop(
    pair_table: &PairTable,
    start: usize,
    end: usize,
    base_info: &mut [BaseInfo],
    configs: &[LoopConfig],
    context: TurtleLoopContext,
) -> Result<(), LayoutError> {
    let cfg_idx = base_info[start]
        .config
        .ok_or_else(|| LayoutError::InternalInvariant {
            message: format!("missing config for loop at base {start}"),
        })?;
    let cfg = &configs[cfg_idx];
    let angle_over_paired = 2.0
        * (context.paired / (2.0 * cfg.radius))
            .clamp(-1.0, 1.0)
            .asin();
    let mut arc_state = LoopArcState::new(cfg, angle_over_paired);

    let mut i_idx = start;
    base_info[i_idx + 1].angle += context.direction * (PI - arc_state.base_to_stem_delta);
    base_info[i_idx].distance = arc_state.current_distance;
    i_idx += 1;

    let mut current_stem_count = 0;
    while i_idx < end {
        let partner = pair_table.raw_partner(i_idx);
        if partner == 0 {
            base_info[i_idx + 1].angle = -context.direction * (arc_state.backbone_delta - PI);
            base_info[i_idx].distance = arc_state.current_distance;
            i_idx += 1;
        } else if partner > i_idx {
            base_info[i_idx + 1].angle = context.direction * (PI - arc_state.base_to_stem_delta);
            current_stem_count += 1;
            handle_stem(
                pair_table,
                i_idx,
                context.paired,
                context.unpaired,
                base_info,
                configs,
                context.direction,
            )?;
            i_idx = partner;
        } else {
            if current_stem_count == 1 {
                current_stem_count = 0;
                arc_state.advance(cfg, angle_over_paired);
            }
            base_info[i_idx + 1].angle += context.direction * (PI - arc_state.base_to_stem_delta);
            base_info[i_idx].distance = arc_state.current_distance;
            i_idx += 1;
        }
    }

    if i_idx + 1 < base_info.len() {
        base_info[i_idx + 1].angle = context.direction * (PI - arc_state.base_to_stem_delta);
    }
    Ok(())
}

struct LoopArcState {
    current_arc: usize,
    current_distance: f64,
    base_to_stem_delta: f64,
    backbone_delta: f64,
}

impl LoopArcState {
    fn new(cfg: &LoopConfig, angle_over_paired: f64) -> Self {
        let mut state = Self {
            current_arc: 0,
            current_distance: 0.0,
            base_to_stem_delta: 0.0,
            backbone_delta: 0.0,
        };
        state.advance(cfg, angle_over_paired);
        state
    }

    fn advance(&mut self, cfg: &LoopConfig, angle_over_paired: f64) {
        let current_angle = cfg.arcs[self.current_arc].arc_angle;
        let current_bb_angle =
            (current_angle - angle_over_paired) / (cfg.arcs[self.current_arc].segment_count as f64);
        self.current_distance = (2.0 * cfg.radius * cfg.radius * (1.0 - current_bb_angle.cos()))
            .max(0.0)
            .sqrt();
        self.base_to_stem_delta = 0.5 * (PI + angle_over_paired + current_bb_angle);
        self.backbone_delta = PI + current_bb_angle;
        self.current_arc += 1;
    }
}

fn handle_stem(
    pair_table: &PairTable,
    mut i: usize,
    paired: f64,
    unpaired: f64,
    base_info: &mut [BaseInfo],
    configs: &[LoopConfig],
    direction: f64,
) -> Result<(), LayoutError> {
    let end = pair_table.raw_partner(i) + 1;
    i += 1;

    // A zero-length hairpin defeats the second disjunct: once `i` crosses onto
    // the closing strand, partners decrease as `i` grows and the disjunct holds
    // spuriously, so an unbounded walk would run off the end of `base_info`.
    // Requiring `i` to still be an opening base keeps the walk on its own stem,
    // mirroring the bound in `arcs::calc_stem_arcs`.
    while pair_table.raw_partner(i) > i
        && (pair_table.raw_partner(i) == end - 1
            || pair_table.raw_partner(i) + 1 == pair_table.raw_partner(i - 1))
    {
        base_info[i + 1].angle = 0.0;
        i += 1;
    }

    if pair_table.raw_partner(i) != end - 1 {
        handle_loop(
            pair_table,
            i - 1,
            paired,
            unpaired,
            base_info,
            configs,
            direction,
        )?;
    }
    Ok(())
}

pub(super) fn compute_affine_coordinates(
    pair_table: &PairTable,
    paired: f64,
    unpaired: f64,
    base_info: &mut [BaseInfo],
    configs: &[LoopConfig],
) -> Result<(), LayoutError> {
    let length = pair_table.len();
    let mut current_base = 1;
    let direction = -1.0;

    base_info[0].angle = 0.0;

    if length >= 2 {
        base_info[1].angle = base_info[0].angle;
        base_info[2].angle = base_info[1].angle;
    }

    let mut dangle_count = 0;
    while current_base < length {
        if pair_table.raw_partner(current_base) == 0 {
            current_base = handle_exterior_bases(pair_table, current_base, base_info, direction);
            dangle_count += 1;
        }

        if current_base < length {
            let p_curr = pair_table.raw_partner(current_base);
            let p_prev = pair_table.raw_partner(current_base - 1);
            if p_curr != p_prev + 1 && p_curr != 0 && p_prev != 0 {
                if current_base == 1 {
                    if dangle_count < 1 {
                        base_info[0].angle = -FRAC_PI_2;
                        base_info[1].angle = -FRAC_PI_2;
                        base_info[2].angle = -FRAC_PI_2;
                    }
                    handle_stem(
                        pair_table,
                        current_base,
                        paired,
                        unpaired,
                        base_info,
                        configs,
                        direction,
                    )?;
                    current_base = pair_table.raw_partner(current_base) + 1;
                    if current_base == length {
                        base_info[current_base].angle = -FRAC_PI_2;
                    }
                    continue;
                }
                base_info[current_base].angle += direction * FRAC_PI_2;
                base_info[current_base + 1].distance = unpaired;
                base_info[current_base + 1].angle += direction * FRAC_PI_2;
                dangle_count += 1;
            }

            handle_stem(
                pair_table,
                current_base,
                paired,
                unpaired,
                base_info,
                configs,
                direction,
            )?;
            current_base = pair_table.raw_partner(current_base) + 1;

            if current_base == length {
                current_base =
                    handle_exterior_bases(pair_table, current_base, base_info, direction);
            }
        }
    }
    Ok(())
}

pub(super) fn affine_to_cartesian(base_info: &[BaseInfo]) -> Vec<Vec2> {
    let length = base_info.len().saturating_sub(1);
    if length == 0 {
        return Vec::new();
    }

    let mut coordinates = Vec::with_capacity(length);
    coordinates.push(Vec2::zero());

    let mut angle = 0.0;
    for i in 1..length {
        angle -= base_info[i + 1].angle;
        let previous = coordinates[i - 1];
        coordinates.push(Vec2::new(
            previous.x + base_info[i].distance * angle.cos(),
            previous.y + base_info[i].distance * angle.sin(),
        ));
    }

    coordinates
}

/// Computes RNA secondary-structure coordinates with the `RNAturtle` algorithm.
///
/// # Errors
///
/// Returns [`LayoutError`] when the structure cannot be laid out.
pub(crate) fn layout(
    pair_table: &PairTable,
    options: &RnaTurtleOptions,
) -> Result<AlgorithmLayout, LayoutError> {
    if !pair_table.has_pairs() {
        return Ok(AlgorithmLayout::unpaired_line(
            pair_table,
            options.unpaired_distance,
        ));
    }

    let mut state = build_turtle_state(
        pair_table,
        options.paired_distance,
        options.unpaired_distance,
    );
    compute_affine_coordinates(
        pair_table,
        options.paired_distance,
        options.unpaired_distance,
        &mut state.base_info,
        &state.configs,
    )?;

    let coordinates = affine_to_cartesian(&state.base_info);
    let backbone =
        crate::drawing::arcs::generate_backbone(pair_table, &coordinates, options.draw_arcs);

    Ok(AlgorithmLayout::new(
        coordinates,
        backbone,
        options.unpaired_distance,
    ))
}

#[cfg(test)]
mod tests {
    use super::{cfg_generate_default_config, layout};
    use crate::drawing::config::RnaTurtleOptions;
    use crate::drawing::output::AlgorithmLayout;
    use crate::drawing::testing::{
        MOTIF_STRUCTURES, RNASE_P2_SEQUENCE, RNASE_P2_STRUCTURE, balanced_structures_up_to_length,
        pair_table,
    };
    use crate::drawing::validation::validate;

    fn turtle(structure: &str) -> AlgorithmLayout {
        layout(&pair_table(structure), &RnaTurtleOptions::default())
            .expect("RNAturtle layout must succeed")
    }

    /// True when every loop in `structure` either has a child stem or at least
    /// two unpaired bases. A stem makes the loop satisfiable. With fewer than
    /// two unpaired bases, a hairpin cannot fit its backbone steps and closing chord.
    fn hairpin_loops_are_satisfiable(table: &crate::drawing::validation::PairTable) -> bool {
        table.pairs().all(|(opener, _)| {
            let counts = table.loop_counts(opener);
            counts.stems > 0 || counts.unpaired >= 2
        })
    }

    #[test]
    fn satisfiable_structures_place_paired_bases_at_exactly_the_paired_distance() {
        // A helix walk once escaped its stem and placed paired bases at the
        // wrong spacing. Check exact spacing wherever the geometry is satisfiable.
        let options = RnaTurtleOptions::default();
        let structures = balanced_structures_up_to_length(8);
        assert_eq!(structures.len(), 537);

        let mut checked_pairs = 0;
        for structure in structures
            .iter()
            .map(String::as_str)
            .chain(MOTIF_STRUCTURES)
        {
            let table = pair_table(structure);
            if !hairpin_loops_are_satisfiable(&table) {
                continue;
            }
            let result = layout(&table, &options).expect("layout must succeed");
            for (opener, closer) in table.pairs() {
                let distance =
                    result.coordinates[opener - 1].distance(result.coordinates[closer - 1]);
                assert!(
                    (distance - options.paired_distance).abs() < 1e-6,
                    "{structure}: pair {opener}-{closer} spanned {distance}, \
                     expected {}",
                    options.paired_distance
                );
                checked_pairs += 1;
            }
        }
        // Keep a fixed count so a broken filter cannot pass the test vacuously.
        assert_eq!(
            checked_pairs, 130,
            "the satisfiable sweep must cover a fixed set of pairs"
        );
    }

    #[test]
    fn degenerate_hairpins_stay_near_the_paired_distance_they_cannot_reach() {
        // A hairpin with fewer than two unpaired bases cannot fit both its
        // backbone steps and closing chord. RNAturtle uses an approximation,
        // which must stay closer to `paired_distance` than the old failures.
        let options = RnaTurtleOptions::default();
        let mut worst: f64 = 0.0;
        let mut degenerate = 0;

        for structure in balanced_structures_up_to_length(8) {
            let table = pair_table(&structure);
            if hairpin_loops_are_satisfiable(&table) {
                continue;
            }
            degenerate += 1;
            let result = layout(&table, &options).expect("layout must succeed");
            for (opener, closer) in table.pairs() {
                let distance =
                    result.coordinates[opener - 1].distance(result.coordinates[closer - 1]);
                worst = worst.max((distance - options.paired_distance).abs());
                assert!(
                    (distance - options.paired_distance).abs()
                        < (distance - options.unpaired_distance).abs(),
                    "{structure}: pair {opener}-{closer} spanned {distance}, \
                     which is nearer the unpaired distance than the paired one"
                );
            }
        }

        assert!(degenerate > 100, "the sweep must reach degenerate hairpins");
        // The measured worst case is 3.69 units. This bound catches a regression
        // that introduces a whole-unit error.
        assert!(
            worst < 0.15 * options.paired_distance,
            "degenerate hairpin spacing drifted by {worst}"
        );
    }

    #[test]
    fn a_zero_length_hairpin_keeps_paired_spacing_alone_and_in_context() {
        // The pair and backbone edge are the same segment, so the degenerate loop
        // must use `paired_distance`.
        let options = RnaTurtleOptions::default();
        for (structure, pairs) in [
            ("()", [(0, 1)].as_slice()),
            ("(((...()...)).)", &[(0, 14), (1, 12), (2, 11), (6, 7)]),
        ] {
            let result = turtle(structure);
            for &(first, second) in pairs {
                let distance = result.coordinates[first].distance(result.coordinates[second]);
                assert!(
                    (distance - options.paired_distance).abs() < 1e-9,
                    "{structure}: pair {}-{} must use paired spacing, got {distance}",
                    first + 1,
                    second + 1
                );
            }
        }
    }

    #[test]
    fn rnase_p2_keeps_its_zero_length_pair_at_paired_spacing() {
        // A bad degenerate seed once displaced remote nucleotides by hundreds of
        // layout units. Keep this full structure as the regression case.
        let options = RnaTurtleOptions::default();
        let input = validate(RNASE_P2_SEQUENCE, RNASE_P2_STRUCTURE)
            .expect("the RNase P2 fixture must validate");
        let result = layout(&input.pair_table, &options).expect("RNase P2 must lay out");

        let distance = result.coordinates[304].distance(result.coordinates[305]);
        assert!(
            (distance - options.paired_distance).abs() < 1e-9,
            "the zero-length pair must use paired spacing, got {distance}"
        );

        // The degenerate loop must stay local. Every other pair must keep its
        // spacing. Before the fix, this seed corrupted the affine reconstruction.
        for (opener, closer) in input.pair_table.pairs() {
            let distance = result.coordinates[opener - 1].distance(result.coordinates[closer - 1]);
            assert!(
                (distance - options.paired_distance).abs() < 1e-4,
                "pair {opener}-{closer} spanned {distance}"
            );
        }
    }

    #[test]
    fn a_stacked_helix_lays_its_pairs_on_parallel_rungs() {
        // Consecutive pairs in one helix must stay parallel and evenly spaced,
        // so a stem forms a ladder instead of a fan.
        let structure = "((((((((....))))))))";
        let table = pair_table(structure);
        let result = turtle(structure);

        let pairs: Vec<(usize, usize)> = table.pairs().collect();
        let mut previous_axis = None;
        for &(opener, closer) in &pairs {
            let rung = result.coordinates[closer - 1] - result.coordinates[opener - 1];
            let axis = rung.normalized().expect("a rung must have length");
            if let Some(previous) = previous_axis {
                let turn: f64 = axis.angle_between(previous);
                assert!(
                    turn < 1e-9,
                    "stacked pairs must stay parallel, turned {turn}"
                );
            }
            previous_axis = Some(axis);
        }

        // Successive rungs advance by one backbone step along the helix.
        for window in pairs.windows(2) {
            let first = result.coordinates[window[0].0 - 1];
            let second = result.coordinates[window[1].0 - 1];
            let rise = first.distance(second);
            assert!(
                (rise - RnaTurtleOptions::default().unpaired_distance).abs() < 1e-9,
                "helix rise was {rise}"
            );
        }
    }

    #[test]
    fn scaling_both_distances_scales_the_whole_drawing() {
        let structure = "(((...)))..((...))";
        let base = turtle(structure);
        let scaled = layout(
            &pair_table(structure),
            &RnaTurtleOptions {
                paired_distance: RnaTurtleOptions::default().paired_distance * 2.0,
                unpaired_distance: RnaTurtleOptions::default().unpaired_distance * 2.0,
                draw_arcs: true,
            },
        )
        .expect("scaled layout must succeed");

        for (index, point) in base.coordinates.iter().enumerate() {
            assert!(
                (*point * 2.0).distance(scaled.coordinates[index]) < 1e-9,
                "nucleotide {index} did not scale"
            );
        }
        assert!((scaled.nominal_spacing / base.nominal_spacing - 2.0).abs() < 1e-12);
    }

    #[test]
    fn changing_only_the_paired_distance_moves_only_the_rungs() {
        // Keep the two options independent. Widening helices must not change loop
        // pitch.
        // 45 stays inside what a five-base loop at the default unpaired step
        // can carry. Far beyond that, the closing chord stops being reachable
        // and the layout falls back to an approximation.
        let structure = "(((.....)))";
        let options = RnaTurtleOptions {
            paired_distance: 45.0,
            ..RnaTurtleOptions::default()
        };
        let result = layout(&pair_table(structure), &options).expect("layout must succeed");
        let table = pair_table(structure);

        for (opener, closer) in table.pairs() {
            let distance = result.coordinates[opener - 1].distance(result.coordinates[closer - 1]);
            assert!((distance - 45.0).abs() < 1e-6, "pair span was {distance}");
        }
        for index in 3..7 {
            let step = result.coordinates[index].distance(result.coordinates[index + 1]);
            assert!(
                (step - options.unpaired_distance).abs() < 1e-9,
                "loop step {index} was {step} and must not follow paired_distance"
            );
        }
    }

    #[test]
    fn default_config_groups_loop_segments_between_stems() {
        let structures = [
            "(...)",
            "(.((...)))",
            "(((...)).)",
            "(.(...).)",
            "((...)(...))",
        ];
        let actual = structures
            .map(|structure| {
                let sequence = "A".repeat(structure.len());
                let input = validate(&sequence, structure).expect("test structure must validate");
                cfg_generate_default_config(&input.pair_table, 1, 25.0, 35.0, 50.0)
                    .arcs
                    .into_iter()
                    .map(|arc| arc.segment_count)
                    .collect::<Vec<_>>()
            })
            .to_vec();

        assert_eq!(
            actual,
            [vec![4], vec![2, 1], vec![1, 2], vec![2, 2], vec![1, 1, 1],]
        );
    }
}
