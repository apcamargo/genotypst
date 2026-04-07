//! genome-map: Leader routing backend for genome maps.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[cfg(target_arch = "wasm32")]
use wasm_minimal_protocol::*;

#[cfg(target_arch = "wasm32")]
initiate_protocol!();

const X_EPSILON_PT: f64 = 1e-9;
const Y_EPSILON_PT: f64 = 1e-9;

#[derive(Debug, Clone, Deserialize)]
struct RoutingRequest {
    labels: Vec<LabelInput>,
    line_bottom_pt: f64,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
struct RoutingResponse {
    labels: Vec<LabelOutput>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct LabelOutput {
    leader_segments: Vec<LeaderSegment>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
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

fn validate_request(request: &RoutingRequest) -> Result<(), String> {
    if !request.line_bottom_pt.is_finite() {
        return Err("line_bottom_pt must be finite".into());
    }

    let label_count = request.labels.len();
    for (index, label) in request.labels.iter().enumerate() {
        if label.level >= label_count {
            return Err(format!(
                "labels[{index}].level must be less than the number of labels"
            ));
        }

        for (name, value) in [
            ("hit_left_pt", label.hit_left_pt),
            ("hit_right_pt", label.hit_right_pt),
            ("query_x_pt", label.query_x_pt),
            ("line_top_pt", label.line_top_pt),
            ("raw_top_pt", label.raw_top_pt),
            ("raw_bottom_pt", label.raw_bottom_pt),
            ("block_top_pt", label.block_top_pt),
            ("block_bottom_pt", label.block_bottom_pt),
        ] {
            if !value.is_finite() {
                return Err(format!("labels[{index}].{name} must be finite"));
            }
        }

        if label.hit_right_pt + X_EPSILON_PT < label.hit_left_pt {
            return Err(format!(
                "labels[{index}].hit_right_pt must be >= hit_left_pt"
            ));
        }
        if label.raw_bottom_pt + Y_EPSILON_PT < label.raw_top_pt {
            return Err(format!(
                "labels[{index}].raw_bottom_pt must be >= raw_top_pt"
            ));
        }
        if label.block_bottom_pt + Y_EPSILON_PT < label.block_top_pt {
            return Err(format!(
                "labels[{index}].block_bottom_pt must be >= block_top_pt"
            ));
        }
    }

    Ok(())
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
    label: &LabelInput,
    line_bottom_pt: f64,
    active_levels: &BTreeSet<usize>,
    level_geometries: &[Option<LevelGeometry>],
) -> Vec<LeaderSegment> {
    let blocked_intervals = active_levels
        .range(..label.level)
        .rev()
        .filter_map(|level| {
            let geometry = level_geometries[*level]?;
            if !raw_box_overlaps_line_span(
                geometry.raw_top_pt,
                geometry.raw_bottom_pt,
                label.line_top_pt,
                line_bottom_pt,
            ) {
                return None;
            }
            clip_blocked_interval(
                geometry.block_top_pt,
                geometry.block_bottom_pt,
                label.line_top_pt,
                line_bottom_pt,
            )
        });

    visible_segments_from_blocked_intervals(label.line_top_pt, line_bottom_pt, blocked_intervals)
}

fn validate_or_insert_level_geometry(
    level_geometries: &mut [Option<LevelGeometry>],
    label_index: usize,
    level: usize,
    geometry: LevelGeometry,
) -> Result<(), String> {
    match level_geometries[level] {
        Some(existing_geometry) => {
            if (existing_geometry.raw_top_pt - geometry.raw_top_pt).abs() > Y_EPSILON_PT {
                return Err(format!(
                    "labels[{label_index}].raw_top_pt conflicts with level {}",
                    level
                ));
            }
            if (existing_geometry.raw_bottom_pt - geometry.raw_bottom_pt).abs() > Y_EPSILON_PT {
                return Err(format!(
                    "labels[{label_index}].raw_bottom_pt conflicts with level {}",
                    level
                ));
            }
            if (existing_geometry.block_top_pt - geometry.block_top_pt).abs() > Y_EPSILON_PT {
                return Err(format!(
                    "labels[{label_index}].block_top_pt conflicts with level {}",
                    level
                ));
            }
            if (existing_geometry.block_bottom_pt - geometry.block_bottom_pt).abs() > Y_EPSILON_PT {
                return Err(format!(
                    "labels[{label_index}].block_bottom_pt conflicts with level {}",
                    level
                ));
            }
        }
        None => level_geometries[level] = Some(geometry),
    }

    Ok(())
}

// Sweep across precomputed label hit intervals and scan the exact active lower
// levels for each leader query.
fn compute_leader_segments(request: &RoutingRequest) -> Result<Vec<Vec<LeaderSegment>>, String> {
    if request.labels.is_empty() {
        return Ok(Vec::new());
    }

    let level_count = request
        .labels
        .iter()
        .map(|label| label.level)
        .max()
        .expect("non-empty request")
        + 1;
    let mut level_geometries = vec![None::<LevelGeometry>; level_count];
    for (label_index, label) in request.labels.iter().enumerate() {
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
    }

    let mut events = Vec::with_capacity(request.labels.len() * 3);
    for (label_index, label) in request.labels.iter().enumerate() {
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

    events.sort_by(|a, b| {
        a.x_pt
            .total_cmp(&b.x_pt)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.label_index.cmp(&b.label_index))
    });

    let mut segments_by_label = vec![Vec::<LeaderSegment>::new(); request.labels.len()];
    let mut active_counts = vec![0usize; level_count];
    let mut active_levels = BTreeSet::<usize>::new();
    for event in events {
        match event.kind {
            EventKind::Start => {
                let level = request.labels[event.label_index].level;
                if active_counts[level] == 0 {
                    active_levels.insert(level);
                }
                active_counts[level] += 1;
            }
            EventKind::Query => {
                let label = &request.labels[event.label_index];
                segments_by_label[event.label_index] = visible_segments_for_levels(
                    label,
                    request.line_bottom_pt,
                    &active_levels,
                    &level_geometries,
                );
            }
            EventKind::End => {
                let level = request.labels[event.label_index].level;
                active_counts[level] -= 1;
                if active_counts[level] == 0 {
                    active_levels.remove(&level);
                }
            }
        }
    }

    Ok(segments_by_label)
}

fn compute_routes(request: RoutingRequest) -> Result<RoutingResponse, String> {
    validate_request(&request)?;

    let labels = compute_leader_segments(&request)?
        .into_iter()
        .map(|leader_segments| LabelOutput { leader_segments })
        .collect();

    Ok(RoutingResponse { labels })
}

#[cfg_attr(target_arch = "wasm32", wasm_func)]
/// Routes genome-map label leaders from Typst-positioned pt-based geometry.
///
/// The caller is responsible for text measurement, dodge-level assignment, and
/// final row positioning. The backend returns only visible leader segments.
pub fn route_leaders(config: &[u8]) -> Result<Vec<u8>, String> {
    let request: RoutingRequest =
        serde_json::from_slice(config).map_err(|e| format!("Invalid config JSON: {e}"))?;
    let response = compute_routes(request)?;
    serde_json::to_vec(&response).map_err(|e| format!("Serialization failed: {e}"))
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

    fn label_top(level_count: usize, label_vertical_gap_pt: f64, level: usize) -> f64 {
        let block_height_pt = level_block_height(level_count, label_vertical_gap_pt);
        let level_step_pt = LABEL_HEIGHT_PT + label_vertical_gap_pt;
        let level_base_top_pt = block_height_pt - LABEL_HEIGHT_PT;
        level_base_top_pt - level as f64 * level_step_pt
    }

    fn label(
        level_count: usize,
        label_vertical_gap_pt: f64,
        level: usize,
        left_pt: f64,
        right_pt: f64,
        query_x_pt: f64,
    ) -> LabelInput {
        let top_pt = label_top(level_count, label_vertical_gap_pt, level);
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

    fn request_with_spacing(
        level_count: usize,
        label_vertical_gap_pt: f64,
        label_track_gap_pt: f64,
        label_leader_offset_pt: f64,
        labels: Vec<LabelInput>,
    ) -> RoutingRequest {
        RoutingRequest {
            labels,
            line_bottom_pt: level_block_height(level_count, label_vertical_gap_pt)
                + label_track_gap_pt
                - label_leader_offset_pt,
        }
    }

    fn request(
        level_count: usize,
        label_vertical_gap_pt: f64,
        labels: Vec<LabelInput>,
    ) -> RoutingRequest {
        request_with_spacing(
            level_count,
            label_vertical_gap_pt,
            LABEL_TRACK_GAP_PT,
            LABEL_LEADER_OFFSET_PT,
            labels,
        )
    }

    #[test]
    fn lower_level_blocker_splits_visible_leader_segments() {
        let response = compute_routes(request(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 20.0, 40.0, 30.0),
                label(2, 4.0, 1, 24.0, 44.0, 30.0),
            ],
        ))
        .unwrap();

        assert_segments(&response.labels[1].leader_segments, &[(11.4, 0.1)]);
    }

    #[test]
    fn non_blocking_lower_level_keeps_full_leader() {
        let response = compute_routes(request(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 0.0, 8.0, 4.0),
                label(2, 4.0, 1, 10.0, 28.0, 34.0),
            ],
        ))
        .unwrap();

