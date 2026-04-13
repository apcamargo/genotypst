//! genome-map: Label packing and leader routing backend for genome maps.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[cfg(target_arch = "wasm32")]
use wasm_minimal_protocol::*;

#[cfg(target_arch = "wasm32")]
initiate_protocol!();

const Y_EPSILON_PT: f64 = 1e-9;

/// Request from Typst for the full label layout pipeline.
///
/// Typst measures label text and sends horizontal geometry. Rust handles:
/// sorting, first-fit level packing, vertical geometry, and leader routing.
#[derive(Debug, Clone, Deserialize)]
struct LayoutRequest {
    label_height_pt: f64,
    /// Spacing for the interval overlap predicate during level packing.
    label_horizontal_gap_pt: f64,
    label_vertical_gap_pt: f64,
    /// Horizontal clearance added around label boxes for leader-routing hit
    /// testing. Different from `label_horizontal_gap_pt`.
    label_line_distance_pt: f64,
    label_track_gap_pt: f64,
    label_leader_offset_pt: f64,
    labels: Vec<MeasuredLabel>,
}

/// Per-label measured geometry sent from Typst (in original gene order).
#[derive(Debug, Clone, Deserialize)]
struct MeasuredLabel {
    /// Label center x-position used as the primary sort key for render order.
    center_pt: f64,
    left_pt: f64,
    right_pt: f64,
    dodge_left_pt: f64,
    dodge_right_pt: f64,
    /// Packing span (dodge_right - dodge_left) used as the primary sort key
    /// for level assignment. Larger spans are packed first.
    packing_span_pt: f64,
    /// Gene center x-position. Serves as both the leader-routing query point
    /// and the leader-line x-position for rendering.
    gene_center_pt: f64,
}

/// Response to Typst from the full layout pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LayoutResponse {
    level_count: usize,
    level_block_height_pt: f64,
    labels: Vec<PositionedLabel>,
}

/// Per-label positioned output returned to Typst.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PositionedLabel {
    source_index: usize,
    top_pt: f64,
    underline_y_pt: f64,
    leader_segments: Vec<LeaderSegment>,
}

/// Internal working struct that carries packing-relevant fields through both
/// sort phases. Fields only needed for Typst-side rendering (underline_left,
/// underline_width) are not included — the Typst wrapper reattaches them via
/// source_index before rendering.
#[derive(Debug, Clone)]
struct WorkingLabel {
    source_index: usize,
    center_pt: f64,
    left_pt: f64,
    right_pt: f64,
    dodge_left_pt: f64,
    dodge_right_pt: f64,
    packing_span_pt: f64,
    gene_center_pt: f64,
    level: usize,
    packed_rank: usize,
}

