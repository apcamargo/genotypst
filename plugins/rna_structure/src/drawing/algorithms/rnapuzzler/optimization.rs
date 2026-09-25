use super::{
    IntersectionScratch, LayoutError, LoopConfig, RadiusPolicy, RnaPuzzlerOptions, TreeNode,
    apply_changes_to_config_and_bounding_boxes, collect_ancestor_nodes, collect_subtree_nodes,
    distance_to_angle, get_bounding_wedge, intersect_optimization_lists,
};
use crate::drawing::geometry::{EPSILON_3, EPSILON_7};
use std::f64::consts::{PI, TAU};

pub(super) fn optimize_tree(
    node_idx: usize,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<f64, LayoutError> {
    if !options.behavior.optimize {
        return Ok(1.0);
    }

    let mut subtree = Vec::new();
    collect_subtree_nodes(node_idx, nodes, &mut subtree);

    let mut ancestor_list = Vec::new();
    collect_ancestor_nodes(node_idx, nodes, &mut ancestor_list);

    // Every probe checks these lists again, so reuse the broad-phase buffers.
    let mut scratch = IntersectionScratch::default();

    if !intersect_optimization_lists(&subtree, &ancestor_list, &mut scratch, options, nodes) {
        return optimize_tree_recursive(
            node_idx,
            &subtree,
            &ancestor_list,
            &mut scratch,
            options,
            nodes,
            changes_applied,
        );
    }
    Ok(1.0)
}

fn optimize_tree_recursive(
    node_idx: usize,
    subtree: &[usize],
    ancestor_list: &[usize],
    scratch: &mut IntersectionScratch,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<f64, LayoutError> {
    let mut shrinking_ratio = 1.0;
    let mut min_ratio: f64;

    loop {
        if *changes_applied > options.max_config_changes {
            break;
        }

        min_ratio = 1.0;
        for i in 0..nodes[node_idx].children.len() {
            let child_idx = nodes[node_idx].children[i];
            let ratio = optimize_tree_recursive(
                child_idx,
                subtree,
                ancestor_list,
                scratch,
                options,
                nodes,
                changes_applied,
            )?;
            min_ratio = min_ratio.min(ratio);
            shrinking_ratio *= ratio;
        }

        if min_ratio < 1.0 {
            continue;
        }

        if node_idx != 0 {
            let ratio = optimize_node(
                node_idx,
                subtree,
                ancestor_list,
                scratch,
                options,
                nodes,
                changes_applied,
            )?;
            min_ratio = min_ratio.min(ratio);
            shrinking_ratio *= ratio;
        }

        if min_ratio >= 1.0 {
            break;
        }
    }

    Ok(shrinking_ratio)
}

fn compute_alphas(cfg: &LoopConfig, paired_distance: f64) -> Vec<f64> {
    let paired_angle = distance_to_angle(paired_distance, cfg.radius);
    cfg.arcs
        .iter()
        .map(|arc| (arc.arc_angle - paired_angle) / ((arc.segment_count) as f64))
        .collect()
}

fn can_shrink_alphas(alphas: &[f64], backbone_angle: f64) -> bool {
    alphas.iter().all(|&alpha| alpha > backbone_angle)
}

fn get_optimizer_spaces(
    node_idx: usize,
    config_size: usize,
    paired_angle: f64,
    nodes: &[TreeNode],
) -> Result<Vec<f64>, LayoutError> {
    let mut spaces = Vec::with_capacity(config_size);
    let mut left_bound = 0.5 * paired_angle;

    for i in 0..(config_size - 1) {
        let (min_angle, max_angle) = get_bounding_wedge(node_idx, i, nodes)?;
        spaces.push(min_angle - left_bound);
        left_bound = max_angle;
    }
    spaces.push(TAU - 0.5 * paired_angle - left_bound);

    Ok(spaces)
}

fn sorted_optimizer_indices(alphas: &[f64], spaces: &[f64]) -> Vec<usize> {
    let mut indices = (0..alphas.len()).collect::<Vec<_>>();
    indices.sort_by(|&left, &right| {
        compare_descending(alphas[left], alphas[right])
            .then_with(|| compare_descending(spaces[left], spaces[right]))
    });
    indices
}

fn compare_descending(left: f64, right: f64) -> std::cmp::Ordering {
    let difference = right - left;
    if difference > EPSILON_7 {
        std::cmp::Ordering::Greater
    } else if difference < -EPSILON_7 {
        std::cmp::Ordering::Less
    } else {
        std::cmp::Ordering::Equal
    }
}

fn compute_optimizer_deltas(
    cfg: &LoopConfig,
    decrease_index: usize,
    decrease_angle: f64,
    alphas: &[f64],
) -> Vec<f64> {
    let config_size = cfg.arcs.len();
    let mut deltas = vec![0.0; config_size];
    let sum_increase_alphas = (0..config_size)
        .filter(|&idx| idx != decrease_index)
        .map(|idx| (cfg.arcs[idx].segment_count as f64) * alphas[idx])
        .sum::<f64>();

    for (idx, delta) in deltas.iter_mut().enumerate() {
        if idx != decrease_index {
            *delta = ((cfg.arcs[idx].segment_count as f64) * alphas[idx] / sum_increase_alphas)
                * decrease_angle;
        }
    }
    deltas[decrease_index] = -decrease_angle;
    deltas
}

fn apply_optimizer_deltas(
    node_idx: usize,
    deltas: &[f64],
    target_radius: f64,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
) -> Result<(), LayoutError> {
    let cfg = nodes[node_idx].require_cfg(node_idx)?;
    let changed_radius = target_radius - cfg.radius != 0.0;
    let changed_deltas = deltas.iter().any(|&delta| delta != 0.0);
    if changed_radius || changed_deltas {
        apply_changes_to_config_and_bounding_boxes(
            node_idx,
            deltas,
            RadiusPolicy::Exact(target_radius),
            options,
            nodes,
        )?;
    }
    Ok(())
}

fn apply_optimizer_config(
    node_idx: usize,
    target_cfg: &LoopConfig,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
) -> Result<(), LayoutError> {
    let cfg = nodes[node_idx].require_cfg(node_idx)?;
    let deltas = target_cfg
        .arcs
        .iter()
        .zip(&cfg.arcs)
        .map(|(target, current)| target.arc_angle - current.arc_angle)
        .collect::<Vec<_>>();
    apply_optimizer_deltas(node_idx, &deltas, target_cfg.radius, options, nodes)
}

fn search_best_optimizer_config(
    node_idx: usize,
    mut deltas: Vec<f64>,
    subtree: &[usize],
    ancestor_list: &[usize],
    scratch: &mut IntersectionScratch,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
) -> Result<bool, LayoutError> {
    let radius = nodes[node_idx].require_cfg(node_idx)?.radius;
    apply_optimizer_deltas(node_idx, &deltas, radius, options, nodes)?;

    let num_steps = 10.0;
    let factor = 1.0 / num_steps;
    for delta in &mut deltas {
        *delta *= -factor;
    }

    let mut intersecting =
        intersect_optimization_lists(subtree, ancestor_list, scratch, options, nodes);
    if intersecting {
        for _ in 0..9 {
            let radius = nodes[node_idx].require_cfg(node_idx)?.radius;
            apply_optimizer_deltas(node_idx, &deltas, radius, options, nodes)?;
            intersecting =
                intersect_optimization_lists(subtree, ancestor_list, scratch, options, nodes);
            if !intersecting {
                break;
            }
        }
    }

    Ok(!intersecting)
}

fn shrink_loop_radius(
    node_idx: usize,
    subtree: &[usize],
    ancestor_list: &[usize],
    scratch: &mut IntersectionScratch,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
) -> Result<f64, LayoutError> {
    let cfg = nodes[node_idx].require_cfg(node_idx)?;
    let cfg_radius = cfg.radius;
    let mut min_valid_radius = cfg_radius;

    let max_radius = cfg_radius;
    let min_radius = cfg.min_radius;
    let arc_count = cfg.arcs.len();
    let min_absolute_delta = 1.0;

    if max_radius - min_radius < min_absolute_delta {
        return Ok(1.0);
    }

    let mut radius = min_radius;
    let delta = 0.1 * (max_radius - min_radius);

    let max_steps = 10;
    let zero_deltas = vec![0.0; arc_count];
    let mut exhausted = true;

    for _ in 0..max_steps {
        apply_changes_to_config_and_bounding_boxes(
            node_idx,
            &zero_deltas,
            RadiusPolicy::Exact(radius),
            options,
            nodes,
        )?;

        let intersecting =
            intersect_optimization_lists(subtree, ancestor_list, scratch, options, nodes);
        if intersecting {
            radius += delta;
        } else {
            min_valid_radius = radius;
            exhausted = false;
            break;
        }
    }

    if exhausted || nodes[node_idx].require_cfg(node_idx)?.radius > max_radius {
        apply_changes_to_config_and_bounding_boxes(
            node_idx,
            &zero_deltas,
            RadiusPolicy::Exact(min_valid_radius),
            options,
            nodes,
        )?;
    }

    Ok(nodes[node_idx].require_cfg(node_idx)?.radius / max_radius)
}

fn optimize_node(
    node_idx: usize,
    subtree: &[usize],
    ancestor_list: &[usize],
    scratch: &mut IntersectionScratch,
    options: &RnaPuzzlerOptions,
    nodes: &mut [TreeNode],
    changes_applied: &mut usize,
) -> Result<f64, LayoutError> {
    if nodes[node_idx].children.is_empty() {
        return Ok(1.0);
    }
    let cfg = nodes[node_idx].require_cfg(node_idx)?.clone();
    if cfg.radius - cfg.default_radius < 5.0 {
        return Ok(1.0);
    }

    let min_multiple = 2.0;
    let config_size = cfg.arcs.len();
    let initial_cfg = cfg;
    let mut best_cfg = initial_cfg.clone();
    let initial_radius = initial_cfg.radius;
    let mut min_sorted_index = 0;
    let mut run_nr = 0;
    let run_nr_max = 100 * config_size;
    let mut config_changed = true;
    let mut backbone_angle = 0.0;
    let mut alphas = Vec::new();
    let mut spaces = Vec::new();
    let mut sorted = Vec::new();

    while min_sorted_index < config_size && run_nr < run_nr_max {
        run_nr += 1;

        if config_changed {
            let cfg = nodes[node_idx].require_cfg(node_idx)?;
            backbone_angle = distance_to_angle(cfg.backbone_distance, cfg.radius);
            alphas = compute_alphas(cfg, options.turtle.paired_distance);

            if can_shrink_alphas(&alphas, backbone_angle)
                && shrink_loop_radius(node_idx, subtree, ancestor_list, scratch, options, nodes)?
                    < 1.0
            {
                best_cfg = nodes[node_idx].require_cfg(node_idx)?.clone();
                min_sorted_index = 0;

                let cfg = nodes[node_idx].require_cfg(node_idx)?;
                backbone_angle = distance_to_angle(cfg.backbone_distance, cfg.radius);
                alphas = compute_alphas(cfg, options.turtle.paired_distance);
            } else {
                apply_optimizer_config(node_idx, &best_cfg, options, nodes)?;
            }

            if min_sorted_index == 0 {
                let cfg = nodes[node_idx].require_cfg(node_idx)?;
                let paired_angle = distance_to_angle(options.turtle.paired_distance, cfg.radius);
                spaces = get_optimizer_spaces(node_idx, config_size, paired_angle, nodes)?;
                sorted = sorted_optimizer_indices(&alphas, &spaces);
            }
        } else {
            apply_optimizer_config(node_idx, &best_cfg, options, nodes)?;
        }

        let mut decrease_index = None;
        for (index, &current_arc) in sorted.iter().enumerate().skip(min_sorted_index) {
            let mut space = spaces[current_arc];
            if space > PI {
                space = PI;
            }
            let min_space = min_multiple * backbone_angle;
            if space > min_space {
                decrease_index = Some(current_arc);
                min_sorted_index = index + 1;
                break;
            }
        }

        let Some(decrease_index) = decrease_index else {
            break;
        };

        let cfg = nodes[node_idx].require_cfg(node_idx)?;
        let space = spaces[decrease_index];
        let min_necessary_space = (cfg.arcs[decrease_index].segment_count as f64) * backbone_angle;
        let current_necessary_space =
            (cfg.arcs[decrease_index].segment_count as f64) * alphas[decrease_index];
        let decrease_angle = 0.5 * (current_necessary_space - min_necessary_space).min(space);

        if decrease_angle < EPSILON_3 {
            continue;
        }

        let deltas = compute_optimizer_deltas(cfg, decrease_index, decrease_angle, &alphas);
        config_changed = search_best_optimizer_config(
            node_idx,
            deltas,
            subtree,
            ancestor_list,
            scratch,
            options,
            nodes,
        )?;
    }

    apply_optimizer_config(node_idx, &best_cfg, options, nodes)?;

    if best_cfg.radius < initial_cfg.radius {
        *changes_applied += 1;
    } else {
        apply_optimizer_config(node_idx, &initial_cfg, options, nodes)?;
    }

    Ok(nodes[node_idx].require_cfg(node_idx)?.radius / initial_radius)
}

#[cfg(test)]
mod tests {
    use super::{LoopConfig, distance_to_angle};
    use super::{
        can_shrink_alphas, compute_alphas, compute_optimizer_deltas, sorted_optimizer_indices,
    };
    use crate::drawing::algorithms::rnaturtle::ConfigArc;

    fn config(radius: f64, arcs: &[(usize, f64)]) -> LoopConfig {
        LoopConfig {
            radius,
            min_radius: radius / 2.0,
            default_radius: radius,
            backbone_distance: 25.0,
            arcs: arcs
                .iter()
                .map(|&(segment_count, arc_angle)| ConfigArc {
                    segment_count,
                    arc_angle,
                })
                .collect(),
        }
    }

    #[test]
    fn shrinking_requires_every_arc_to_have_slack() {
        // One tight arc blocks a shrink if shrinking would push its segments
        // below the backbone distance.
        assert!(can_shrink_alphas(&[0.5, 0.6, 0.7], 0.4));
        assert!(!can_shrink_alphas(&[0.5, 0.3, 0.7], 0.4));
        // The comparison is strict, so an arc exactly at the bound cannot shrink.
        assert!(!can_shrink_alphas(&[0.4], 0.4));
        assert!(can_shrink_alphas(&[], 0.4), "no arcs means nothing blocks");
    }

    #[test]
    fn optimizer_indices_rank_by_alpha_then_by_available_space() {
        // Try the largest alpha first. Break ties with the larger free wedge.
        let alphas = [0.1, 0.9, 0.5];
        let spaces = [1.0, 1.0, 1.0];
        assert_eq!(sorted_optimizer_indices(&alphas, &spaces), [1, 2, 0]);

        let tied = [0.5, 0.5, 0.5];
        let spaces = [0.2, 0.9, 0.4];
        assert_eq!(sorted_optimizer_indices(&tied, &spaces), [1, 2, 0]);

        // Treat differences below epsilon as ties and compare free space.
        let nearly_tied = [0.5, 0.5 + 1e-9, 0.5];
        let spaces = [0.1, 0.2, 0.9];
        assert_eq!(sorted_optimizer_indices(&nearly_tied, &spaces), [2, 1, 0]);
    }

    #[test]
    fn optimizer_deltas_conserve_the_angle_they_redistribute() {
        // The loop is closed, so an angle removed from one arc must go to the
        // others or the config will no longer sum to TAU. Each arc must still
        // span its closing chord, which keeps alpha non-negative.
        let paired_angle = distance_to_angle(35.0, 40.0);
        let cfg = config(
            40.0,
            &[
                (2, paired_angle + 0.6),
                (3, paired_angle + 1.5),
                (1, paired_angle + 0.3),
            ],
        );
        let alphas = compute_alphas(&cfg, 35.0);
        assert!(
            alphas.iter().all(|&alpha| alpha > 0.0),
            "the fixture must leave every arc some slack, got {alphas:?}"
        );
        let decrease = 0.3;

        for decrease_index in 0..cfg.arcs.len() {
            let deltas = compute_optimizer_deltas(&cfg, decrease_index, decrease, &alphas);

            assert!(
                (deltas[decrease_index] + decrease).abs() < 1e-12,
                "the chosen arc must give up exactly the requested angle"
            );
            assert!(
                deltas.iter().sum::<f64>().abs() < 1e-12,
                "deltas must sum to zero, got {deltas:?}"
            );
            for (index, &delta) in deltas.iter().enumerate() {
                if index != decrease_index {
                    assert!(
                        delta > 0.0,
                        "every other arc must gain, arc {index} got {delta}"
                    );
                }
            }
        }
    }

    #[test]
    fn redistribution_is_proportional_to_each_arcs_segment_weight() {
        // The weight is `segment_count * alpha`, while alpha divides by
        // `segment_count`. They cancel, so free angle controls the split.
        let paired_angle = distance_to_angle(35.0, 40.0);
        let cfg = config(
            40.0,
            &[
                (1, paired_angle + 0.2),
                (2, paired_angle + 0.4),
                (4, paired_angle + 0.8),
            ],
        );
        let alphas = compute_alphas(&cfg, 35.0);
        let deltas = compute_optimizer_deltas(&cfg, 0, 0.6, &alphas);

        // Free angles are 0.4 and 0.8, so the second arc takes twice as much
        // even though it carries twice as many segments as well.
        assert!((deltas[2] / deltas[1] - 2.0).abs() < 1e-9, "{deltas:?}");
        assert!((deltas[1] + deltas[2] - 0.6).abs() < 1e-12);

        // Segment counts alone must not shift the split.
        let uniform = config(
            40.0,
            &[
                (1, paired_angle + 0.5),
                (2, paired_angle + 0.5),
                (4, paired_angle + 0.5),
            ],
        );
        let uniform_alphas = compute_alphas(&uniform, 35.0);
        let uniform_deltas = compute_optimizer_deltas(&uniform, 0, 0.6, &uniform_alphas);
        assert!(
            (uniform_deltas[1] - uniform_deltas[2]).abs() < 1e-12,
            "equal free angles must split evenly, got {uniform_deltas:?}"
        );
    }
}