        assert_segments(&response.labels[1].leader_segments, &[(11.4, 14.6)]);
    }

    #[test]
    fn clearance_only_overlap_keeps_full_leader() {
        let response = compute_routes(request_with_spacing(
            2,
            4.0,
            1.0,
            12.0,
            vec![
                label(2, 4.0, 0, 20.0, 40.0, 30.0),
                label(2, 4.0, 1, 24.0, 44.0, 30.0),
            ],
        ))
        .unwrap();

        assert_segments(&response.labels[1].leader_segments, &[(11.4, 1.6)]);
    }

    #[test]
    fn sparse_lower_levels_still_block_leaders() {
        let response = compute_routes(request(
            3,
            4.0,
            vec![
                label(3, 4.0, 0, 20.0, 40.0, 30.0),
                label(3, 4.0, 2, 24.0, 44.0, 30.0),
                label(3, 4.0, 2, 0.0, 8.0, 4.0),
            ],
        ))
        .unwrap();

        assert_segments(&response.labels[1].leader_segments, &[(11.4, 14.1)]);
    }

    #[test]
    fn blocker_starting_at_query_x_is_active_for_routing() {
        let response = compute_routes(request(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 11.0, 31.0, 21.0),
                label(2, 4.0, 1, 0.0, 20.0, 10.0),
            ],
        ))
        .unwrap();

        assert_segments(&response.labels[1].leader_segments, &[(11.4, 0.1)]);
    }

    #[test]
    fn same_level_multiplicity_keeps_blocker_active_until_last_label_ends() {
        let response = compute_routes(request(
            2,
            4.0,
            vec![
                label(2, 4.0, 0, 0.0, 9.0, 4.0),
                label(2, 4.0, 0, 0.0, 20.0, 10.0),
                label(2, 4.0, 1, 20.0, 40.0, 10.5),
            ],
        ))
        .unwrap();

        assert_segments(&response.labels[2].leader_segments, &[(11.4, 0.1)]);
    }

    fn same_level_conflict_error(mutator: impl FnOnce(&mut LabelInput)) -> String {
        let first = label(1, 4.0, 0, 0.0, 20.0, 10.0);
        let mut second = label(1, 4.0, 0, 30.0, 50.0, 40.0);
        mutator(&mut second);

        compute_routes(request(1, 4.0, vec![first, second])).unwrap_err()
    }

    #[test]
    fn conflicting_raw_top_on_same_level_errors() {
        let error = same_level_conflict_error(|label| label.raw_top_pt += 0.2);
        assert!(error.contains("raw_top_pt conflicts with level 0"));
    }

    #[test]
    fn conflicting_raw_bottom_on_same_level_errors() {
        let error = same_level_conflict_error(|label| label.raw_bottom_pt += 0.2);
        assert!(error.contains("raw_bottom_pt conflicts with level 0"));
    }

    #[test]
    fn conflicting_block_top_on_same_level_errors() {
        let error = same_level_conflict_error(|label| label.block_top_pt += 0.2);
        assert!(error.contains("block_top_pt conflicts with level 0"));
    }

    #[test]
    fn conflicting_block_bottom_on_same_level_errors() {
        let error = same_level_conflict_error(|label| label.block_bottom_pt += 0.2);
        assert!(error.contains("block_bottom_pt conflicts with level 0"));
    }

    #[test]
    fn multiple_active_levels_merge_when_clearance_overlaps() {
        let response = compute_routes(request(
            3,
            1.0,
            vec![
                label(3, 1.0, 0, 20.0, 40.0, 30.0),
                label(3, 1.0, 1, 20.0, 40.0, 30.0),
                label(3, 1.0, 2, 20.0, 40.0, 30.0),
            ],
        ))
        .unwrap();

        assert!(response.labels[2].leader_segments.is_empty());
    }

    #[test]
    fn empty_request_returns_empty_routes() {
        let response = compute_routes(RoutingRequest {
            labels: Vec::new(),
            line_bottom_pt: 0.0,
        })
        .unwrap();
        assert_eq!(response, RoutingResponse { labels: Vec::new() });
    }

    #[test]
    fn route_leaders_rejects_extreme_level_before_allocation() {
        let config = format!(
            concat!(
                r#"{{"line_bottom_pt":1.0,"labels":[{{"#,
                r#""level":{},"hit_left_pt":0.0,"hit_right_pt":1.0,"query_x_pt":0.5,"#,
                r#""line_top_pt":0.0,"raw_top_pt":0.0,"raw_bottom_pt":1.0,"#,
                r#""block_top_pt":0.0,"block_bottom_pt":1.0"#,
                r#"}}]}}"#
            ),
            usize::MAX
        );

        let error = route_leaders(config.as_bytes()).unwrap_err();
        assert!(error.contains("labels[0].level must be less than the number of labels"));
    }
}