/// Per-level state: intervals sorted by left edge for efficient overlap checks.
struct LevelState {
    intervals: Vec<(f64, f64)>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct LabelInput {
    level: usize,
    hit_left_pt: f64,
    hit_right_pt: f64,
    query_x_pt: f64,
    line_top_pt: f64,
    raw_top_pt: f64,
    raw_bottom_pt: f64,
    block_top_pt: f64,
    block_bottom_pt: f64,
}

#[derive(Debug, Clone, Copy)]
struct RoutingLabel {
    level: usize,
    hit_left_pt: f64,
    hit_right_pt: f64,
    query_x_pt: f64,
    line_top_pt: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LeaderSegment {
    top_pt: f64,
    length_pt: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EventKind {
    Start,
    Query,
    End,
}

#[derive(Debug, Clone, Copy)]
struct SweepEvent {
    x_pt: f64,
    kind: EventKind,
    label_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct LevelGeometry {
    raw_top_pt: f64,
    raw_bottom_pt: f64,
    block_top_pt: f64,
    block_bottom_pt: f64,
}

#[derive(Debug, Clone, Copy)]
struct LayoutGeometry {
    level_block_height_pt: f64,
    line_bottom_pt: f64,
    level_step_pt: f64,
    level_base_top_pt: f64,
    label_height_pt: f64,
    label_line_clearance_pt: f64,
    underline_offset_pt: f64,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
struct VerticalGeometry {
    top_pt: f64,
    underline_y_pt: f64,
}

fn validate_layout_request(request: &LayoutRequest) -> Result<(), String> {
    for (name, value) in [
        ("label_height_pt", request.label_height_pt),
        ("label_horizontal_gap_pt", request.label_horizontal_gap_pt),
        ("label_vertical_gap_pt", request.label_vertical_gap_pt),
        ("label_line_distance_pt", request.label_line_distance_pt),
        ("label_track_gap_pt", request.label_track_gap_pt),
        ("label_leader_offset_pt", request.label_leader_offset_pt),
    ] {
        if !value.is_finite() {
            return Err(format!("{name} must be finite"));
        }
    }

    for (index, label) in request.labels.iter().enumerate() {
        for (name, value) in [
            ("center_pt", label.center_pt),
            ("left_pt", label.left_pt),
            ("right_pt", label.right_pt),
            ("dodge_left_pt", label.dodge_left_pt),
            ("dodge_right_pt", label.dodge_right_pt),
            ("packing_span_pt", label.packing_span_pt),
            ("gene_center_pt", label.gene_center_pt),
        ] {
            if !value.is_finite() {
                return Err(format!("labels[{index}].{name} must be finite"));
            }
        }
    }

    Ok(())
}

fn build_working_labels(request: &LayoutRequest) -> Vec<WorkingLabel> {
    let mut working = Vec::with_capacity(request.labels.len());
    for (source_index, label) in request.labels.iter().enumerate() {
        working.push(WorkingLabel {
            source_index,
            center_pt: label.center_pt,
            left_pt: label.left_pt,
            right_pt: label.right_pt,
            dodge_left_pt: label.dodge_left_pt,
            dodge_right_pt: label.dodge_right_pt,
            packing_span_pt: label.packing_span_pt,
            gene_center_pt: label.gene_center_pt,
            level: 0,
            packed_rank: 0,
        });
    }
    working
}

fn sort_for_packing(labels: &mut [WorkingLabel]) {
    // Match the previous Typst/WASM packing order: descending packing span, then
    // ascending center, then ascending source index for determinism.
    labels.sort_unstable_by(|a, b| {
        b.packing_span_pt
            .total_cmp(&a.packing_span_pt)
            .then_with(|| a.center_pt.total_cmp(&b.center_pt))
            .then_with(|| a.source_index.cmp(&b.source_index))
    });
}

/// Checks whether a candidate interval fits in a level without overlapping any
/// existing interval under the given spacing.
///
/// When `negative_spacing` is false, only the immediate predecessor and
/// successor can conflict because existing intervals are pairwise
/// non-overlapping under the same non-negative spacing. When spacing is
/// negative, any interval can potentially overlap across gaps, so a full scan
/// is required.
fn fits_in_level(
    intervals: &[(f64, f64)],
    left: f64,
    right: f64,
    spacing: f64,
    negative_spacing: bool,
) -> bool {
    if intervals.is_empty() {
        return true;
    }

    if negative_spacing {
        !intervals
            .iter()
            .any(|&(il, ir)| left < ir + spacing && right > il - spacing)
    } else {
        let pos = intervals.partition_point(|&(l, _)| l < left);
        // Check successor (leftmost interval starting at or after `left`).
        if pos < intervals.len() {
            let (succ_l, _) = intervals[pos];
            if right > succ_l - spacing {
                return false;
            }
        }
        // Check predecessor (rightmost interval starting before `left`).
        if pos > 0 {
            let (_, pred_r) = intervals[pos - 1];
            if left < pred_r + spacing {
                return false;
            }
        }
        true
    }
}

/// Assigns each working label to a dodge level using first-fit packing.
///
/// Labels must be pre-sorted by the Typst packing key (descending canonical
/// packing span, ascending center) before calling this function. The algorithm
/// scans levels from 0 upward and places each label in the first level where it
/// fits.
fn assign_levels(labels: &mut [WorkingLabel], spacing: f64) -> usize {
    let negative_spacing = spacing < 0.0;
    let mut levels: Vec<LevelState> = Vec::with_capacity(8);

    for label in labels.iter_mut() {
        let left = label.dodge_left_pt;
        let right = label.dodge_right_pt;

        let mut assigned = false;
        for (idx, level) in levels.iter_mut().enumerate() {
            if fits_in_level(&level.intervals, left, right, spacing, negative_spacing) {
                label.level = idx;
                // Insert interval maintaining sorted order by left edge.
                let pos = level.intervals.partition_point(|&(l, _)| l < left);
                level.intervals.insert(pos, (left, right));
                assigned = true;
                break;
            }
        }

        if !assigned {
            label.level = levels.len();
            levels.push(LevelState {
                intervals: vec![(left, right)],
            });
        }
    }

    levels.len()
}

fn layout_geometry(request: &LayoutRequest, level_count: usize) -> LayoutGeometry {
    let label_height_pt = request.label_height_pt;
    let level_block_height_pt = level_count as f64 * label_height_pt
        + level_count.saturating_sub(1) as f64 * request.label_vertical_gap_pt;

    LayoutGeometry {
        level_block_height_pt,
        line_bottom_pt: level_block_height_pt + request.label_track_gap_pt
            - request.label_leader_offset_pt,
        level_step_pt: label_height_pt + request.label_vertical_gap_pt,
        level_base_top_pt: level_block_height_pt - label_height_pt,
        label_height_pt,
        label_line_clearance_pt: label_height_pt * 0.25,
        underline_offset_pt: label_height_pt * 0.14,
    }
}

#[cfg(test)]
fn vertical_geometry(layout: &LayoutGeometry, level: usize) -> VerticalGeometry {
    let top_pt = layout.level_base_top_pt - level as f64 * layout.level_step_pt;
    let bottom_pt = top_pt + layout.label_height_pt;

    VerticalGeometry {
        top_pt,
        underline_y_pt: bottom_pt + layout.underline_offset_pt,
    }
}

fn precompute_level_geometries(layout: &LayoutGeometry, level_count: usize) -> Vec<LevelGeometry> {
    let mut geometries = Vec::with_capacity(level_count);
    let mut top_pt = layout.level_base_top_pt;
    for _ in 0..level_count {
        let bottom_pt = top_pt + layout.label_height_pt;
        geometries.push(LevelGeometry {
            raw_top_pt: top_pt,
            raw_bottom_pt: bottom_pt,
            block_top_pt: top_pt - layout.label_line_clearance_pt,
            block_bottom_pt: bottom_pt + layout.label_line_clearance_pt,
        });
        top_pt -= layout.level_step_pt;
    }

    geometries
}

fn compute_layout(request: &LayoutRequest) -> Result<LayoutResponse, String> {
    validate_layout_request(request)?;

    if request.labels.is_empty() {
        return Ok(LayoutResponse {
            level_count: 0,
            level_block_height_pt: 0.0,
            labels: Vec::new(),
        });
    }

    let mut working = build_working_labels(request);

    // Step 1: Sort by the transmitted packing key.
    sort_for_packing(&mut working);

    // Step 2: Assign levels with first-fit packing.
    let level_count = assign_levels(&mut working, request.label_horizontal_gap_pt);

    // Step 3: Sort by center position for render/routing order while
    // preserving the packed order for equal center positions.
    for (packed_rank, label) in working.iter_mut().enumerate() {
        label.packed_rank = packed_rank;
    }
    working.sort_unstable_by(|a, b| {
        a.center_pt
            .total_cmp(&b.center_pt)
            .then_with(|| a.packed_rank.cmp(&b.packed_rank))
    });

    // Step 4: Compute shared vertical geometry (exact port of Typst formulas).
    let layout = layout_geometry(request, level_count);
    let level_geometries = precompute_level_geometries(&layout, level_count);

    // Step 5: Build routing inputs and positioned labels from shared geometry.
    let mut routing_labels = Vec::with_capacity(working.len());
    let mut positioned = Vec::with_capacity(working.len());
    for wl in &working {
        let geometry = level_geometries[wl.level];
        let underline_y_pt = geometry.raw_bottom_pt + layout.underline_offset_pt;
        routing_labels.push(RoutingLabel {
            level: wl.level,
            hit_left_pt: wl.left_pt - request.label_line_distance_pt,
            hit_right_pt: wl.right_pt + request.label_line_distance_pt,
            query_x_pt: wl.gene_center_pt,
            line_top_pt: underline_y_pt,
        });
        positioned.push(PositionedLabel {
            source_index: wl.source_index,
            top_pt: geometry.raw_top_pt,
            underline_y_pt,
            leader_segments: Vec::new(),
        });
    }

    let leader_segments = compute_leader_segments_with_geometry(
        &routing_labels,
        layout.line_bottom_pt,
        &level_geometries,
    );

    // Step 6: Attach routed segments to the already-positioned labels.
    for (label, segments) in positioned.iter_mut().zip(leader_segments) {
        label.leader_segments = segments;
    }

    Ok(LayoutResponse {
        level_count,
        level_block_height_pt: layout.level_block_height_pt,
        labels: positioned,
    })
}

#[cfg_attr(target_arch = "wasm32", wasm_func)]
/// Full label layout pipeline: packing, vertical geometry, and leader routing.
///
/// Typst sends measured label geometry. Rust sorts by packing key, assigns
/// dodge levels with first-fit, computes vertical positions, routes leader
/// segments, and returns positioned labels.
pub fn layout_labels(config: &[u8]) -> Result<Vec<u8>, String> {
    let request: LayoutRequest =
        serde_json::from_slice(config).map_err(|e| format!("Invalid config JSON: {e}"))?;
    let response = compute_layout(&request)?;
    serde_json::to_vec(&response).map_err(|e| format!("Serialization failed: {e}"))
}

fn raw_box_overlaps_line_span(
    raw_top_pt: f64,
    raw_bottom_pt: f64,
    line_top_pt: f64,
    line_bottom_pt: f64,
) -> bool {
    raw_bottom_pt + Y_EPSILON_PT >= line_top_pt && raw_top_pt <= line_bottom_pt + Y_EPSILON_PT
}

fn clip_blocked_interval(
    mut block_start_pt: f64,
    mut block_end_pt: f64,
    line_top_pt: f64,
    line_bottom_pt: f64,
) -> Option<(f64, f64)> {
    if block_end_pt <= line_top_pt + Y_EPSILON_PT || block_start_pt >= line_bottom_pt - Y_EPSILON_PT
    {
        return None;
    }

    block_start_pt = block_start_pt.max(line_top_pt);
    block_end_pt = block_end_pt.min(line_bottom_pt);
    if block_end_pt <= block_start_pt + Y_EPSILON_PT {
        return None;
    }

    Some((block_start_pt, block_end_pt))
}

fn visible_segments_from_blocked_intervals<I>(
    line_top_pt: f64,
    line_bottom_pt: f64,
    blocked_intervals: I,
) -> Vec<LeaderSegment>
where
    I: IntoIterator<Item = (f64, f64)>,
{
    if line_bottom_pt <= line_top_pt + Y_EPSILON_PT {
        return Vec::new();
    }

    // `blocked_intervals` must arrive ordered by start position.
    let mut segments = Vec::new();
    let mut cursor_pt = line_top_pt;
    let mut current_run: Option<(f64, f64)> = None;

    for (block_start_pt, block_end_pt) in blocked_intervals {
        match current_run {
            None => current_run = Some((block_start_pt, block_end_pt)),
            Some((start_pt, end_pt)) => {
                if block_start_pt <= end_pt + Y_EPSILON_PT {
                    current_run = Some((start_pt, end_pt.max(block_end_pt)));
                } else {
                    if start_pt > cursor_pt + Y_EPSILON_PT {
                        segments.push(LeaderSegment {
                            top_pt: cursor_pt,
                            length_pt: start_pt - cursor_pt,
                        });
                    }
                    if end_pt > cursor_pt {
                        cursor_pt = end_pt;
                    }
                    current_run = Some((block_start_pt, block_end_pt));
                }
            }
        }
    }

    if let Some((block_start_pt, block_end_pt)) = current_run {
        if block_start_pt > cursor_pt + Y_EPSILON_PT {
            segments.push(LeaderSegment {
                top_pt: cursor_pt,
                length_pt: block_start_pt - cursor_pt,
            });
        }
        if block_end_pt > cursor_pt {
            cursor_pt = block_end_pt;
        }
    }

    if line_bottom_pt > cursor_pt + Y_EPSILON_PT {
        segments.push(LeaderSegment {
            top_pt: cursor_pt,
            length_pt: line_bottom_pt - cursor_pt,
        });
    }

    segments
}

fn visible_segments_for_levels(
    label_level: usize,
    line_top_pt: f64,
    line_bottom_pt: f64,
    active_levels: &BTreeSet<usize>,
    level_geometries: &[LevelGeometry],
) -> Vec<LeaderSegment> {
    let blocked_intervals = active_levels
        .range(..label_level)
        .rev()
        .filter_map(|level| {
            let geometry = level_geometries[*level];
            if !raw_box_overlaps_line_span(
                geometry.raw_top_pt,
                geometry.raw_bottom_pt,
                line_top_pt,
                line_bottom_pt,
            ) {
                return None;
            }
            clip_blocked_interval(
                geometry.block_top_pt,
                geometry.block_bottom_pt,
                line_top_pt,
                line_bottom_pt,
            )
        });

    visible_segments_from_blocked_intervals(line_top_pt, line_bottom_pt, blocked_intervals)
}

#[cfg(test)]
fn validate_or_insert_level_geometry(
    level_geometries: &mut [Option<LevelGeometry>],
    label_index: usize,
    level: usize,
    geometry: LevelGeometry,
) -> Result<(), String> {
    match level_geometries[level] {
        Some(existing_geometry) => {
            for (field_name, existing, candidate) in [
                (
                    "raw_top_pt",
                    existing_geometry.raw_top_pt,
                    geometry.raw_top_pt,
                ),
                (
                    "raw_bottom_pt",
                    existing_geometry.raw_bottom_pt,
                    geometry.raw_bottom_pt,
                ),
                (
                    "block_top_pt",
                    existing_geometry.block_top_pt,
                    geometry.block_top_pt,
                ),
                (
                    "block_bottom_pt",
                    existing_geometry.block_bottom_pt,
                    geometry.block_bottom_pt,
                ),
            ] {
                if (existing - candidate).abs() > Y_EPSILON_PT {
                    return Err(format!(
                        "labels[{label_index}].{field_name} conflicts with level {level}"
                    ));
                }
            }
        }
        None => level_geometries[level] = Some(geometry),
    }

    Ok(())
}

// Sweep across precomputed label hit intervals and scan the exact active lower
// levels for each leader query.
fn compute_leader_segments_with_geometry(
    labels: &[RoutingLabel],
    line_bottom_pt: f64,
    level_geometries: &[LevelGeometry],
) -> Vec<Vec<LeaderSegment>> {
    if labels.is_empty() {
        return Vec::new();
    }

    let mut events = Vec::with_capacity(labels.len() * 3);
    for (label_index, label) in labels.iter().enumerate() {
        events.push(SweepEvent {
            x_pt: label.hit_left_pt,
            kind: EventKind::Start,
            label_index,
        });
        events.push(SweepEvent {
            x_pt: label.query_x_pt,
            kind: EventKind::Query,
            label_index,
        });
        events.push(SweepEvent {
            x_pt: label.hit_right_pt,
            kind: EventKind::End,
            label_index,
        });
    }

    events.sort_unstable_by(|a, b| {
        a.x_pt
            .total_cmp(&b.x_pt)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.label_index.cmp(&b.label_index))
    });

    let mut segments_by_label = vec![Vec::<LeaderSegment>::new(); labels.len()];
    let mut active_counts = vec![0usize; level_geometries.len()];
    let mut active_levels = BTreeSet::<usize>::new();
    for event in events {
        match event.kind {
            EventKind::Start => {
                let level = labels[event.label_index].level;
                if active_counts[level] == 0 {
                    active_levels.insert(level);
                }
                active_counts[level] += 1;
            }
            EventKind::Query => {
                let label = labels[event.label_index];
                segments_by_label[event.label_index] = visible_segments_for_levels(
                    label.level,
                    label.line_top_pt,
                    line_bottom_pt,
                    &active_levels,
                    level_geometries,
                );
            }
            EventKind::End => {
                let level = labels[event.label_index].level;
                active_counts[level] -= 1;
                if active_counts[level] == 0 {
                    active_levels.remove(&level);
                }
            }
        }
    }

    segments_by_label
}

// Validation-preserving wrapper kept for tests.
#[cfg(test)]
fn compute_leader_segments(
    labels: &[LabelInput],
    line_bottom_pt: f64,
) -> Result<Vec<Vec<LeaderSegment>>, String> {
    if labels.is_empty() {
        return Ok(Vec::new());
    }

    let level_count = labels
        .iter()
        .map(|label| label.level)
        .max()
        .expect("non-empty request")
        + 1;
    let mut level_geometries = vec![None::<LevelGeometry>; level_count];
    let mut routing_labels = Vec::with_capacity(labels.len());
    for (label_index, label) in labels.iter().enumerate() {
        validate_or_insert_level_geometry(
            &mut level_geometries,
            label_index,
            label.level,
            LevelGeometry {
                raw_top_pt: label.raw_top_pt,
                raw_bottom_pt: label.raw_bottom_pt,
                block_top_pt: label.block_top_pt,
                block_bottom_pt: label.block_bottom_pt,
            },
        )?;
        routing_labels.push(RoutingLabel {
            level: label.level,
            hit_left_pt: label.hit_left_pt,
            hit_right_pt: label.hit_right_pt,
            query_x_pt: label.query_x_pt,
            line_top_pt: label.line_top_pt,
        });
    }

    let level_geometries: Vec<LevelGeometry> = level_geometries
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "test routing inputs must use dense levels".to_string())?;

    Ok(compute_leader_segments_with_geometry(
        &routing_labels,
        line_bottom_pt,
        &level_geometries,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL_HEIGHT_PT: f64 = 10.0;
    const LABEL_TRACK_GAP_PT: f64 = 6.0;
    const LABEL_LEADER_OFFSET_PT: f64 = 4.0;
    const LABEL_LINE_DISTANCE_PT: f64 = 1.0;
    const UNDERLINE_OFFSET_PT: f64 = LABEL_HEIGHT_PT * 0.14;
    const CLEARANCE_PT: f64 = LABEL_HEIGHT_PT * 0.25;

    fn assert_close(left: f64, right: f64) {
        assert!(
            (left - right).abs() < 1e-6,
            "expected {left} to be close to {right}"
        );
    }

    fn assert_segments(actual: &[LeaderSegment], expected: &[(f64, f64)]) {
        assert_eq!(actual.len(), expected.len());
        for (segment, (top_pt, length_pt)) in actual.iter().zip(expected.iter().copied()) {
            assert_close(segment.top_pt, top_pt);
            assert_close(segment.length_pt, length_pt);
        }
    }

    fn level_block_height(level_count: usize, label_vertical_gap_pt: f64) -> f64 {
        level_count as f64 * LABEL_HEIGHT_PT
            + (level_count.saturating_sub(1)) as f64 * label_vertical_gap_pt
    }

    fn label(
        level_count: usize,
        label_vertical_gap_pt: f64,
        level: usize,
        left_pt: f64,
        right_pt: f64,
        query_x_pt: f64,
    ) -> LabelInput {
        let block_height_pt = level_block_height(level_count, label_vertical_gap_pt);
        let level_step_pt = LABEL_HEIGHT_PT + label_vertical_gap_pt;
        let level_base_top_pt = block_height_pt - LABEL_HEIGHT_PT;
        let top_pt = level_base_top_pt - level as f64 * level_step_pt;
        let bottom_pt = top_pt + LABEL_HEIGHT_PT;
        LabelInput {
            level,
            hit_left_pt: left_pt - LABEL_LINE_DISTANCE_PT,
            hit_right_pt: right_pt + LABEL_LINE_DISTANCE_PT,
            query_x_pt,
            line_top_pt: bottom_pt + UNDERLINE_OFFSET_PT,
            raw_top_pt: top_pt,
            raw_bottom_pt: bottom_pt,
            block_top_pt: top_pt - CLEARANCE_PT,
            block_bottom_pt: bottom_pt + CLEARANCE_PT,
        }
    }

    fn routed_segments(
        level_count: usize,
        label_vertical_gap_pt: f64,
        labels: Vec<LabelInput>,
    ) -> Vec<Vec<LeaderSegment>> {
        compute_leader_segments(
            &labels,
            level_block_height(level_count, label_vertical_gap_pt) + LABEL_TRACK_GAP_PT
                - LABEL_LEADER_OFFSET_PT,
        )
        .unwrap()
    }

    #[test]
    fn lower_level_blocker_splits_visible_leader_segments() {
        let segments = routed_segments(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 20.0, 40.0, 30.0),
                label(2, 4.0, 1, 24.0, 44.0, 30.0),
            ],
        );

        assert_segments(&segments[1], &[(11.4, 0.1)]);
    }

    #[test]
    fn non_blocking_lower_level_keeps_full_leader() {
        let segments = routed_segments(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 0.0, 8.0, 4.0),
                label(2, 4.0, 1, 10.0, 28.0, 34.0),
            ],
        );

        assert_segments(&segments[1], &[(11.4, 14.6)]);
    }

    #[test]
    fn clearance_only_overlap_keeps_full_leader() {
        let labels = vec![
            label(2, 4.0, 0, 20.0, 40.0, 30.0),
            label(2, 4.0, 1, 24.0, 44.0, 30.0),
        ];
        let segments =
            compute_leader_segments(&labels, level_block_height(2, 4.0) + 1.0 - 12.0).unwrap();

        assert_segments(&segments[1], &[(11.4, 1.6)]);
    }

    #[test]
    fn blocker_starting_at_query_x_is_active_for_routing() {
        let segments = routed_segments(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 11.0, 31.0, 21.0),
                label(2, 4.0, 1, 0.0, 20.0, 10.0),
            ],
        );

        assert_segments(&segments[1], &[(11.4, 0.1)]);
    }

    #[test]
    fn same_level_multiplicity_keeps_blocker_active_until_last_label_ends() {
        let segments = routed_segments(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 0.0, 9.0, 4.0),
                label(2, 4.0, 0, 0.0, 20.0, 10.0),
                label(2, 4.0, 1, 20.0, 40.0, 10.5),
            ],
        );

        assert_segments(&segments[2], &[(11.4, 0.1)]);
    }

    #[test]
    fn multiple_active_levels_merge_when_clearance_overlaps() {
        let segments = routed_segments(
            3,
            1.0,
            vec![
                label(3, 1.0, 0, 20.0, 40.0, 30.0),
                label(3, 1.0, 1, 20.0, 40.0, 30.0),
                label(3, 1.0, 2, 20.0, 40.0, 30.0),
            ],
        );

        assert!(segments[2].is_empty());
    }

    // -----------------------------------------------------------------------
    // Layout tests: label packing + vertical geometry + end-to-end pipeline
    // -----------------------------------------------------------------------

    /// Faithful reference packer: line-for-line transliteration of the Typst
    /// `_assign-label-levels` function. Intentionally O(n*m*k) — its purpose
    /// is to be obviously correct, not fast.
    fn reference_assign_levels(labels: &mut [WorkingLabel], spacing: f64) -> usize {
        let mut level_intervals: Vec<Vec<(f64, f64)>> = Vec::new();

        for label in labels.iter_mut() {
            let left = label.dodge_left_pt;
            let right = label.dodge_right_pt;
            let mut assigned_level = None;

            for (idx, intervals) in level_intervals.iter_mut().enumerate() {
                let overlaps = intervals
                    .iter()
                    .any(|&(il, ir)| left < ir + spacing && right > il - spacing);
                if !overlaps {
                    assigned_level = Some(idx);
                    intervals.push((left, right));
                    break;
                }
            }

            if assigned_level.is_none() {
                assigned_level = Some(level_intervals.len());
                level_intervals.push(vec![(left, right)]);
            }
            label.level = assigned_level.unwrap();
        }

        level_intervals.len()
    }

    fn make_layout_request(
        label_height_pt: f64,
        label_horizontal_gap_pt: f64,
        label_vertical_gap_pt: f64,
        label_line_distance_pt: f64,
        label_track_gap_pt: f64,
        label_leader_offset_pt: f64,
        labels: Vec<MeasuredLabel>,
    ) -> LayoutRequest {
        LayoutRequest {
            label_height_pt,
            label_horizontal_gap_pt,
            label_vertical_gap_pt,
            label_line_distance_pt,
            label_track_gap_pt,
            label_leader_offset_pt,
            labels,
        }
    }

    fn simple_measured_label(
        center_pt: f64,
        half_width_pt: f64,
        gene_center_pt: f64,
    ) -> MeasuredLabel {
        let left = center_pt - half_width_pt;
        let right = center_pt + half_width_pt;
        MeasuredLabel {
            center_pt,
            left_pt: left,
            right_pt: right,
            dodge_left_pt: left,
            dodge_right_pt: right,
            packing_span_pt: half_width_pt * 2.0,
            gene_center_pt,
        }
    }

    fn measured_label(
        center_pt: f64,
        left_pt: f64,
        right_pt: f64,
        packing_span_pt: f64,
        gene_center_pt: f64,
    ) -> MeasuredLabel {
        MeasuredLabel {
            center_pt,
            left_pt,
            right_pt,
            dodge_left_pt: left_pt,
            dodge_right_pt: right_pt,
            packing_span_pt,
            gene_center_pt,
        }
    }

    fn assert_layout_matches_reference_levels(request: &LayoutRequest) {
        let mut working = build_working_labels(request);
        sort_for_packing(&mut working);
        let level_count = reference_assign_levels(&mut working, request.label_horizontal_gap_pt);
        working.sort_by(|a, b| a.center_pt.total_cmp(&b.center_pt));

        let layout = layout_geometry(request, level_count);
        let response = compute_layout(request).unwrap();
        assert_eq!(response.level_count, level_count);
        assert_close(response.level_block_height_pt, layout.level_block_height_pt);
        assert_eq!(response.labels.len(), working.len());

        for (actual, expected) in response.labels.iter().zip(&working) {
            let vertical = vertical_geometry(&layout, expected.level);
            assert_eq!(actual.source_index, expected.source_index);
            assert_close(actual.top_pt, vertical.top_pt);
            assert_close(actual.underline_y_pt, vertical.underline_y_pt);
        }
    }

    #[test]
    fn layout_matches_reference_equal_spans() {
        // All labels have equal spans — tests stable sort tie-breaking
        let labels: Vec<MeasuredLabel> = (0..10)
            .map(|i| simple_measured_label(15.0 + i as f64 * 8.0, 10.0, 15.0 + i as f64 * 8.0))
            .collect();
        let request = make_layout_request(10.0, 1.0, 4.0, 1.0, 6.0, 4.0, labels);
        assert_layout_matches_reference_levels(&request);
    }

    #[test]
    fn layout_matches_reference_just_touching() {
        // Labels whose dodge spans just touch at the spacing boundary
        let spacing = 1.0;
        let labels = vec![
            simple_measured_label(15.0, 10.0, 15.0), // dodge: 5..25
            simple_measured_label(36.0, 10.0, 36.0), // dodge: 26..46, gap = 1.0 = spacing
        ];
        let request = make_layout_request(10.0, spacing, 4.0, 1.0, 6.0, 4.0, labels);
        assert_layout_matches_reference_levels(&request);
    }

    #[test]
    fn layout_matches_reference_negative_spacing() {
        // Negative spacing: exercises the full-scan fallback path
        let labels: Vec<MeasuredLabel> = (0..8)
            .map(|i| simple_measured_label(10.0 + i as f64 * 6.0, 4.0, 10.0 + i as f64 * 6.0))
            .collect();
        let request = make_layout_request(10.0, -0.5, 4.0, 1.0, 6.0, 4.0, labels);
        assert_layout_matches_reference_levels(&request);
    }

    #[test]
    fn layout_preserves_source_order_for_equal_packing_and_center() {
        let request = make_layout_request(
            10.0,
            1.0,
            4.0,
            1.0,
            6.0,
            4.0,
            vec![
                measured_label(10.0, 0.0, 20.0, 20.0, 10.0),
                measured_label(10.0, 0.0, 20.0, 20.0, 10.0),
            ],
        );
        let response = compute_layout(&request).unwrap();
        let source_order: Vec<usize> = response
            .labels
            .iter()
            .map(|label| label.source_index)
            .collect();

        assert_eq!(source_order, vec![0, 1]);
    }

    /// Seeded deterministic differential test comparing the optimized packer
    /// against the reference implementation across many random configurations.
    #[test]
    fn layout_differential_randomized() {
        // Simple deterministic PRNG (xorshift64)
        let mut state: u64 = 0xDEAD_BEEF_CAFE_BABE;
        let mut next = || -> f64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state as f64) / (u64::MAX as f64)
        };

        for trial in 0..200 {
            let n = 2 + (next() * 30.0) as usize;
            let label_height = 8.0 + next() * 6.0;
            let h_gap = if trial % 20 == 0 {
                -0.5 + next() * 0.5 // occasional negative spacing
            } else {
                next() * 2.0
            };
            let v_gap = 1.0 + next() * 5.0;
            let line_dist = 0.5 + next() * 1.5;
            let track_gap = 3.0 + next() * 5.0;
            let leader_offset = 2.0 + next() * 6.0;

            let labels: Vec<MeasuredLabel> = (0..n)
                .map(|_| {
                    let center = next() * 300.0;
                    let half_width = 3.0 + next() * 25.0;
                    let gene_center = center + (next() - 0.5) * 10.0;
                    simple_measured_label(center, half_width, gene_center)
                })
                .collect();

            let request = make_layout_request(
                label_height,
                h_gap,
                v_gap,
                line_dist,
                track_gap,
                leader_offset,
                labels,
            );

            assert_layout_matches_reference_levels(&request);
        }
    }

    #[test]
    fn layout_labels_json_round_trip() {
        let json = r#"{
            "label_height_pt": 10.0,
            "label_horizontal_gap_pt": 1.0,
            "label_vertical_gap_pt": 4.0,
            "label_line_distance_pt": 1.0,
            "label_track_gap_pt": 6.0,
            "label_leader_offset_pt": 4.0,
            "labels": [
                {
                    "center_pt": 50.0,
                    "left_pt": 35.0,
                    "right_pt": 65.0,
                    "dodge_left_pt": 35.0,
                    "dodge_right_pt": 65.0,
                    "packing_span_pt": 30.0,
                    "gene_center_pt": 50.0
                }
            ]
        }"#;
        let result = layout_labels(json.as_bytes()).unwrap();
        let response: LayoutResponse = serde_json::from_slice(&result).unwrap();
        assert_eq!(response.level_count, 1);
        assert_eq!(response.labels.len(), 1);
        assert_eq!(response.labels[0].source_index, 0);
    }
}
