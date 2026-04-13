//! tree: Phylogenetic tree layout and fitting backend for the Typst package.

use newick::{NewickTree, one_from_string};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use wasm_minimal_protocol::*;

initiate_protocol!();

const FIT_TOLERANCE_PT: f64 = 0.1;
const FIT_ACCEPTANCE_TOLERANCE_PT: f64 = 0.2;
const ROTATION_SCORE_TOLERANCE: f64 = 1e-9;
const DEGENERATE_TOLERANCE: f64 = 1e-12;

/// JSON result returned by the `parse_newick` WASM entry point.
#[derive(Serialize)]
struct ParseResult {
    rooted: bool,
    #[serde(flatten)]
    tree: SimpleTreeNode,
}

/// Simplified tree node serialized for Typst-side Newick parsing.
#[derive(Serialize)]
struct SimpleTreeNode {
    name: Option<String>,
    length: Option<f64>,
    children: Option<Vec<SimpleTreeNode>>,
}

#[derive(Debug, Clone)]
struct RawTreeNode {
    name: Option<String>,
    label_id: Option<String>,
    length: Option<f64>,
    children: Vec<RawTreeNode>,
    rooted: bool,
}

#[derive(Debug, Clone, Serialize)]
struct LayoutNodeWire {
    #[serde(rename = "parent-id")]
    parent_id: Option<usize>,
    #[serde(rename = "children-ids")]
    children_ids: Vec<usize>,
    #[serde(rename = "is-root")]
    is_root: bool,
    #[serde(rename = "input-rooted")]
    input_rooted: bool,
    #[serde(rename = "is-leaf")]
    is_leaf: bool,
    #[serde(rename = "label-text")]
    label_text: Option<String>,
    #[serde(rename = "label-id")]
    label_id: Option<String>,
    #[serde(rename = "x-unit")]
    x_unit: f64,
    #[serde(rename = "y-unit")]
    y_unit: f64,
    #[serde(rename = "branch-angle")]
    branch_angle: f64,
}

/// Typst-facing layout fit modes returned in prepared layout metadata.
#[derive(Debug, Clone, Copy, Serialize)]
enum LayoutFitModeWire {
    #[serde(rename = "independent-axes")]
    IndependentAxes,
    #[serde(rename = "uniform")]
    Uniform,
}

/// Typst-facing primitive mode tags used by prepared layout responses.
#[derive(Debug, Clone, Copy, Serialize)]
enum PrimitiveModeWire {
    #[serde(rename = "rectangular")]
    Rectangular,
    #[serde(rename = "edge-segments")]
    EdgeSegments,
}

/// Serialized layout tree returned by the `prepare_layout` WASM entry point.
#[derive(Debug, Clone, Serialize)]
struct LayoutTreeWire {
    nodes: Vec<LayoutNodeWire>,
    #[serde(rename = "root-id")]
    root_id: usize,
    #[serde(rename = "node-count")]
    node_count: usize,
    #[serde(rename = "effective-cladogram")]
    effective_cladogram: bool,
    #[serde(rename = "layout-kind")]
    layout_kind: LayoutKind,
    #[serde(rename = "fit-mode")]
    fit_mode: LayoutFitModeWire,
    #[serde(rename = "primitive-mode")]
    primitive_mode: PrimitiveModeWire,
    #[serde(rename = "tree-depth")]
    tree_depth: f64,
    #[serde(rename = "tree-height")]
    tree_height: f64,
}

/// JSON request accepted by the `prepare_layout` WASM entry point.
///
/// `layout-kind` uses the Rust wire vocabulary: `"rectangular"`,
/// `"equal_angle"`, or `"daylight"`.
#[derive(Debug, Deserialize)]
struct PrepareLayoutRequest {
    #[serde(rename = "tree-data")]
    tree_data: Value,
    cladogram: bool,
    #[serde(rename = "suppress-unrooted")]
    suppress_unrooted: bool,
    #[serde(rename = "hide-internal-labels")]
    hide_internal_labels: bool,
    #[serde(rename = "layout-kind")]
    layout_kind: LayoutKind,
}

#[derive(Debug, Clone)]
struct InternalNode {
    id: usize,
    parent_id: Option<usize>,
    children_ids: Vec<usize>,
    subtree_end_id: usize,
    is_root: bool,
    input_rooted: bool,
    is_leaf: bool,
    label_text: Option<String>,
    label_id: Option<String>,
    length: Option<f64>,
    resolved_length: f64,
}

#[derive(Debug, Clone)]
struct NormalizedTreeData {
    nodes: Vec<InternalNode>,
    root_id: usize,
    node_count: usize,
    effective_cladogram: bool,
}

#[derive(Debug, Clone)]
struct LayoutTreeData {
    normalized: NormalizedTreeData,
    x_by_id: Vec<f64>,
    y_by_id: Vec<f64>,
    branch_angles: Vec<f64>,
    layout_kind: LayoutKind,
    fit_mode: LayoutFitModeWire,
    primitive_mode: PrimitiveModeWire,
    tree_depth: f64,
    tree_height: f64,
}

#[derive(Debug, Clone, Copy)]
struct Interval {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy)]
struct IntervalSet {
    first: Interval,
    second: Option<Interval>,
}

impl IntervalSet {
    fn single(start: usize, end: usize) -> Self {
        Self {
            first: Interval { start, end },
            second: None,
        }
    }

    fn with_second(first: Interval, second: Option<Interval>) -> Self {
        Self { first, second }
    }

    fn contains(&self, value: usize) -> bool {
        (self.first.start <= value && value <= self.first.end)
            || self
                .second
                .is_some_and(|interval| interval.start <= value && value <= interval.end)
    }
}

#[derive(Debug, Clone, Copy)]
struct DaylightParentEntry {
    parent_id: usize,
    child_start: usize,
    child_len: usize,
}

#[derive(Debug, Clone, Copy)]
struct DaylightSpan {
    start: usize,
    len: usize,
}

#[derive(Debug, Clone, Copy)]
struct DaylightComponent {
    node_id: usize,
    intervals: IntervalSet,
    parent_entries: DaylightSpan,
}

#[derive(Debug, Clone)]
struct DaylightCache {
    active_internal_ids: Vec<usize>,
    internal_count: usize,
    max_component_count: usize,
    component_spans_by_id: Vec<DaylightSpan>,
    components: Vec<DaylightComponent>,
    parent_entries: Vec<DaylightParentEntry>,
    child_ids: Vec<usize>,
}

impl DaylightCache {
    fn component_span(&self, pivot_id: usize) -> DaylightSpan {
        self.component_spans_by_id[pivot_id]
    }

    fn components_at(&self, pivot_id: usize) -> &[DaylightComponent] {
        let span = self.component_span(pivot_id);
        &self.components[span.start..span.start + span.len]
    }

    fn parent_entries_for(&self, component: DaylightComponent) -> &[DaylightParentEntry] {
        let span = component.parent_entries;
        &self.parent_entries[span.start..span.start + span.len]
    }

    fn child_ids_for(&self, parent_entry: DaylightParentEntry) -> &[usize] {
        &self.child_ids[parent_entry.child_start..parent_entry.child_start + parent_entry.child_len]
    }
}

struct DaylightCacheBuilder<'a> {
    normalized: &'a NormalizedTreeData,
    components: Vec<DaylightComponent>,
    parent_entries: Vec<DaylightParentEntry>,
    child_ids: Vec<usize>,
}

impl<'a> DaylightCacheBuilder<'a> {
    fn new(normalized: &'a NormalizedTreeData) -> Self {
        Self {
            normalized,
            components: Vec::new(),
            parent_entries: Vec::new(),
            child_ids: Vec::new(),
        }
    }

    fn build_component(
        &mut self,
        pivot_id: usize,
        node_id: usize,
        intervals: IntervalSet,
    ) -> DaylightComponent {
        let parent_start = self.parent_entries.len();

        let push_interval = |interval: Interval, builder: &mut Self| {
            for parent_id in interval.start..=interval.end {
                if parent_id == pivot_id {
                    continue;
                }
                let parent = &builder.normalized.nodes[parent_id];
                if parent.is_leaf {
                    continue;
                }

                let child_start = builder.child_ids.len();
                for &child_id in &parent.children_ids {
                    if child_id != pivot_id && intervals.contains(child_id) {
                        builder.child_ids.push(child_id);
                    }
                }

                let child_len = builder.child_ids.len() - child_start;
                if child_len > 0 {
                    builder.parent_entries.push(DaylightParentEntry {
                        parent_id,
                        child_start,
                        child_len,
                    });
                }
            }
        };

        push_interval(intervals.first, self);
        if let Some(interval) = intervals.second {
            push_interval(interval, self);
        }

        DaylightComponent {
            node_id,
            intervals,
            parent_entries: DaylightSpan {
                start: parent_start,
                len: self.parent_entries.len() - parent_start,
            },
        }
    }

    fn push_component(&mut self, pivot_id: usize, node_id: usize, intervals: IntervalSet) {
        let component = self.build_component(pivot_id, node_id, intervals);
        self.components.push(component);
    }

    fn into_cache(
        self,
        active_internal_ids: Vec<usize>,
        internal_count: usize,
        max_component_count: usize,
        component_spans_by_id: Vec<DaylightSpan>,
    ) -> DaylightCache {
        DaylightCache {
            active_internal_ids,
            internal_count,
            max_component_count,
            component_spans_by_id,
            components: self.components,
            parent_entries: self.parent_entries,
            child_ids: self.child_ids,
        }
    }
}

#[derive(Debug, Clone)]
struct EqualAngleState {
    x_by_id: Vec<f64>,
    y_by_id: Vec<f64>,
    branch_angles: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
struct LayoutBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[derive(Debug, Clone, Copy)]
struct LayoutSummary {
    layout_kind: LayoutKind,
    fit_mode: LayoutFitModeWire,
    primitive_mode: PrimitiveModeWire,
    bounds: LayoutBounds,
    tree_depth: Option<f64>,
    tree_height: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct DaylightArc {
    left: f64,
    right: f64,
}

#[derive(Debug, Clone, Copy)]
struct ComponentArcEntry {
    left: f64,
    beta: f64,
    component_index: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FitMode {
    IndependentAxes,
    Uniform,
}

/// Tree layout kind used on the Rust JSON wire boundary.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LayoutKind {
    Rectangular,
    EqualAngle,
    Daylight,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WidthMode {
    Auto,
    Resolved,
    Provisional,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HeightMode {
    Auto,
    Resolved,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PlacementFrame {
    Screen,
    Local,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum XAlign {
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum YAlign {
    Top,
    Bottom,
    Center,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct Anchor {
    tree: Point,
    page: Point,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct PreparedLine {
    start_anchor: Anchor,
    end_anchor: Anchor,
    half_stroke_pt: f64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct PreparedLabel {
    anchor_tree: Point,
    anchor_page: Point,
    x_align: XAlign,
    y_align: YAlign,
    x_gap_pt: f64,
    y_gap_pt: f64,
    rotation_deg: f64,
    placement_frame: PlacementFrame,
    branch_angle_half_turn: Option<f64>,
    placement_angle_half_turn: Option<f64>,
    measure_width_pt: f64,
    measure_height_pt: f64,
}

/// JSON request accepted by the `fit_tree` WASM entry point.
#[derive(Debug, Deserialize)]
struct FitRequest {
    fit_mode: FitMode,
    layout_kind: LayoutKind,
    orientation: Orientation,
    prepared_lines: Vec<PreparedLine>,
    prepared_labels: Vec<PreparedLabel>,
    root_tree_point: Point,
    tree_depth: f64,
    tree_height: f64,
    width_mode: WidthMode,
    viewport_width_pt: Option<f64>,
    height_mode: HeightMode,
    viewport_height_pt: Option<f64>,
    auto_height_floor_pt: f64,
    fit_band_samples: Option<usize>,
    fit_max_bands: usize,
    optimize_uniform_rotation: bool,
}

#[derive(Debug, Clone)]
struct FitInputs {
    prepared_lines: Vec<PreparedLine>,
    prepared_labels: Vec<PreparedLabel>,
    root_tree_point: Point,
    tree_depth: f64,
    tree_height: f64,
}

#[derive(Debug, Clone, Copy)]
struct Formula {
    coeff: f64,
    offset: f64,
}

#[derive(Debug, Clone, Copy)]
struct FormulaPoint {
    x: Formula,
    y: Formula,
}

#[derive(Debug, Clone, Copy)]
struct OrderedFormulaInterval {
    min: Formula,
    max: Formula,
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    width: f64,
    height: f64,
}

#[derive(Default)]
struct BoundsAccumulator {
    min_x: Option<f64>,
    min_y: Option<f64>,
    max_x: Option<f64>,
    max_y: Option<f64>,
}

#[derive(Debug, Clone)]
struct UniformAxisDescriptor {
    min_coeff: f64,
    min_offset: f64,
    max_coeff: f64,
    max_offset: f64,
    activation_threshold: Option<f64>,
}

#[derive(Debug, Clone)]
struct UniformBoundsDescriptors {
    x: Vec<UniformAxisDescriptor>,
    y: Vec<UniformAxisDescriptor>,
}

impl UniformBoundsDescriptors {
    fn as_descriptors(&self) -> UniformBoundsDescriptorsRef<'_> {
        UniformBoundsDescriptorsRef {
            x: &self.x,
            y: &self.y,
        }
    }
}

#[derive(Clone, Copy)]
struct UniformBoundsDescriptorsRef<'a> {
    x: &'a [UniformAxisDescriptor],
    y: &'a [UniformAxisDescriptor],
}

#[derive(Debug, Clone, Copy)]
struct SolveDescriptor {
    min_coeff: f64,
    min_offset: f64,
    max_coeff: f64,
    max_offset: f64,
}

#[derive(Debug, Clone)]
struct SolveDescriptors {
    depth: Vec<SolveDescriptor>,
    spread: Vec<SolveDescriptor>,
}

#[derive(Debug, Clone)]
struct MaterializedLine {
    line_index: usize,
    start: Point,
    end: Point,
}

#[derive(Debug, Clone)]
struct MaterializedLabel {
    label_index: usize,
    origin: Point,
    rotation_deg: f64,
}

#[derive(Debug, Clone)]
struct MaterializedTree {
    tree_lines: Vec<MaterializedLine>,
    tree_labels: Vec<MaterializedLabel>,
    root_position: Point,
    tree_occupied_bounds: Bounds,
}

#[derive(Debug, Clone, Copy)]
struct EvaluatedFit {
    viewport_width: f64,
    viewport_height: f64,
    x_scale: f64,
    y_scale: f64,
    tree_occupied_bounds: Bounds,
}

#[derive(Debug, Clone)]
struct FittedWidth {
    width_unresolved: bool,
    viewport_width: f64,
    viewport_height: f64,
    x_scale: f64,
    materialized_tree: MaterializedTree,
}

#[derive(Debug, Clone, Copy)]
struct UniformViewport {
    width_unresolved: bool,
    viewport_width: f64,
    viewport_height: f64,
}

#[derive(Debug, Clone, Copy)]
enum UniformViewportPolicy {
    AutoWidth {
        viewport_height: f64,
    },
    ResolvedWidthAutoHeight {
        viewport_width: f64,
    },
    Constrained {
        width_unresolved: bool,
        viewport_width: f64,
        viewport_height: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisKind {
    Depth,
    Spread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RotationObjective {
    None,
    CompactWidth,
    CompactHeight,
    Area,
}

#[derive(Debug, Clone)]
struct RotationCandidate {
    rotation_half_turn: f64,
    evaluated_fit: EvaluatedFit,
}

#[derive(Debug, Clone, Copy)]
struct RotationTransform {
    rotation_half_turn: f64,
    cos_theta: f64,
    sin_theta: f64,
}

/// JSON response returned by the `fit_tree` WASM entry point.
#[derive(Serialize)]
struct FitResponse {
    width_unresolved: bool,
    tree_viewport_width_pt: f64,
    tree_viewport_height_pt: f64,
    x_scale_pt: f64,
    tree_translation_pt: Point,
    root_position_pt: Point,
    tree_lines: Vec<SerializableLine>,
    tree_labels: Vec<SerializableLabel>,
}

#[derive(Serialize)]
struct SerializableLine {
    line_index: usize,
    start_pt: Point,
    end_pt: Point,
}

#[derive(Serialize)]
struct SerializableLabel {
    label_index: usize,
    origin_pt: Point,
    rotation_deg: f64,
}

#[derive(Debug, Clone, Copy)]
struct RadialPlacement {
    rotation_deg: f64,
    x_align: XAlign,
    y_align: YAlign,
    gap_sign: f64,
    branch_angle_half_turn: f64,
}

#[derive(Debug, Clone, Copy)]
struct HorizontalPlacement {
    x_align: XAlign,
    y_align: YAlign,
    placement_angle_half_turn: f64,
}

fn normalize_label(raw: &str) -> String {
    if let Some(inner) = raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        raw.to_owned()
    }
}

fn convert_node_to_simple(tree: &NewickTree, node_id: usize) -> Result<SimpleTreeNode, String> {
    let node = tree
        .get(node_id)
        .map_err(|e| format!("Failed to get node {}: {:?}", node_id, e))?;

    let children_ids = node.children();
    let children = if children_ids.is_empty() {
        None
    } else {
        Some(
            children_ids
                .iter()
                .map(|&child_id| convert_node_to_simple(tree, child_id))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };

    let name = node.data().name.as_deref().map(normalize_label);
    let length = node.branch().map(|&l| l as f64);

    Ok(SimpleTreeNode {
        name,
        length,
        children,
    })
}

fn parse_raw_tree(value: &Value, is_root: bool) -> Result<RawTreeNode, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "tree nodes must be dictionaries.".to_string())?;

    let children_value = object
        .get("children")
        .ok_or_else(|| "tree nodes must define children.".to_string())?;

    let name = match object.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => return Err("node name must be a string or none.".into()),
    };
    let label_id = match object.get("label-id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => return Err("node label-id must be a string or none.".into()),
    };

    let length = match object.get("length") {
        None | Some(Value::Null) => None,
        Some(Value::Number(value)) => {
            let Some(length) = value.as_f64() else {
                return Err("node length must be a number or none.".into());
            };
            if !length.is_finite() {
                return Err("node length must be a finite number or none.".into());
            }
            if length < 0.0 {
                return Err("node length must be non-negative.".into());
            }
            Some(length)
        }
        Some(_) => return Err("node length must be a number or none.".into()),
    };

    let rooted = if is_root {
        match object.get("rooted") {
            None => false,
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err("rooted must be a boolean.".into()),
        }
    } else {
        false
    };

    let children = match children_value {
        Value::Null => Vec::new(),
        Value::Array(values) => values
            .iter()
            .map(|child| parse_raw_tree(child, false))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("children must be an array or none.".into()),
    };

    Ok(RawTreeNode {
        name,
        label_id,
        length,
        children,
        rooted,
    })
}

fn merge_unrooted_root_branch_lengths(first: Option<f64>, second: Option<f64>) -> Option<f64> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first + second),
        (Some(first), None) => Some(first),
        (None, Some(second)) => Some(second),
        (None, None) => None,
    }
}

fn suppression_subtree_tip_count(node: &RawTreeNode) -> usize {
    if node.children.is_empty() {
        1
    } else {
        node.children
            .iter()
            .map(suppression_subtree_tip_count)
            .sum()
    }
}

fn suppression_subtree_node_count(node: &RawTreeNode) -> usize {
    1 + node
        .children
        .iter()
        .map(suppression_subtree_node_count)
        .sum::<usize>()
}

fn compare_unrooted_suppression_candidates(first: &RawTreeNode, second: &RawTreeNode) -> Ordering {
    let tip_cmp = suppression_subtree_tip_count(second).cmp(&suppression_subtree_tip_count(first));
    if !tip_cmp.is_eq() {
        return tip_cmp;
    }

    let node_cmp =
        suppression_subtree_node_count(second).cmp(&suppression_subtree_node_count(first));
    if !node_cmp.is_eq() {
        return node_cmp;
    }

    let label_cmp = first
        .name
        .as_deref()
        .filter(|value| !value.is_empty())
        .cmp(&second.name.as_deref().filter(|value| !value.is_empty()));
    if !label_cmp.is_eq() {
        return label_cmp;
    }

    let length_cmp = match (first.length, second.length) {
        (Some(first), Some(second)) => first.total_cmp(&second),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    if !length_cmp.is_eq() {
        return length_cmp;
    }

    let child_count_cmp = second.children.len().cmp(&first.children.len());
    if !child_count_cmp.is_eq() {
        return child_count_cmp;
    }

    for (first_child, second_child) in first.children.iter().zip(&second.children) {
        let child_cmp = compare_unrooted_suppression_candidates(first_child, second_child);
        if !child_cmp.is_eq() {
            return child_cmp;
        }
    }

    Ordering::Equal
}

fn suppress_unrooted_artificial_root(mut node: RawTreeNode) -> RawTreeNode {
    if !node.rooted || node.children.len() != 2 {
        return node;
    }

    let first_is_leaf = node.children[0].children.is_empty();
    let second_is_leaf = node.children[1].children.is_empty();
    if first_is_leaf && second_is_leaf {
        node.rooted = false;
        node.name = None;
        node.label_id = None;
        node.length = None;
        return node;
    }

    let promoted_index = if !first_is_leaf && second_is_leaf {
        0
    } else if first_is_leaf && !second_is_leaf {
        1
    } else {
        // Hidden binary roots should not bias unrooted output based on which
        // side happened to appear first in the serialized Newick input.
        let first = &node.children[0];
        let second = &node.children[1];
        if compare_unrooted_suppression_candidates(first, second).is_gt() {
            1
        } else {
            0
        }
    };
    let promoted_child = node.children.remove(promoted_index);
    let sibling_child = node.children.remove(0);

    let mut promoted_root = promoted_child;
    let merged_sibling = RawTreeNode {
        length: merge_unrooted_root_branch_lengths(promoted_root.length, sibling_child.length),
        ..sibling_child
    };

    promoted_root.rooted = false;
    promoted_root.length = None;
    promoted_root.children.push(merged_sibling);
    promoted_root
}

fn normalize_tree_node(
    node: &RawTreeNode,
    nodes: &mut Vec<InternalNode>,
    next_id: &mut usize,
    parent_id: Option<usize>,
    is_root: bool,
    hide_internal_labels: bool,
) -> (usize, bool) {
    let id = *next_id;
    *next_id += 1;
    let hide_label = hide_internal_labels && !node.children.is_empty();
    let label_text = if hide_label {
        None
    } else {
        node.name.clone().filter(|value| !value.is_empty())
    };
    let label_id = if hide_label {
        None
    } else {
        node.label_id.clone().filter(|value| !value.is_empty())
    };
    nodes.push(InternalNode {
        id,
        parent_id,
        children_ids: Vec::new(),
        subtree_end_id: id,
        is_root,
        input_rooted: if is_root { node.rooted } else { false },
        is_leaf: node.children.is_empty(),
        label_text,
        label_id,
        length: node.length,
        resolved_length: 0.0,
    });

    let mut child_ids = Vec::with_capacity(node.children.len());
    let mut has_explicit_non_root_length = !is_root && node.length.is_some();
    for child in &node.children {
        let (child_id, child_has_explicit_non_root_length) =
            normalize_tree_node(child, nodes, next_id, Some(id), false, hide_internal_labels);
        child_ids.push(child_id);
        has_explicit_non_root_length =
            has_explicit_non_root_length || child_has_explicit_non_root_length;
    }

    let subtree_end_id = *next_id - 1;
    let node_slot = &mut nodes[id];
    node_slot.children_ids = child_ids;
    node_slot.subtree_end_id = subtree_end_id;
    node_slot.is_leaf = node.children.is_empty();
    (id, has_explicit_non_root_length)
}

fn normalize_raw_tree(
    tree_data: Value,
    cladogram: bool,
    suppress_unrooted: bool,
    hide_internal_labels: bool,
) -> Result<NormalizedTreeData, String> {
    let raw_tree = parse_raw_tree(&tree_data, true)?;
    let raw_tree = if suppress_unrooted {
        suppress_unrooted_artificial_root(raw_tree)
    } else {
        raw_tree
    };

    let mut nodes = Vec::new();
    let mut next_id = 0;
    let (root_id, has_explicit_non_root_length) = normalize_tree_node(
        &raw_tree,
        &mut nodes,
        &mut next_id,
        None,
        true,
        hide_internal_labels,
    );
    let node_count = next_id;
    let effective_cladogram = cladogram || !has_explicit_non_root_length;

    for node in &mut nodes {
        node.resolved_length = if effective_cladogram {
            if node.is_root { 0.0 } else { 1.0 }
        } else {
            node.length.unwrap_or(0.0)
        };
    }

    Ok(NormalizedTreeData {
        nodes,
        root_id,
        node_count,
        effective_cladogram,
    })
}

fn half_turn_to_radians(value: f64) -> f64 {
    value * std::f64::consts::PI
}

fn normalize_half_turn(value: f64) -> f64 {
    let wrapped = value.rem_euclid(2.0);
    if approx_eq(wrapped, 2.0, 1e-9) {
        0.0
    } else {
        wrapped
    }
}

fn signed_half_turn_delta(delta: f64) -> f64 {
    if delta > 1.0 {
        delta - 2.0
    } else if delta < -1.0 {
        delta + 2.0
    } else {
        delta
    }
}

fn half_turn_arc_width(left: f64, right: f64) -> f64 {
    let width = left - right;
    if width < 0.0 { width + 2.0 } else { width }
}

fn node_angle_half_turn(from_x: f64, from_y: f64, to_x: f64, to_y: f64, default_angle: f64) -> f64 {
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    if dx.abs() <= DEGENERATE_TOLERANCE && dy.abs() <= DEGENERATE_TOLERANCE {
        default_angle
    } else {
        dy.atan2(dx) / std::f64::consts::PI
    }
}

fn tree_layout_bounds(x_by_id: &[f64], y_by_id: &[f64]) -> LayoutBounds {
    let mut min_x = None::<f64>;
    let mut max_x = None::<f64>;
    let mut min_y = None::<f64>;
    let mut max_y = None::<f64>;

    for (&x, &y) in x_by_id.iter().zip(y_by_id.iter()) {
        match (min_x, max_x, min_y, max_y) {
            (Some(cur_min_x), Some(cur_max_x), Some(cur_min_y), Some(cur_max_y)) => {
                min_x = Some(cur_min_x.min(x));
                max_x = Some(cur_max_x.max(x));
                min_y = Some(cur_min_y.min(y));
                max_y = Some(cur_max_y.max(y));
            }
            _ => {
                min_x = Some(x);
                max_x = Some(x);
                min_y = Some(y);
                max_y = Some(y);
            }
        }
    }

    LayoutBounds {
        min_x: min_x.unwrap_or(0.0),
        max_x: max_x.unwrap_or(0.0),
        min_y: min_y.unwrap_or(0.0),
        max_y: max_y.unwrap_or(0.0),
    }
}

fn finalize_layout_tree(
    normalized: NormalizedTreeData,
    x_by_id: Vec<f64>,
    y_by_id: Vec<f64>,
    branch_angles: Vec<f64>,
    summary: LayoutSummary,
) -> LayoutTreeData {
    LayoutTreeData {
        normalized,
        x_by_id,
        y_by_id,
        branch_angles,
        layout_kind: summary.layout_kind,
        fit_mode: summary.fit_mode,
        primitive_mode: summary.primitive_mode,
        tree_depth: summary
            .tree_depth
            .unwrap_or(summary.bounds.max_x - summary.bounds.min_x),
        tree_height: summary
            .tree_height
            .unwrap_or(summary.bounds.max_y - summary.bounds.min_y),
    }
}

fn layout_tree_to_wire(layout: &LayoutTreeData) -> LayoutTreeWire {
    let nodes = layout
        .normalized
        .nodes
        .iter()
        .map(|node| LayoutNodeWire {
            parent_id: node.parent_id,
            children_ids: node.children_ids.clone(),
            is_root: node.is_root,
            input_rooted: node.input_rooted,
            is_leaf: node.is_leaf,
            label_text: node.label_text.clone(),
            label_id: node.label_id.clone(),
            x_unit: layout.x_by_id[node.id],
            y_unit: layout.y_by_id[node.id],
            branch_angle: layout.branch_angles[node.id],
        })
        .collect();

    LayoutTreeWire {
        nodes,
        root_id: layout.normalized.root_id,
        node_count: layout.normalized.node_count,
        effective_cladogram: layout.normalized.effective_cladogram,
        layout_kind: layout.layout_kind,
        fit_mode: layout.fit_mode,
        primitive_mode: layout.primitive_mode,
        tree_depth: layout.tree_depth,
        tree_height: layout.tree_height,
    }
}

fn tree_tip_counts(normalized: &NormalizedTreeData) -> Vec<usize> {
    let mut tip_counts = vec![0; normalized.node_count];
    for id in (0..normalized.node_count).rev() {
        let node = &normalized.nodes[id];
        tip_counts[id] = if node.is_leaf {
            1
        } else {
            node.children_ids
                .iter()
                .map(|&child_id| tip_counts[child_id])
                .sum()
        };
    }
    tip_counts
}

fn build_equal_angle_state(normalized: &NormalizedTreeData) -> EqualAngleState {
    let tip_counts = tree_tip_counts(normalized);
    let mut x_by_id = vec![0.0; normalized.node_count];
    let mut y_by_id = vec![0.0; normalized.node_count];
    let mut angle_starts = vec![0.0; normalized.node_count];
    let mut angle_ends = vec![0.0; normalized.node_count];
    let mut branch_angles = vec![0.0; normalized.node_count];

    angle_starts[normalized.root_id] = 0.0;
    angle_ends[normalized.root_id] = 2.0;
    branch_angles[normalized.root_id] = 0.0;

    for id in 0..normalized.node_count {
        let node = &normalized.nodes[id];
        let current_start = angle_starts[id];
        let current_end = angle_ends[id];
        let total_angle = current_end - current_start;
        let total_tips = tip_counts[id];
        let current_x = x_by_id[id];
        let current_y = y_by_id[id];
        let mut next_start = current_start;

        for &child_id in &node.children_ids {
            let child_tips = tip_counts[child_id];
            let alpha = total_angle * child_tips as f64 / total_tips as f64;
            let beta = next_start + alpha / 2.0;
            let theta = half_turn_to_radians(beta);
            let (sin_theta, cos_theta) = theta.sin_cos();
            let branch_length = normalized.nodes[child_id].resolved_length;

            x_by_id[child_id] = current_x + branch_length * cos_theta;
            y_by_id[child_id] = current_y + branch_length * sin_theta;
            angle_starts[child_id] = next_start;
            angle_ends[child_id] = next_start + alpha;
            branch_angles[child_id] = normalize_half_turn(beta);
            next_start += alpha;
        }
    }

    EqualAngleState {
        x_by_id,
        y_by_id,
        branch_angles,
    }
}

fn layout_tree_rectangular(normalized: NormalizedTreeData) -> LayoutTreeData {
    let mut subtree_heights = vec![0.0; normalized.node_count];
    let mut y_locals = vec![0.0; normalized.node_count];
    let mut child_offsets = vec![Vec::new(); normalized.node_count];

    for id in (0..normalized.node_count).rev() {
        let node = &normalized.nodes[id];
        if node.is_leaf {
            subtree_heights[id] = 1.0;
            y_locals[id] = 0.5;
            child_offsets[id] = Vec::new();
        } else {
            let mut subtree_height = 0.0;
            let mut offsets = Vec::with_capacity(node.children_ids.len());
            let mut first_center = None::<f64>;
            let mut last_center = None::<f64>;

            for &child_id in &node.children_ids {
                offsets.push(subtree_height);
                let child_center = subtree_height + y_locals[child_id];
                if first_center.is_none() {
                    first_center = Some(child_center);
                }
                last_center = Some(child_center);
                subtree_height += subtree_heights[child_id];
            }

            subtree_heights[id] = subtree_height;
            y_locals[id] = (first_center.unwrap_or(0.0) + last_center.unwrap_or(0.0)) / 2.0;
            child_offsets[id] = offsets;
        }
    }

    let mut x_by_id = vec![0.0; normalized.node_count];
    let mut y_by_id = vec![0.0; normalized.node_count];
    let branch_angles = vec![0.0; normalized.node_count];
    y_by_id[normalized.root_id] = y_locals[normalized.root_id];

    let mut tree_depth = 0.0_f64;
    for id in 0..normalized.node_count {
        tree_depth = tree_depth.max(x_by_id[id]);
        let subtree_top = y_by_id[id] - y_locals[id];
        for (index, &child_id) in normalized.nodes[id].children_ids.iter().enumerate() {
            x_by_id[child_id] = x_by_id[id] + normalized.nodes[child_id].resolved_length;
            y_by_id[child_id] = subtree_top + child_offsets[id][index] + y_locals[child_id];
            tree_depth = tree_depth.max(x_by_id[child_id]);
        }
    }

    let root_height = subtree_heights[normalized.root_id];

    finalize_layout_tree(
        normalized,
        x_by_id,
        y_by_id,
        branch_angles,
        LayoutSummary {
            layout_kind: LayoutKind::Rectangular,
            fit_mode: LayoutFitModeWire::IndependentAxes,
            primitive_mode: PrimitiveModeWire::Rectangular,
            bounds: LayoutBounds {
                min_x: 0.0,
                max_x: tree_depth,
                min_y: 0.0,
                max_y: root_height,
            },
            tree_depth: Some(tree_depth),
            tree_height: Some(root_height),
        },
    )
}

fn layout_tree_equal_angle(normalized: NormalizedTreeData) -> LayoutTreeData {
    let equal_state = build_equal_angle_state(&normalized);
    let bounds = tree_layout_bounds(&equal_state.x_by_id, &equal_state.y_by_id);
    finalize_layout_tree(
        normalized,
        equal_state.x_by_id,
        equal_state.y_by_id,
        equal_state.branch_angles,
        LayoutSummary {
            layout_kind: LayoutKind::EqualAngle,
            fit_mode: LayoutFitModeWire::Uniform,
            primitive_mode: PrimitiveModeWire::EdgeSegments,
            bounds,
            tree_depth: None,
            tree_height: None,
        },
    )
}

fn build_daylight_cache(normalized: &NormalizedTreeData) -> DaylightCache {
    let mut queue = vec![normalized.root_id];
    let mut queue_index = 0;
    let mut active_internal_ids = Vec::new();
    let mut internal_count = 0;
    let mut max_component_count = 0;
    let mut component_spans_by_id = vec![DaylightSpan { start: 0, len: 0 }; normalized.node_count];
    let mut builder = DaylightCacheBuilder::new(normalized);

    while queue_index < queue.len() {
        let node_id = queue[queue_index];
        queue_index += 1;
        let node = &normalized.nodes[node_id];

        if node.is_leaf {
            continue;
        }

        internal_count += 1;
        let component_start = builder.components.len();

        for &child_id in &node.children_ids {
            let child = &normalized.nodes[child_id];
            builder.push_component(
                node_id,
                child_id,
                IntervalSet::single(child_id, child.subtree_end_id),
            );
            queue.push(child_id);
        }

        if !node.is_root {
            let second = (node.subtree_end_id + 1 < normalized.node_count).then_some(Interval {
                start: node.subtree_end_id + 1,
                end: normalized.node_count - 1,
            });
            builder.push_component(
                node_id,
                node.parent_id.expect("non-root nodes must have parents"),
                IntervalSet::with_second(
                    Interval {
                        start: 0,
                        end: node_id,
                    },
                    second,
                ),
            );
        }

        let component_len = builder.components.len() - component_start;
        if component_len > max_component_count {
            max_component_count = component_len;
        }
        if component_len > 2 {
            active_internal_ids.push(node_id);
        }
        component_spans_by_id[node_id] = DaylightSpan {
            start: component_start,
            len: component_len,
        };
    }

    builder.into_cache(
        active_internal_ids,
        internal_count,
        max_component_count,
        component_spans_by_id,
    )
}

fn rotate_daylight_component(
    x_by_id: &mut [f64],
    y_by_id: &mut [f64],
    pivot_id: usize,
    intervals: &IntervalSet,
    angle: f64,
) {
    let pivot_x = x_by_id[pivot_id];
    let pivot_y = y_by_id[pivot_id];
    let theta = half_turn_to_radians(angle);
    let (sin_theta, cos_theta) = theta.sin_cos();

    for node_id in intervals.first.start..=intervals.first.end {
        let dx = x_by_id[node_id] - pivot_x;
        let dy = y_by_id[node_id] - pivot_y;
        x_by_id[node_id] = cos_theta * dx - sin_theta * dy + pivot_x;
        y_by_id[node_id] = sin_theta * dx + cos_theta * dy + pivot_y;
    }
    if let Some(interval) = intervals.second {
        for node_id in interval.start..=interval.end {
            let dx = x_by_id[node_id] - pivot_x;
            let dy = y_by_id[node_id] - pivot_y;
            x_by_id[node_id] = cos_theta * dx - sin_theta * dy + pivot_x;
            y_by_id[node_id] = sin_theta * dx + cos_theta * dy + pivot_y;
        }
    }
}

fn fill_pivot_angle_cache(
    x_by_id: &[f64],
    y_by_id: &[f64],
    pivot_id: usize,
    angle_cache: &mut [f64],
) {
    let pivot_x = x_by_id[pivot_id];
    let pivot_y = y_by_id[pivot_id];

    for node_id in 0..angle_cache.len() {
        let dx = x_by_id[node_id] - pivot_x;
        let dy = y_by_id[node_id] - pivot_y;
        angle_cache[node_id] =
            if dx.abs() <= DEGENERATE_TOLERANCE && dy.abs() <= DEGENERATE_TOLERANCE {
                f64::NAN
            } else {
                dy.atan2(dx) / std::f64::consts::PI
            };
    }
}

fn cached_node_angle_half_turn(angle_cache: &[f64], node_id: usize, default_angle: f64) -> f64 {
    let angle = angle_cache[node_id];
    if angle.is_nan() { default_angle } else { angle }
}

fn daylight_component_arc(
    normalized: &NormalizedTreeData,
    branch_angles: &[f64],
    pivot_id: usize,
    component: DaylightComponent,
    cache: &DaylightCache,
    angle_cache: &[f64],
) -> DaylightArc {
    let pivot = &normalized.nodes[pivot_id];
    let adjacent_default = if Some(component.node_id) == pivot.parent_id {
        normalize_half_turn(branch_angles[pivot_id] + 1.0)
    } else {
        branch_angles[component.node_id]
    };
    let adjacent_angle =
        cached_node_angle_half_turn(angle_cache, component.node_id, adjacent_default);

    let mut arc_left = adjacent_angle;
    let mut arc_right = adjacent_angle;

    for &parent_entry in cache.parent_entries_for(component) {
        let theta_parent =
            cached_node_angle_half_turn(angle_cache, parent_entry.parent_id, adjacent_default);

        for &child_id in cache.child_ids_for(parent_entry) {
            let theta_child =
                cached_node_angle_half_turn(angle_cache, child_id, branch_angles[child_id]);

            let child_inside_arc = if arc_left < arc_right {
                theta_child > arc_left && theta_child < arc_right
            } else {
                !(theta_child < arc_left && theta_child > arc_right)
            };
            if !child_inside_arc {
                continue;
            }

            let delta = theta_child - theta_parent;
            let delta_adjusted = signed_half_turn_delta(delta);
            let mut theta_child_adjusted = theta_child;

            if delta_adjusted > 0.0 {
                if delta.abs() > 1.0 {
                    if arc_left > 0.0 && theta_child < 0.0 {
                        theta_child_adjusted = theta_child + 2.0;
                    } else if arc_left < 0.0 && theta_child > 0.0 {
                        theta_child_adjusted = theta_child - 2.0;
                    }
                }
                if arc_left < theta_child_adjusted {
                    arc_left = theta_child;
                }
            } else if delta_adjusted < 0.0 {
                if delta.abs() > 1.0 {
                    if arc_right > 0.0 && theta_child < 0.0 {
                        theta_child_adjusted = theta_child + 2.0;
                    } else if arc_right < 0.0 && theta_child > 0.0 {
                        theta_child_adjusted = theta_child - 2.0;
                    }
                }
                if arc_right > theta_child_adjusted {
                    arc_right = theta_child;
                }
            }
        }
    }

    DaylightArc {
        left: if arc_left < 0.0 {
            arc_left + 2.0
        } else {
            arc_left
        },
        right: if arc_right < 0.0 {
            arc_right + 2.0
        } else {
            arc_right
        },
    }
}

fn stable_sort_component_arcs(component_arcs: &mut [ComponentArcEntry]) {
    if component_arcs.len() <= 8 {
        for index in 1..component_arcs.len() {
            let entry = component_arcs[index];
            let mut insert_at = index;
            while insert_at > 0
                && component_arcs[insert_at - 1]
                    .left
                    .total_cmp(&entry.left)
                    .is_gt()
            {
                component_arcs[insert_at] = component_arcs[insert_at - 1];
                insert_at -= 1;
            }
            component_arcs[insert_at] = entry;
        }
    } else {
        component_arcs.sort_by(|first, second| first.left.total_cmp(&second.left));
    }
}

struct DaylightWorkspace {
    angle_cache: Vec<f64>,
    component_arcs: Vec<ComponentArcEntry>,
}

impl DaylightWorkspace {
    fn new(node_count: usize, max_component_count: usize) -> Self {
        Self {
            angle_cache: vec![f64::NAN; node_count],
            component_arcs: Vec::with_capacity(max_component_count),
        }
    }

    fn apply_at_node(
        &mut self,
        normalized: &NormalizedTreeData,
        cache: &DaylightCache,
        x_by_id: &mut [f64],
        y_by_id: &mut [f64],
        branch_angles: &[f64],
        pivot_id: usize,
    ) -> f64 {
        let component_span = cache.component_span(pivot_id);
        let components = cache.components_at(pivot_id);
        if components.len() <= 2 {
            return 0.0;
        }

        fill_pivot_angle_cache(x_by_id, y_by_id, pivot_id, &mut self.angle_cache);
        self.component_arcs.clear();

        for (component_index, &component) in components.iter().enumerate() {
            let arc = daylight_component_arc(
                normalized,
                branch_angles,
                pivot_id,
                component,
                cache,
                &self.angle_cache,
            );
            self.component_arcs.push(ComponentArcEntry {
                left: arc.left,
                beta: half_turn_arc_width(arc.left, arc.right),
                component_index: component_span.start + component_index,
            });
        }

        stable_sort_component_arcs(self.component_arcs.as_mut_slice());
        let occupied_angle: f64 = self.component_arcs.iter().map(|entry| entry.beta).sum();
        let total_daylight = (2.0 - occupied_angle).max(0.0);
        let gap = total_daylight / self.component_arcs.len() as f64;
        let mut new_left = self.component_arcs[0].left;
        let mut max_change = 0.0_f64;

        for entry in self.component_arcs.iter().skip(1) {
            new_left += gap + entry.beta;
            let adjust_angle = new_left - entry.left;
            max_change = max_change.max(adjust_angle.abs());
            let component = cache.components[entry.component_index];
            rotate_daylight_component(
                x_by_id,
                y_by_id,
                pivot_id,
                &component.intervals,
                adjust_angle,
            );
        }

        max_change
    }
}

fn recompute_branch_angles(
    normalized: &NormalizedTreeData,
    x_by_id: &[f64],
    y_by_id: &[f64],
    branch_angles: &mut [f64],
) {
    for id in 0..normalized.node_count {
        let node = &normalized.nodes[id];
        branch_angles[id] = if node.is_root {
            0.0
        } else {
            normalize_half_turn(node_angle_half_turn(
                x_by_id[node.parent_id.expect("non-root nodes must have parents")],
                y_by_id[node.parent_id.expect("non-root nodes must have parents")],
                x_by_id[id],
                y_by_id[id],
                branch_angles[id],
            ))
        };
    }
}

fn layout_tree_daylight(normalized: NormalizedTreeData) -> LayoutTreeData {
    let equal_state = build_equal_angle_state(&normalized);
    let cache = build_daylight_cache(&normalized);
    let mut x_by_id = equal_state.x_by_id;
    let mut y_by_id = equal_state.y_by_id;
    let mut branch_angles = equal_state.branch_angles;
    let mut workspace = DaylightWorkspace::new(normalized.node_count, cache.max_component_count);

    for _ in 0..5 {
        let mut total_change = 0.0;
        for &pivot_id in &cache.active_internal_ids {
            total_change += workspace.apply_at_node(
                &normalized,
                &cache,
                &mut x_by_id,
                &mut y_by_id,
                &branch_angles,
                pivot_id,
            );
        }

        let average_change = if cache.internal_count == 0 {
            0.0
        } else {
            total_change / cache.internal_count as f64
        };
        if average_change <= 0.05 {
            break;
        }
    }

    recompute_branch_angles(&normalized, &x_by_id, &y_by_id, &mut branch_angles);
    let bounds = tree_layout_bounds(&x_by_id, &y_by_id);
    finalize_layout_tree(
        normalized,
        x_by_id,
        y_by_id,
        branch_angles,
        LayoutSummary {
            layout_kind: LayoutKind::Daylight,
            fit_mode: LayoutFitModeWire::Uniform,
            primitive_mode: PrimitiveModeWire::EdgeSegments,
            bounds,
            tree_depth: None,
            tree_height: None,
        },
    )
}

fn layout_normalized_tree(
    normalized: NormalizedTreeData,
    layout_kind: LayoutKind,
) -> Result<LayoutTreeData, String> {
    Ok(match layout_kind {
        LayoutKind::Rectangular => layout_tree_rectangular(normalized),
        LayoutKind::EqualAngle => layout_tree_equal_angle(normalized),
        LayoutKind::Daylight => layout_tree_daylight(normalized),
    })
}

impl FitRequest {
    fn validate(&self) -> Result<(), String> {
        if self.fit_max_bands == 0 {
            return Err("fit_max_bands must be positive.".into());
        }
        if self.fit_mode == FitMode::IndependentAxes && self.fit_band_samples.unwrap_or(0) == 0 {
            return Err("fit_band_samples must be positive for independent-axis fitting.".into());
        }
        if self.height_mode == HeightMode::Resolved && self.viewport_height_pt.is_none() {
            return Err("viewport_height_pt is required when height_mode is resolved.".into());
        }
        if self.width_mode == WidthMode::Resolved && self.viewport_width_pt.is_none() {
            return Err("viewport_width_pt is required when width_mode is resolved.".into());
        }
        Ok(())
    }
}

impl From<&FitRequest> for FitInputs {
    fn from(request: &FitRequest) -> Self {
        Self {
            prepared_lines: request.prepared_lines.clone(),
            prepared_labels: request.prepared_labels.clone(),
            root_tree_point: request.root_tree_point,
            tree_depth: request.tree_depth,
            tree_height: request.tree_height,
        }
    }
}

impl BoundsAccumulator {
    fn expand(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        match (self.min_x, self.min_y, self.max_x, self.max_y) {
            (Some(cur_min_x), Some(cur_min_y), Some(cur_max_x), Some(cur_max_y)) => {
                self.min_x = Some(cur_min_x.min(min_x));
                self.min_y = Some(cur_min_y.min(min_y));
                self.max_x = Some(cur_max_x.max(max_x));
                self.max_y = Some(cur_max_y.max(max_y));
            }
            _ => {
                self.min_x = Some(min_x);
                self.min_y = Some(min_y);
                self.max_x = Some(max_x);
                self.max_y = Some(max_y);
            }
        }
    }

    fn finalize(&self) -> Bounds {
        match (self.min_x, self.min_y, self.max_x, self.max_y) {
            (Some(min_x), Some(min_y), Some(max_x), Some(max_y)) => Bounds {
                min_x,
                min_y,
                max_x,
                max_y,
                width: max_x - min_x,
                height: max_y - min_y,
            },
            _ => Bounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 0.0,
                max_y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        }
    }
}

fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn rem_euclid_half_turn(value: f64) -> f64 {
    let wrapped = value.rem_euclid(2.0);
    if approx_eq(wrapped, 2.0, 1e-9) {
        0.0
    } else {
        wrapped
    }
}

fn format_pt(value: f64) -> String {
    format!("{value:.4}pt")
}

fn half_turn_to_deg(value: f64) -> f64 {
    value * 180.0
}

fn transform_point(x: f64, y: f64, orientation: Orientation) -> Point {
    match orientation {
        Orientation::Vertical => Point { x: y, y: -x },
        Orientation::Horizontal => Point { x, y },
    }
}

fn rotate_page_vector(point: Point, rotation_deg: f64) -> Point {
    let theta = rotation_deg.to_radians();
    let cos_theta = theta.cos();
    let sin_theta = theta.sin();
    Point {
        x: cos_theta * point.x - sin_theta * point.y,
        y: sin_theta * point.x + cos_theta * point.y,
    }
}

fn is_negative_ninety_deg(rotation_deg: f64) -> bool {
    approx_eq(rotation_deg, -90.0, 1e-9)
}

fn screen_label_box_size(primitive: &PreparedLabel) -> Point {
    if is_negative_ninety_deg(primitive.rotation_deg) {
        Point {
            x: primitive.measure_height_pt,
            y: primitive.measure_width_pt,
        }
    } else {
        Point {
            x: primitive.measure_width_pt,
            y: primitive.measure_height_pt,
        }
    }
}

fn label_unrotated_box_size(primitive: &PreparedLabel) -> Point {
    Point {
        x: primitive.measure_width_pt,
        y: primitive.measure_height_pt,
    }
}

fn label_local_anchor_vector(primitive: &PreparedLabel) -> Point {
    let box_size = label_unrotated_box_size(primitive);
    Point {
        x: match primitive.x_align {
            XAlign::Left => 0.0,
            XAlign::Right => box_size.x,
            XAlign::Center => box_size.x / 2.0,
        },
        y: match primitive.y_align {
            YAlign::Top => 0.0,
            YAlign::Bottom => box_size.y,
            YAlign::Center => box_size.y / 2.0,
        },
    }
}

fn label_local_top_left(anchor: Point, primitive: &PreparedLabel) -> Point {
    let anchor_vector = label_local_anchor_vector(primitive);
    let local_offset = Point {
        x: primitive.x_gap_pt - anchor_vector.x,
        y: primitive.y_gap_pt - anchor_vector.y,
    };
    let rotated_offset = rotate_page_vector(local_offset, primitive.rotation_deg);
    Point {
        x: anchor.x + rotated_offset.x,
        y: anchor.y + rotated_offset.y,
    }
}

fn rotated_label_bounds(top_left: Point, primitive: &PreparedLabel) -> Bounds {
    let box_size = label_unrotated_box_size(primitive);
    let corners = [
        rotate_page_vector(Point { x: 0.0, y: 0.0 }, primitive.rotation_deg),
        rotate_page_vector(
            Point {
                x: box_size.x,
                y: 0.0,
            },
            primitive.rotation_deg,
        ),
        rotate_page_vector(
            Point {
                x: 0.0,
                y: box_size.y,
            },
            primitive.rotation_deg,
        ),
        rotate_page_vector(
            Point {
                x: box_size.x,
                y: box_size.y,
            },
            primitive.rotation_deg,
        ),
    ];
    let mut bounds = BoundsAccumulator::default();
    for corner in corners {
        bounds.expand(corner.x, corner.y, corner.x, corner.y);
    }
    let rotated = bounds.finalize();
    Bounds {
        min_x: top_left.x + rotated.min_x,
        min_y: top_left.y + rotated.min_y,
        max_x: top_left.x + rotated.max_x,
        max_y: top_left.y + rotated.max_y,
        width: rotated.width,
        height: rotated.height,
    }
}

fn screen_label_relative_top_left(primitive: &PreparedLabel) -> Point {
    let box_size = screen_label_box_size(primitive);
    let support_x = match primitive.x_align {
        XAlign::Left => 0.0,
        XAlign::Right => box_size.x,
        XAlign::Center => box_size.x / 2.0,
    };

    if let Some(angle_half_turn) = primitive.placement_angle_half_turn {
        let theta = half_turn_to_deg(angle_half_turn).to_radians();
        let dx = theta.cos();
        let dy = theta.sin();
        let axis = dx.abs().max(dy.abs()).max(DEGENERATE_TOLERANCE);
        let edge_weight = dy.abs() * dy.abs();
        let edge_support_y = if dy >= 0.0 { 0.0 } else { box_size.y };
        // Near-horizontal unrooted labels look better when they drift toward
        // vertical centering instead of snapping fully to the top or bottom edge.
        let support_y = edge_support_y * edge_weight + (box_size.y / 2.0) * (1.0 - edge_weight);
        Point {
            x: primitive.x_gap_pt * dx / axis - support_x,
            y: primitive.y_gap_pt * dy / axis - support_y,
        }
    } else {
        let support_y = match primitive.y_align {
            YAlign::Top => 0.0,
            YAlign::Bottom => box_size.y,
            YAlign::Center => box_size.y / 2.0,
        };
        Point {
            x: match primitive.x_align {
                XAlign::Left => primitive.x_gap_pt,
                XAlign::Right => -primitive.x_gap_pt - box_size.x,
                XAlign::Center => primitive.x_gap_pt - box_size.x / 2.0,
            },
            y: match primitive.y_align {
                YAlign::Bottom => -primitive.y_gap_pt - support_y,
                _ => primitive.y_gap_pt - support_y,
            },
        }
    }
}

fn label_relative_bounds(primitive: &PreparedLabel) -> Bounds {
    match primitive.placement_frame {
        PlacementFrame::Local => {
            let top_left = label_local_top_left(Point { x: 0.0, y: 0.0 }, primitive);
            rotated_label_bounds(top_left, primitive)
        }
        PlacementFrame::Screen => {
            let box_size = screen_label_box_size(primitive);
            let top_left = screen_label_relative_top_left(primitive);
            Bounds {
                min_x: top_left.x,
                min_y: top_left.y,
                max_x: top_left.x + box_size.x,
                max_y: top_left.y + box_size.y,
                width: box_size.x,
                height: box_size.y,
            }
        }
    }
}

fn materialize_line(
    primitive: &PreparedLine,
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
) -> (Point, Point) {
    (
        transform_point(
            primitive.start_anchor.tree.x * x_scale_pt + primitive.start_anchor.page.x,
            primitive.start_anchor.tree.y * y_scale_pt + primitive.start_anchor.page.y,
            orientation,
        ),
        transform_point(
            primitive.end_anchor.tree.x * x_scale_pt + primitive.end_anchor.page.x,
            primitive.end_anchor.tree.y * y_scale_pt + primitive.end_anchor.page.y,
            orientation,
        ),
    )
}

fn line_bounds(start: Point, end: Point, half_stroke_pt: f64) -> Bounds {
    Bounds {
        min_x: start.x.min(end.x) - half_stroke_pt,
        min_y: start.y.min(end.y) - half_stroke_pt,
        max_x: start.x.max(end.x) + half_stroke_pt,
        max_y: start.y.max(end.y) + half_stroke_pt,
        width: (start.x.max(end.x) + half_stroke_pt) - (start.x.min(end.x) - half_stroke_pt),
        height: (start.y.max(end.y) + half_stroke_pt) - (start.y.min(end.y) - half_stroke_pt),
    }
}

fn line_is_degenerate(start: Point, end: Point) -> bool {
    let dx = (end.x - start.x).abs();
    let dy = (end.y - start.y).abs();
    dx <= FIT_TOLERANCE_PT && dy <= FIT_TOLERANCE_PT
}

fn materialize_label_anchor(
    primitive: &PreparedLabel,
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
) -> Point {
    let transformed = transform_point(
        primitive.anchor_tree.x * x_scale_pt,
        primitive.anchor_tree.y * y_scale_pt,
        orientation,
    );
    Point {
        x: transformed.x + primitive.anchor_page.x,
        y: transformed.y + primitive.anchor_page.y,
    }
}

fn materialize_label_geometry(
    primitive: &PreparedLabel,
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
) -> (Point, Bounds) {
    let anchor = materialize_label_anchor(primitive, x_scale_pt, y_scale_pt, orientation);
    match primitive.placement_frame {
        PlacementFrame::Local => {
            let top_left = label_local_top_left(anchor, primitive);
            let bounds = rotated_label_bounds(top_left, primitive);
            (top_left, bounds)
        }
        PlacementFrame::Screen => {
            let box_size = screen_label_box_size(primitive);
            let relative_top_left = screen_label_relative_top_left(primitive);
            let top_left = Point {
                x: anchor.x + relative_top_left.x,
                y: anchor.y + relative_top_left.y,
            };
            let origin = if is_negative_ninety_deg(primitive.rotation_deg) {
                Point {
                    x: top_left.x,
                    y: top_left.y + primitive.measure_width_pt,
                }
            } else {
                top_left
            };
            let bounds = Bounds {
                min_x: top_left.x,
                min_y: top_left.y,
                max_x: top_left.x + box_size.x,
                max_y: top_left.y + box_size.y,
                width: box_size.x,
                height: box_size.y,
            };
            (origin, bounds)
        }
    }
}

fn materialize_fitted_line(
    primitive: &PreparedLine,
    line_index: usize,
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
) -> Option<(MaterializedLine, Bounds)> {
    let (start, end) = materialize_line(primitive, x_scale_pt, y_scale_pt, orientation);
    if line_is_degenerate(start, end) {
        None
    } else {
        Some((
            MaterializedLine {
                line_index,
                start,
                end,
            },
            line_bounds(start, end, primitive.half_stroke_pt),
        ))
    }
}

fn materialize_fitted_label(
    primitive: &PreparedLabel,
    label_index: usize,
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
) -> (MaterializedLabel, Bounds) {
    let (origin, bounds) =
        materialize_label_geometry(primitive, x_scale_pt, y_scale_pt, orientation);
    (
        MaterializedLabel {
            label_index,
            origin,
            rotation_deg: primitive.rotation_deg,
        },
        bounds,
    )
}

fn for_each_internal_label_candidate(
    primitive: &PreparedLabel,
    mut visit: impl FnMut(PreparedLabel, usize, usize),
) {
    let preferred_angle = primitive
        .placement_angle_half_turn
        .expect("internal-label placement should provide a direction");
    let candidate_angles = [
        preferred_angle,
        rem_euclid_half_turn(preferred_angle + 0.5),
        rem_euclid_half_turn(preferred_angle + 1.5),
        rem_euclid_half_turn(preferred_angle + 1.0),
    ];
    let gap_scales = [1.0, 1.5, 2.0];

    for (direction_rank, angle) in candidate_angles.into_iter().enumerate() {
        for (gap_rank, gap_scale) in gap_scales.into_iter().enumerate() {
            let placement = horizontal_label_placement(angle);
            let mut candidate = *primitive;
            candidate.x_align = placement.x_align;
            candidate.y_align = placement.y_align;
            candidate.x_gap_pt = primitive.x_gap_pt * gap_scale;
            candidate.y_gap_pt = primitive.y_gap_pt * gap_scale;
            candidate.placement_angle_half_turn = Some(placement.placement_angle_half_turn);
            visit(candidate, direction_rank, gap_rank);
        }
    }
}

fn materialize_fitted_internal_label(
    primitive: &PreparedLabel,
    label_index: usize,
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
    occupied_internal_bounds: &[Bounds],
) -> (MaterializedLabel, Bounds) {
    let mut best_score: Option<(usize, f64, usize, usize)> = None;
    let mut best_result: Option<(MaterializedLabel, Bounds)> = None;

    for_each_internal_label_candidate(primitive, |candidate, direction_rank, gap_rank| {
        let (label, bounds) =
            materialize_fitted_label(&candidate, label_index, x_scale_pt, y_scale_pt, orientation);
        let mut overlap_count = 0usize;
        let mut overlap_area = 0.0;
        for occupied in occupied_internal_bounds.iter().copied() {
            let overlap_width =
                (bounds.max_x.min(occupied.max_x) - bounds.min_x.max(occupied.min_x)).max(0.0);
            let overlap_height =
                (bounds.max_y.min(occupied.max_y) - bounds.min_y.max(occupied.min_y)).max(0.0);
            let area = overlap_width * overlap_height;
            if area > 0.0 {
                overlap_count += 1;
                overlap_area += area;
            }
        }
        let candidate_score = (overlap_count, overlap_area, direction_rank, gap_rank);
        if best_score
            .map(|score| candidate_score < score)
            .unwrap_or(true)
        {
            best_score = Some(candidate_score);
            best_result = Some((label, bounds));
        }
    });

    let (label, bounds) =
        best_result.expect("internal label should always produce at least one candidate");
    (label, bounds)
}

fn evaluate_tree_bounds_only(
    prepared_lines: &[PreparedLine],
    prepared_labels: &[PreparedLabel],
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
) -> Bounds {
    let mut bounds = BoundsAccumulator::default();

    for (index, primitive) in prepared_lines.iter().enumerate() {
        if let Some((_, line_bounds)) =
            materialize_fitted_line(primitive, index, x_scale_pt, y_scale_pt, orientation)
        {
            bounds.expand(
                line_bounds.min_x,
                line_bounds.min_y,
                line_bounds.max_x,
                line_bounds.max_y,
            );
        }
    }

    for (index, primitive) in prepared_labels.iter().enumerate() {
        let (_, label_bounds) =
            materialize_fitted_label(primitive, index, x_scale_pt, y_scale_pt, orientation);
        bounds.expand(
            label_bounds.min_x,
            label_bounds.min_y,
            label_bounds.max_x,
            label_bounds.max_y,
        );
    }

    bounds.finalize()
}

fn materialize_fitted_tree(
    prepared_lines: &[PreparedLine],
    prepared_labels: &[PreparedLabel],
    root_tree_point: Point,
    x_scale_pt: f64,
    y_scale_pt: f64,
    orientation: Orientation,
) -> MaterializedTree {
    let mut tree_lines = Vec::new();
    let mut tree_labels = Vec::new();
    let mut occupied_internal_label_bounds = Vec::new();
    let mut bounds = BoundsAccumulator::default();
    let root_position = transform_point(
        root_tree_point.x * x_scale_pt,
        root_tree_point.y * y_scale_pt,
        orientation,
    );

    for (index, primitive) in prepared_lines.iter().enumerate() {
        if let Some((line, line_bounds)) =
            materialize_fitted_line(primitive, index, x_scale_pt, y_scale_pt, orientation)
        {
            bounds.expand(
                line_bounds.min_x,
                line_bounds.min_y,
                line_bounds.max_x,
                line_bounds.max_y,
            );
            tree_lines.push(line);
        }
    }

    for (index, primitive) in prepared_labels.iter().enumerate() {
        let (label, label_bounds) = if primitive.placement_angle_half_turn.is_some() {
            materialize_fitted_internal_label(
                primitive,
                index,
                x_scale_pt,
                y_scale_pt,
                orientation,
                &occupied_internal_label_bounds,
            )
        } else {
            materialize_fitted_label(primitive, index, x_scale_pt, y_scale_pt, orientation)
        };
        bounds.expand(
            label_bounds.min_x,
            label_bounds.min_y,
            label_bounds.max_x,
            label_bounds.max_y,
        );
        if primitive.placement_angle_half_turn.is_some() {
            occupied_internal_label_bounds.push(label_bounds);
        }
        tree_labels.push(label);
    }

    MaterializedTree {
        tree_lines,
        tree_labels,
        root_position,
        tree_occupied_bounds: bounds.finalize(),
    }
}

fn radial_tip_label_placement(branch_angle_half_turn: f64) -> RadialPlacement {
    let raw_angle_half_turn = rem_euclid_half_turn(branch_angle_half_turn);
    let left_facing = (0.5..=1.5).contains(&raw_angle_half_turn);
    RadialPlacement {
        rotation_deg: if left_facing {
            half_turn_to_deg(rem_euclid_half_turn(raw_angle_half_turn + 1.0))
        } else {
            half_turn_to_deg(raw_angle_half_turn)
        },
        x_align: if left_facing {
            XAlign::Right
        } else {
            XAlign::Left
        },
        y_align: YAlign::Center,
        gap_sign: if left_facing { -1.0 } else { 1.0 },
        branch_angle_half_turn: raw_angle_half_turn,
    }
}

fn horizontal_label_placement(direction_angle_half_turn: f64) -> HorizontalPlacement {
    let placement_angle_half_turn = rem_euclid_half_turn(direction_angle_half_turn);
    let theta = half_turn_to_deg(placement_angle_half_turn).to_radians();
    let dx = theta.cos();
    let dy = theta.sin();
    HorizontalPlacement {
        x_align: if dx >= 0.0 {
            XAlign::Left
        } else {
            XAlign::Right
        },
        y_align: if dy < 0.0 {
            YAlign::Bottom
        } else {
            YAlign::Top
        },
        placement_angle_half_turn,
    }
}

impl RotationTransform {
    fn new(rotation_half_turn: f64) -> Self {
        let theta = half_turn_to_deg(rotation_half_turn).to_radians();
        Self {
            rotation_half_turn,
            cos_theta: theta.cos(),
            sin_theta: theta.sin(),
        }
    }

    fn rotate_tree_point(self, point: Point) -> Point {
        Point {
            x: self.cos_theta * point.x - self.sin_theta * point.y,
            y: self.sin_theta * point.x + self.cos_theta * point.y,
        }
    }

    fn rotate_line(self, primitive: PreparedLine) -> PreparedLine {
        PreparedLine {
            start_anchor: Anchor {
                tree: self.rotate_tree_point(primitive.start_anchor.tree),
                page: primitive.start_anchor.page,
            },
            end_anchor: Anchor {
                tree: self.rotate_tree_point(primitive.end_anchor.tree),
                page: primitive.end_anchor.page,
            },
            half_stroke_pt: primitive.half_stroke_pt,
        }
    }

    fn rotate_label(self, primitive: PreparedLabel) -> PreparedLabel {
        let mut rotated = primitive;
        rotated.anchor_tree = self.rotate_tree_point(primitive.anchor_tree);
        let rotated_branch_angle = primitive
            .branch_angle_half_turn
            .map(|value| rem_euclid_half_turn(value + self.rotation_half_turn));
        let radial_placement = rotated_branch_angle.map(radial_tip_label_placement);
        let rotated_placement_angle = primitive
            .placement_angle_half_turn
            .map(|value| rem_euclid_half_turn(value + self.rotation_half_turn));
        let internal_placement = rotated_placement_angle.map(horizontal_label_placement);

        if let Some(radial) = radial_placement {
            rotated.x_align = radial.x_align;
            rotated.y_align = radial.y_align;
            rotated.x_gap_pt = primitive.x_gap_pt.abs() * radial.gap_sign;
            rotated.rotation_deg = radial.rotation_deg;
            rotated.branch_angle_half_turn = Some(radial.branch_angle_half_turn);
        } else {
            rotated.branch_angle_half_turn = rotated_branch_angle;
        }

        if let Some(internal) = internal_placement {
            rotated.x_align = internal.x_align;
            rotated.y_align = internal.y_align;
            rotated.placement_angle_half_turn = Some(internal.placement_angle_half_turn);
        } else {
            rotated.placement_angle_half_turn = rotated_placement_angle;
        }

        rotated
    }

    fn rotate_fit_inputs(self, fit_inputs: &FitInputs) -> FitInputs {
        let prepared_lines = fit_inputs
            .prepared_lines
            .iter()
            .copied()
            .map(|primitive| self.rotate_line(primitive))
            .collect();

        let prepared_labels = fit_inputs
            .prepared_labels
            .iter()
            .copied()
            .map(|primitive| self.rotate_label(primitive))
            .collect();

        FitInputs {
            prepared_lines,
            prepared_labels,
            root_tree_point: self.rotate_tree_point(fit_inputs.root_tree_point),
            tree_depth: fit_inputs.tree_depth,
            tree_height: fit_inputs.tree_height,
        }
    }
}

fn affine_formula(coeff: f64, offset: f64) -> Formula {
    Formula { coeff, offset }
}

fn negate_affine_formula(formula: Formula) -> Formula {
    affine_formula(-formula.coeff, -formula.offset)
}

fn shift_affine_formula(formula: Formula, delta: f64) -> Formula {
    affine_formula(formula.coeff, formula.offset + delta)
}

fn affine_formulas_equal(first: Formula, second: Formula) -> bool {
    first.coeff == second.coeff && first.offset == second.offset
}

fn point_axis_formula(point: FormulaPoint, axis: Axis) -> Formula {
    match axis {
        Axis::X => point.x,
        Axis::Y => point.y,
    }
}

fn solve_screen_axis(orientation: Orientation, axis_kind: AxisKind) -> Axis {
    match orientation {
        Orientation::Vertical => match axis_kind {
            AxisKind::Depth => Axis::Y,
            AxisKind::Spread => Axis::X,
        },
        Orientation::Horizontal => match axis_kind {
            AxisKind::Depth => Axis::X,
            AxisKind::Spread => Axis::Y,
        },
    }
}

fn order_affine_interval(first: Formula, second: Formula) -> OrderedFormulaInterval {
    let first_precedes = first.coeff < second.coeff
        || (first.coeff == second.coeff && first.offset <= second.offset);
    if first_precedes {
        OrderedFormulaInterval {
            min: first,
            max: second,
        }
    } else {
        OrderedFormulaInterval {
            min: second,
            max: first,
        }
    }
}

fn solve_canonical_point(
    anchor_tree: Point,
    anchor_page: Point,
    axis_kind: AxisKind,
) -> FormulaPoint {
    match axis_kind {
        AxisKind::Depth => FormulaPoint {
            x: affine_formula(anchor_tree.x, anchor_page.x),
            y: affine_formula(0.0, anchor_page.y),
        },
        AxisKind::Spread => FormulaPoint {
            x: affine_formula(0.0, anchor_page.x),
            y: affine_formula(anchor_tree.y, anchor_page.y),
        },
    }
}

fn transform_point_formulas(
    x_formula: Formula,
    y_formula: Formula,
    orientation: Orientation,
) -> FormulaPoint {
    match orientation {
        Orientation::Vertical => FormulaPoint {
            x: y_formula,
            y: negate_affine_formula(x_formula),
        },
        Orientation::Horizontal => FormulaPoint {
            x: x_formula,
            y: y_formula,
        },
    }
}

fn uniform_point_formulas(
    anchor_tree: Point,
    anchor_page: Point,
    orientation: Orientation,
) -> FormulaPoint {
    transform_point_formulas(
        affine_formula(anchor_tree.x, anchor_page.x),
        affine_formula(anchor_tree.y, anchor_page.y),
        orientation,
    )
}

fn uniform_axis_descriptor(
    min_formula: Formula,
    max_formula: Formula,
    activation_threshold: Option<f64>,
) -> UniformAxisDescriptor {
    UniformAxisDescriptor {
        min_coeff: min_formula.coeff,
        min_offset: min_formula.offset,
        max_coeff: max_formula.coeff,
        max_offset: max_formula.offset,
        activation_threshold,
    }
}

fn uniform_line_bounds_descriptors(
    primitive: &PreparedLine,
    orientation: Orientation,
) -> Option<(UniformAxisDescriptor, UniformAxisDescriptor)> {
    let start = uniform_point_formulas(
        primitive.start_anchor.tree,
        primitive.start_anchor.page,
        orientation,
    );
    let end = uniform_point_formulas(
        primitive.end_anchor.tree,
        primitive.end_anchor.page,
        orientation,
    );
    let delta_x_coeff = end.x.coeff - start.x.coeff;
    let delta_y_coeff = end.y.coeff - start.y.coeff;

    if delta_x_coeff.abs() <= DEGENERATE_TOLERANCE && delta_y_coeff.abs() <= DEGENERATE_TOLERANCE {
        return None;
    }

    let activation_threshold = FIT_TOLERANCE_PT / delta_x_coeff.abs().max(delta_y_coeff.abs());
    let x_interval = order_affine_interval(start.x, end.x);
    let y_interval = order_affine_interval(start.y, end.y);
    Some((
        uniform_axis_descriptor(
            shift_affine_formula(x_interval.min, -primitive.half_stroke_pt),
            shift_affine_formula(x_interval.max, primitive.half_stroke_pt),
            Some(activation_threshold),
        ),
        uniform_axis_descriptor(
            shift_affine_formula(y_interval.min, -primitive.half_stroke_pt),
            shift_affine_formula(y_interval.max, primitive.half_stroke_pt),
            Some(activation_threshold),
        ),
    ))
}

fn uniform_label_bounds_descriptors(
    primitive: &PreparedLabel,
    orientation: Orientation,
) -> (UniformAxisDescriptor, UniformAxisDescriptor) {
    let anchor = uniform_point_formulas(primitive.anchor_tree, primitive.anchor_page, orientation);
    let relative_bounds = if primitive.placement_angle_half_turn.is_some() {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for_each_internal_label_candidate(primitive, |candidate, _, _| {
            let candidate_bounds = label_relative_bounds(&candidate);
            min_x = min_x.min(candidate_bounds.min_x);
            min_y = min_y.min(candidate_bounds.min_y);
            max_x = max_x.max(candidate_bounds.max_x);
            max_y = max_y.max(candidate_bounds.max_y);
        });

        Bounds {
            min_x,
            min_y,
            max_x,
            max_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    } else {
        label_relative_bounds(primitive)
    };
    (
        uniform_axis_descriptor(
            shift_affine_formula(anchor.x, relative_bounds.min_x),
            shift_affine_formula(anchor.x, relative_bounds.max_x),
            None,
        ),
        uniform_axis_descriptor(
            shift_affine_formula(anchor.y, relative_bounds.min_y),
            shift_affine_formula(anchor.y, relative_bounds.max_y),
            None,
        ),
    )
}

fn build_uniform_bounds_descriptors(
    prepared_lines: &[PreparedLine],
    prepared_labels: &[PreparedLabel],
    orientation: Orientation,
) -> UniformBoundsDescriptors {
    let mut x = Vec::new();
    let mut y = Vec::new();

    for primitive in prepared_lines {
        if let Some((x_desc, y_desc)) = uniform_line_bounds_descriptors(primitive, orientation) {
            x.push(x_desc);
            y.push(y_desc);
        }
    }

    for primitive in prepared_labels {
        let (x_desc, y_desc) = uniform_label_bounds_descriptors(primitive, orientation);
        x.push(x_desc);
        y.push(y_desc);
    }

    UniformBoundsDescriptors { x, y }
}

fn line_solve_descriptor(
    primitive: &PreparedLine,
    orientation: Orientation,
    axis_kind: AxisKind,
) -> Option<SolveDescriptor> {
    let start_canonical = solve_canonical_point(
        primitive.start_anchor.tree,
        primitive.start_anchor.page,
        axis_kind,
    );
    let end_canonical = solve_canonical_point(
        primitive.end_anchor.tree,
        primitive.end_anchor.page,
        axis_kind,
    );
    let start = transform_point_formulas(start_canonical.x, start_canonical.y, orientation);
    let end = transform_point_formulas(end_canonical.x, end_canonical.y, orientation);
    let is_degenerate =
        affine_formulas_equal(start.x, end.x) && affine_formulas_equal(start.y, end.y);

    if is_degenerate {
        None
    } else {
        let ordered = order_affine_interval(
            point_axis_formula(start, solve_screen_axis(orientation, axis_kind)),
            point_axis_formula(end, solve_screen_axis(orientation, axis_kind)),
        );
        Some(SolveDescriptor {
            min_coeff: ordered.min.coeff,
            min_offset: ordered.min.offset - primitive.half_stroke_pt,
            max_coeff: ordered.max.coeff,
            max_offset: ordered.max.offset + primitive.half_stroke_pt,
        })
    }
}

fn label_screen_anchor_formulas(
    primitive: &PreparedLabel,
    orientation: Orientation,
    axis_kind: AxisKind,
) -> FormulaPoint {
    let canonical =
        solve_canonical_point(primitive.anchor_tree, Point { x: 0.0, y: 0.0 }, axis_kind);
    let transformed = transform_point_formulas(canonical.x, canonical.y, orientation);
    FormulaPoint {
        x: shift_affine_formula(transformed.x, primitive.anchor_page.x),
        y: shift_affine_formula(transformed.y, primitive.anchor_page.y),
    }
}

fn label_solve_descriptor(
    primitive: &PreparedLabel,
    orientation: Orientation,
    axis_kind: AxisKind,
) -> Result<SolveDescriptor, String> {
    if primitive.placement_frame == PlacementFrame::Local {
        return Err("Local-frame labels are only supported by uniform tree fitting.".into());
    }

    let anchor = label_screen_anchor_formulas(primitive, orientation, axis_kind);
    let box_size = screen_label_box_size(primitive);
    let relative_top_left = screen_label_relative_top_left(primitive);
    let min_x = shift_affine_formula(anchor.x, relative_top_left.x);
    let min_y = shift_affine_formula(anchor.y, relative_top_left.y);

    Ok(match solve_screen_axis(orientation, axis_kind) {
        Axis::X => SolveDescriptor {
            min_coeff: min_x.coeff,
            min_offset: min_x.offset,
            max_coeff: min_x.coeff,
            max_offset: min_x.offset + box_size.x,
        },
        Axis::Y => SolveDescriptor {
            min_coeff: min_y.coeff,
            min_offset: min_y.offset,
            max_coeff: min_y.coeff,
            max_offset: min_y.offset + box_size.y,
        },
    })
}

fn build_solve_descriptors(
    prepared_lines: &[PreparedLine],
    prepared_labels: &[PreparedLabel],
    orientation: Orientation,
) -> Result<SolveDescriptors, String> {
    let mut depth = Vec::new();
    let mut spread = Vec::new();

    for primitive in prepared_lines {
        if let Some(descriptor) = line_solve_descriptor(primitive, orientation, AxisKind::Depth) {
            depth.push(descriptor);
        }
        if let Some(descriptor) = line_solve_descriptor(primitive, orientation, AxisKind::Spread) {
            spread.push(descriptor);
        }
    }

    for primitive in prepared_labels {
        depth.push(label_solve_descriptor(
            primitive,
            orientation,
            AxisKind::Depth,
        )?);
        spread.push(label_solve_descriptor(
            primitive,
            orientation,
            AxisKind::Spread,
        )?);
    }

    Ok(SolveDescriptors { depth, spread })
}

fn evaluate_solve_span(solve_descriptors: &[SolveDescriptor], scale: f64) -> f64 {
    let mut min_edge = None::<f64>;
    let mut max_edge = None::<f64>;

    for descriptor in solve_descriptors {
        let min_edge_at_scale = descriptor.min_coeff * scale + descriptor.min_offset;
        let max_edge_at_scale = descriptor.max_coeff * scale + descriptor.max_offset;
        match (min_edge, max_edge) {
            (Some(cur_min), Some(cur_max)) => {
                min_edge = Some(cur_min.min(min_edge_at_scale));
                max_edge = Some(cur_max.max(max_edge_at_scale));
            }
            _ => {
                min_edge = Some(min_edge_at_scale);
                max_edge = Some(max_edge_at_scale);
            }
        }
    }

    match (min_edge, max_edge) {
        (Some(min_edge), Some(max_edge)) => max_edge - min_edge,
        _ => 0.0,
    }
}

fn evaluate_uniform_axis_edges(
    descriptors: &[UniformAxisDescriptor],
    scale: f64,
) -> Option<(f64, f64)> {
    let mut min_edge = None::<f64>;
    let mut max_edge = None::<f64>;

    for descriptor in descriptors {
        if let Some(threshold) = descriptor.activation_threshold
            && scale <= threshold
        {
            continue;
        }

        let min_edge_at_scale = descriptor.min_coeff * scale + descriptor.min_offset;
        let max_edge_at_scale = descriptor.max_coeff * scale + descriptor.max_offset;
        match (min_edge, max_edge) {
            (Some(cur_min), Some(cur_max)) => {
                min_edge = Some(cur_min.min(min_edge_at_scale));
                max_edge = Some(cur_max.max(max_edge_at_scale));
            }
            _ => {
                min_edge = Some(min_edge_at_scale);
                max_edge = Some(max_edge_at_scale);
            }
        }
    }

    min_edge.zip(max_edge)
}

fn evaluate_uniform_bounds(
    bounds_descriptors: UniformBoundsDescriptorsRef<'_>,
    scale: f64,
) -> Bounds {
    let x_edges = evaluate_uniform_axis_edges(bounds_descriptors.x, scale);
    let y_edges = evaluate_uniform_axis_edges(bounds_descriptors.y, scale);

    match (x_edges, y_edges) {
        (Some((min_x, max_x)), Some((min_y, max_y))) => Bounds {
            min_x,
            min_y,
            max_x,
            max_y,
            width: max_x - min_x,
            height: max_y - min_y,
        },
        _ => Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
            width: 0.0,
            height: 0.0,
        },
    }
}

fn span_fits(span: f64, viewport_limit: f64) -> bool {
    span <= viewport_limit + FIT_TOLERANCE_PT
}

fn span_acceptable(span: f64, viewport_limit: f64) -> bool {
    span <= viewport_limit + FIT_ACCEPTANCE_TOLERANCE_PT
}

fn solve_axis_scale(
    tree_extent: f64,
    solve_descriptors: &[SolveDescriptor],
    viewport_limit: f64,
    fit_band_samples: usize,
    fit_max_bands: usize,
) -> f64 {
    if tree_extent <= 0.0 {
        return 0.0;
    }

    let mut best_fit = None::<f64>;
    let mut band_left = 0.0;
    let mut band_right = 1.0;

    for _ in 0..fit_max_bands {
        let mut last_fit = None::<f64>;
        let mut first_fail_after_fit = None::<f64>;

        for sample in 0..=fit_band_samples {
            let t = sample as f64 / fit_band_samples as f64;
            let scale = band_left + (band_right - band_left) * t;
            if span_fits(
                evaluate_solve_span(solve_descriptors, scale),
                viewport_limit,
            ) {
                best_fit = Some(scale);
                last_fit = Some(scale);
            } else if last_fit.is_some() && first_fail_after_fit.is_none() {
                first_fail_after_fit = Some(scale);
            }
        }

        if let (Some(mut low), Some(mut high)) = (last_fit, first_fail_after_fit) {
            for _ in 0..48 {
                let mid = (low + high) / 2.0;
                if span_fits(evaluate_solve_span(solve_descriptors, mid), viewport_limit) {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            return low;
        }

        if let Some(last_fit) = last_fit {
            best_fit = Some(last_fit);
        }

        band_left = band_right;
        band_right *= 2.0;
    }

    best_fit.unwrap_or(0.0)
}

fn solve_uniform_scale(
    viewport_width: f64,
    viewport_height: f64,
    fit_max_bands: usize,
    bounds_descriptors: UniformBoundsDescriptorsRef<'_>,
) -> f64 {
    let fits = |scale: f64| {
        let bounds = evaluate_uniform_bounds(bounds_descriptors, scale);
        span_fits(bounds.width, viewport_width) && span_fits(bounds.height, viewport_height)
    };

    let mut best_fit = None::<f64>;
    let mut band_left = 0.0;
    let mut band_right = 1.0;

    for _ in 0..fit_max_bands {
        let mut last_fit = None::<f64>;
        let mut first_fail_after_fit = None::<f64>;

        for scale in [band_left, band_right] {
            if fits(scale) {
                best_fit = Some(scale);
                last_fit = Some(scale);
            } else if last_fit.is_some() && first_fail_after_fit.is_none() {
                first_fail_after_fit = Some(scale);
            }
        }

        if let (Some(mut low), Some(mut high)) = (last_fit, first_fail_after_fit) {
            for _ in 0..48 {
                let mid = (low + high) / 2.0;
                if fits(mid) {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            return low;
        }

        if let Some(last_fit) = last_fit {
            best_fit = Some(last_fit);
        }

        band_left = band_right;
        band_right *= 2.0;
    }

    best_fit.unwrap_or(0.0)
}

fn rotation_distance_half_turn(rotation_half_turn: f64) -> f64 {
    let wrapped = rotation_half_turn.rem_euclid(2.0);
    wrapped.min(2.0 - wrapped)
}

fn uniform_rotation_objective(width_mode: WidthMode, height_mode: HeightMode) -> RotationObjective {
    if width_mode == WidthMode::Provisional
        || (width_mode == WidthMode::Auto && height_mode == HeightMode::Auto)
    {
        RotationObjective::None
    } else if width_mode == WidthMode::Auto {
        RotationObjective::CompactWidth
    } else if height_mode == HeightMode::Auto {
        RotationObjective::CompactHeight
    } else {
        RotationObjective::Area
    }
}

fn rotation_candidate_is_better(
    objective: RotationObjective,
    candidate: &RotationCandidate,
    best: &RotationCandidate,
) -> bool {
    let candidate_bounds = candidate.evaluated_fit.tree_occupied_bounds;
    let best_bounds = best.evaluated_fit.tree_occupied_bounds;
    let candidate_distance = rotation_distance_half_turn(candidate.rotation_half_turn);
    let best_distance = rotation_distance_half_turn(best.rotation_half_turn);

    match objective {
        RotationObjective::Area => {
            let candidate_fill_x = candidate_bounds.width / candidate.evaluated_fit.viewport_width;
            let candidate_fill_y =
                candidate_bounds.height / candidate.evaluated_fit.viewport_height;
            let best_fill_x = best_bounds.width / best.evaluated_fit.viewport_width;
            let best_fill_y = best_bounds.height / best.evaluated_fit.viewport_height;
            let candidate_area = candidate_fill_x * candidate_fill_y;
            let best_area = best_fill_x * best_fill_y;

            if (candidate_area - best_area).abs() > ROTATION_SCORE_TOLERANCE {
                candidate_area > best_area
            } else {
                let candidate_min_fill = candidate_fill_x.min(candidate_fill_y);
                let best_min_fill = best_fill_x.min(best_fill_y);
                if (candidate_min_fill - best_min_fill).abs() > ROTATION_SCORE_TOLERANCE {
                    candidate_min_fill > best_min_fill
                } else if (candidate.evaluated_fit.x_scale - best.evaluated_fit.x_scale).abs()
                    > FIT_TOLERANCE_PT
                {
                    candidate.evaluated_fit.x_scale > best.evaluated_fit.x_scale
                } else {
                    candidate_distance < best_distance
                }
            }
        }
        RotationObjective::CompactHeight => {
            if (candidate_bounds.height - best_bounds.height).abs() > FIT_TOLERANCE_PT {
                candidate_bounds.height < best_bounds.height
            } else if (candidate.evaluated_fit.x_scale - best.evaluated_fit.x_scale).abs()
                > FIT_TOLERANCE_PT
            {
                candidate.evaluated_fit.x_scale > best.evaluated_fit.x_scale
            } else {
                candidate_distance < best_distance
            }
        }
        RotationObjective::CompactWidth => {
            if (candidate_bounds.width - best_bounds.width).abs() > FIT_TOLERANCE_PT {
                candidate_bounds.width < best_bounds.width
            } else if (candidate.evaluated_fit.x_scale - best.evaluated_fit.x_scale).abs()
                > FIT_TOLERANCE_PT
            {
                candidate.evaluated_fit.x_scale > best.evaluated_fit.x_scale
            } else {
                candidate_distance < best_distance
            }
        }
        RotationObjective::None => false,
    }
}

fn evaluate_uniform_fit(
    request: &FitRequest,
    fit_inputs: &FitInputs,
    viewport_policy: UniformViewportPolicy,
) -> EvaluatedFit {
    let bounds_descriptors = build_uniform_bounds_descriptors(
        &fit_inputs.prepared_lines,
        &fit_inputs.prepared_labels,
        request.orientation,
    );
    evaluate_uniform_fit_from_bounds_descriptors(
        request.fit_max_bands,
        bounds_descriptors.as_descriptors(),
        viewport_policy,
    )
}

impl UniformViewportPolicy {
    fn from_request(request: &FitRequest, auto_height: f64) -> Self {
        match request.width_mode {
            WidthMode::Auto => Self::AutoWidth {
                viewport_height: auto_height,
            },
            WidthMode::Resolved | WidthMode::Provisional => {
                let width_unresolved = request.width_mode == WidthMode::Provisional;
                let viewport_width = if width_unresolved {
                    0.0
                } else {
                    request
                        .viewport_width_pt
                        .expect("validated resolved-width requests must include viewport_width_pt")
                };
                if request.height_mode == HeightMode::Auto && !width_unresolved {
                    Self::ResolvedWidthAutoHeight { viewport_width }
                } else {
                    Self::Constrained {
                        width_unresolved,
                        viewport_width,
                        viewport_height: auto_height,
                    }
                }
            }
        }
    }

    fn solve_limits(self) -> (f64, f64) {
        match self {
            Self::AutoWidth { viewport_height } => (f64::INFINITY, viewport_height),
            Self::ResolvedWidthAutoHeight { viewport_width } => (viewport_width, f64::INFINITY),
            Self::Constrained {
                width_unresolved,
                viewport_width,
                viewport_height,
            } => (
                if width_unresolved {
                    f64::INFINITY
                } else {
                    viewport_width
                },
                viewport_height,
            ),
        }
    }

    fn finalize(self, bounds: Bounds) -> UniformViewport {
        match self {
            Self::AutoWidth { viewport_height } => UniformViewport {
                width_unresolved: false,
                viewport_width: bounds.width,
                viewport_height,
            },
            Self::ResolvedWidthAutoHeight { viewport_width } => UniformViewport {
                width_unresolved: false,
                viewport_width,
                viewport_height: bounds.height,
            },
            Self::Constrained {
                width_unresolved,
                viewport_width,
                viewport_height,
            } => UniformViewport {
                width_unresolved,
                viewport_width,
                viewport_height,
            },
        }
    }
}

fn evaluate_uniform_fit_from_bounds_descriptors(
    fit_max_bands: usize,
    bounds_descriptors: UniformBoundsDescriptorsRef<'_>,
    viewport_policy: UniformViewportPolicy,
) -> EvaluatedFit {
    let (viewport_width_limit, viewport_height_limit) = viewport_policy.solve_limits();
    let scale = solve_uniform_scale(
        viewport_width_limit,
        viewport_height_limit,
        fit_max_bands,
        bounds_descriptors,
    );
    let bounds = evaluate_uniform_bounds(bounds_descriptors, scale);
    let viewport = viewport_policy.finalize(bounds);
    EvaluatedFit {
        viewport_width: viewport.viewport_width,
        viewport_height: viewport.viewport_height,
        x_scale: scale,
        y_scale: scale,
        tree_occupied_bounds: bounds,
    }
}

fn materialize_uniform(
    fit_inputs: &FitInputs,
    orientation: Orientation,
    viewport_policy: UniformViewportPolicy,
    evaluated_fit: EvaluatedFit,
) -> FittedWidth {
    let materialized_tree = materialize_fitted_tree(
        &fit_inputs.prepared_lines,
        &fit_inputs.prepared_labels,
        fit_inputs.root_tree_point,
        evaluated_fit.x_scale,
        evaluated_fit.y_scale,
        orientation,
    );
    let viewport = viewport_policy.finalize(materialized_tree.tree_occupied_bounds);
    FittedWidth {
        width_unresolved: viewport.width_unresolved,
        viewport_width: viewport.viewport_width,
        viewport_height: viewport.viewport_height,
        x_scale: evaluated_fit.x_scale,
        materialized_tree,
    }
}

fn evaluate_materialized_uniform_fit(
    fit_inputs: &FitInputs,
    orientation: Orientation,
    viewport_policy: UniformViewportPolicy,
    evaluated_fit: EvaluatedFit,
) -> EvaluatedFit {
    let materialized_tree = materialize_fitted_tree(
        &fit_inputs.prepared_lines,
        &fit_inputs.prepared_labels,
        fit_inputs.root_tree_point,
        evaluated_fit.x_scale,
        evaluated_fit.y_scale,
        orientation,
    );
    let viewport = viewport_policy.finalize(materialized_tree.tree_occupied_bounds);
    EvaluatedFit {
        viewport_width: viewport.viewport_width,
        viewport_height: viewport.viewport_height,
        x_scale: evaluated_fit.x_scale,
        y_scale: evaluated_fit.y_scale,
        tree_occupied_bounds: materialized_tree.tree_occupied_bounds,
    }
}

fn evaluate_uniform_rotation_candidate(
    request: &FitRequest,
    fit_inputs: &FitInputs,
    viewport_policy: UniformViewportPolicy,
    rotation: RotationTransform,
) -> RotationCandidate {
    let rotated_inputs = rotation.rotate_fit_inputs(fit_inputs);
    let evaluated_fit = evaluate_uniform_fit(request, &rotated_inputs, viewport_policy);
    RotationCandidate {
        rotation_half_turn: rotation.rotation_half_turn,
        evaluated_fit: evaluate_materialized_uniform_fit(
            &rotated_inputs,
            request.orientation,
            viewport_policy,
            evaluated_fit,
        ),
    }
}

fn fit_tree_plan_uniform(
    request: &FitRequest,
    fit_inputs: &FitInputs,
    auto_height: f64,
) -> FittedWidth {
    let viewport_policy = UniformViewportPolicy::from_request(request, auto_height);
    let rotation_objective = uniform_rotation_objective(request.width_mode, request.height_mode);
    if !request.optimize_uniform_rotation
        || rotation_objective == RotationObjective::None
        || request.layout_kind == LayoutKind::Rectangular
    {
        return materialize_uniform(
            fit_inputs,
            request.orientation,
            viewport_policy,
            evaluate_uniform_fit(request, fit_inputs, viewport_policy),
        );
    }

    let mut best: Option<RotationCandidate> = None;
    for index in 0..36 {
        let rotation = RotationTransform::new((index as f64 * (10.0 / 180.0)).rem_euclid(2.0));
        let candidate =
            evaluate_uniform_rotation_candidate(request, fit_inputs, viewport_policy, rotation);
        if best
            .as_ref()
            .map(|current| rotation_candidate_is_better(rotation_objective, &candidate, current))
            .unwrap_or(true)
        {
            best = Some(candidate);
        }
    }

    let mut best = best.expect("rotation search must produce at least one candidate");
    for step in [5.0, 2.5, 1.25] {
        let delta = step / 180.0;
        let best_rotation = best.rotation_half_turn;
        for offset in [-2.0, -1.0, 0.0, 1.0, 2.0] {
            let rotation = RotationTransform::new((best_rotation + offset * delta).rem_euclid(2.0));
            let candidate =
                evaluate_uniform_rotation_candidate(request, fit_inputs, viewport_policy, rotation);
            if rotation_candidate_is_better(rotation_objective, &candidate, &best) {
                best = candidate;
            }
        }
    }

    let rotated_inputs =
        RotationTransform::new(best.rotation_half_turn).rotate_fit_inputs(fit_inputs);
    let evaluated_fit = evaluate_uniform_fit(request, &rotated_inputs, viewport_policy);
    materialize_uniform(
        &rotated_inputs,
        request.orientation,
        viewport_policy,
        evaluated_fit,
    )
}

fn fit_tree_plan_independent_axes(
    request: &FitRequest,
    fit_inputs: &FitInputs,
    auto_height: f64,
) -> Result<FittedWidth, String> {
    let fit_band_samples = request
        .fit_band_samples
        .expect("validated independent-axis requests must include fit_band_samples");
    let solve_descriptors = build_solve_descriptors(
        &fit_inputs.prepared_lines,
        &fit_inputs.prepared_labels,
        request.orientation,
    )?;
    let viewport_height = auto_height;

    Ok(match request.width_mode {
        WidthMode::Auto => {
            let intrinsic_scale = solve_axis_scale(
                if request.orientation == Orientation::Vertical {
                    fit_inputs.tree_depth
                } else {
                    fit_inputs.tree_height
                },
                if request.orientation == Orientation::Vertical {
                    &solve_descriptors.depth
                } else {
                    &solve_descriptors.spread
                },
                viewport_height,
                fit_band_samples,
                request.fit_max_bands,
            );
            let materialized_tree = materialize_fitted_tree(
                &fit_inputs.prepared_lines,
                &fit_inputs.prepared_labels,
                fit_inputs.root_tree_point,
                intrinsic_scale,
                intrinsic_scale,
                request.orientation,
            );
            FittedWidth {
                width_unresolved: false,
                viewport_width: materialized_tree.tree_occupied_bounds.width,
                viewport_height,
                x_scale: intrinsic_scale,
                materialized_tree,
            }
        }
        WidthMode::Resolved | WidthMode::Provisional => {
            let width_unresolved = request.width_mode == WidthMode::Provisional;
            let viewport_width = if width_unresolved {
                0.0
            } else {
                request
                    .viewport_width_pt
                    .expect("validated resolved-width requests must include viewport_width_pt")
            };
            let x_scale = solve_axis_scale(
                fit_inputs.tree_depth,
                &solve_descriptors.depth,
                if request.orientation == Orientation::Vertical {
                    viewport_height
                } else {
                    viewport_width
                },
                fit_band_samples,
                request.fit_max_bands,
            );
            let y_scale = solve_axis_scale(
                fit_inputs.tree_height,
                &solve_descriptors.spread,
                if request.orientation == Orientation::Vertical {
                    viewport_width
                } else {
                    viewport_height
                },
                fit_band_samples,
                request.fit_max_bands,
            );
            let materialized_tree = materialize_fitted_tree(
                &fit_inputs.prepared_lines,
                &fit_inputs.prepared_labels,
                fit_inputs.root_tree_point,
                x_scale,
                y_scale,
                request.orientation,
            );
            FittedWidth {
                width_unresolved,
                viewport_width,
                viewport_height,
                x_scale,
                materialized_tree,
            }
        }
    })
}

fn finalize_fitted_tree_plan(
    fit_mode: FitMode,
    fitted_width: FittedWidth,
) -> Result<FitResponse, String> {
    let width_unresolved = fitted_width.width_unresolved;
    let viewport_width = fitted_width.viewport_width;
    let viewport_height = fitted_width.viewport_height;
    let materialized_tree = fitted_width.materialized_tree;
    let occupied_bounds = materialized_tree.tree_occupied_bounds;

    let mut issues = Vec::new();
    if !width_unresolved && !span_acceptable(occupied_bounds.width, viewport_width) {
        issues.push(format!(
            "width is too small for the tree labels and fixed margins (current: {}, required: >= {})",
            format_pt(viewport_width),
            format_pt(occupied_bounds.width)
        ));
    }
    if !span_acceptable(occupied_bounds.height, viewport_height) {
        issues.push(format!(
            "height is too small for the tree labels and fixed margins (current: {}, required: >= {})",
            format_pt(viewport_height),
            format_pt(occupied_bounds.height)
        ));
    }
    if !issues.is_empty() {
        let suffix = if fit_mode == FitMode::Uniform {
            ". Increase width or height, reduce labels, or reduce label size."
        } else {
            ". Increase width or height, reduce labels, reduce label size, or reduce root-length."
        };
        return Err(format!(
            "Tree cannot be rendered: {}{}",
            issues.join("; "),
            suffix
        ));
    }

    let translate_x = if width_unresolved {
        -occupied_bounds.min_x
    } else {
        (viewport_width - occupied_bounds.width) / 2.0 - occupied_bounds.min_x
    };
    let translate_y = (viewport_height - occupied_bounds.height) / 2.0 - occupied_bounds.min_y;
    let tree_translation = Point {
        x: translate_x,
        y: translate_y,
    };

    Ok(FitResponse {
        width_unresolved,
        tree_viewport_width_pt: viewport_width,
        tree_viewport_height_pt: viewport_height,
        x_scale_pt: fitted_width.x_scale,
        tree_translation_pt: tree_translation,
        root_position_pt: Point {
            x: materialized_tree.root_position.x + translate_x,
            y: materialized_tree.root_position.y + translate_y,
        },
        tree_lines: materialized_tree
            .tree_lines
            .into_iter()
            .map(|line| SerializableLine {
                line_index: line.line_index,
                start_pt: line.start,
                end_pt: line.end,
            })
            .collect(),
        tree_labels: materialized_tree
            .tree_labels
            .into_iter()
            .map(|label| SerializableLabel {
                label_index: label.label_index,
                origin_pt: label.origin,
                rotation_deg: label.rotation_deg,
            })
            .collect(),
    })
}

/// WASM entry point for preparing a normalized tree layout.
///
/// # Arguments
/// * `config` - JSON-encoded [`PrepareLayoutRequest`] payload
///
/// # Returns
/// JSON bytes of [`LayoutTreeWire`] or an error string.
#[wasm_func]
pub fn prepare_layout(config: &[u8]) -> Result<Vec<u8>, String> {
    let request: PrepareLayoutRequest = serde_json::from_slice(config)
        .map_err(|e| format!("Invalid prepare-layout config JSON: {e}"))?;
    let normalized = normalize_raw_tree(
        request.tree_data,
        request.cladogram,
        request.suppress_unrooted,
        request.hide_internal_labels,
    )?;
    let layout = layout_normalized_tree(normalized, request.layout_kind)?;
    serde_json::to_vec(&layout_tree_to_wire(&layout))
        .map_err(|e| format!("Serialization failed: {e}"))
}

/// WASM entry point for parsing Newick input into a simplified tree structure.
///
/// # Arguments
/// * `input` - Newick source as UTF-8 bytes
///
/// # Returns
/// JSON bytes of [`ParseResult`] or an error string.
#[wasm_func]
pub fn parse_newick(input: &[u8]) -> Result<Vec<u8>, String> {
    let input_str = std::str::from_utf8(input)
        .map(|s| s.to_string())
        .map_err(|e| e.to_string())?;

    let tree = one_from_string(&input_str)
        .map_err(|_| format!("Failed to parse Newick string: {input_str}"))?;

    let root_id = tree.root();
    let is_rooted = tree
        .get(root_id)
        .map(|root_node| root_node.children().len() == 2)
        .unwrap_or(false);
    let simple_tree = convert_node_to_simple(&tree, root_id)?;

    let result = ParseResult {
        rooted: is_rooted,
        tree: simple_tree,
    };

    serde_json::to_vec(&result).map_err(|e| e.to_string())
}

/// WASM entry point for fitting a prepared tree plan into a viewport.
///
/// # Arguments
/// * `config` - JSON-encoded [`FitRequest`] payload
///
/// # Returns
/// JSON bytes of [`FitResponse`] or an error string.
#[wasm_func]
pub fn fit_tree(config: &[u8]) -> Result<Vec<u8>, String> {
    let request: FitRequest =
        serde_json::from_slice(config).map_err(|e| format!("Invalid fit config JSON: {e}"))?;
    request.validate()?;

    let fit_inputs = FitInputs::from(&request);
    let auto_height = if request.height_mode == HeightMode::Auto {
        let label_only_bounds = evaluate_tree_bounds_only(
            &fit_inputs.prepared_lines,
            &fit_inputs.prepared_labels,
            0.0,
            0.0,
            request.orientation,
        );
        request.auto_height_floor_pt.max(label_only_bounds.height)
    } else {
        request
            .viewport_height_pt
            .expect("validated resolved-height requests must include viewport_height_pt")
    };

    let fitted_width = match request.fit_mode {
        FitMode::Uniform => fit_tree_plan_uniform(&request, &fit_inputs, auto_height),
        FitMode::IndependentAxes => {
            fit_tree_plan_independent_axes(&request, &fit_inputs, auto_height)?
        }
    };

    let response = finalize_fitted_tree_plan(request.fit_mode, fitted_width)?;
    serde_json::to_vec(&response).map_err(|e| format!("Serialization failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    const HIDE_LABEL_REPRO_NEWICK: &str = "((A,B)ZetaInner,(C,D)AlphaInner)Root;";
    const SWAPPED_ROOT_ORDER_NEWICK: &str =
        "(((A,B)MinorPair,C)MinorClade,(((D,E)MajorPair,F)MajorInner,G,H)MajorClade)Root;";
    const SWAPPED_ROOT_ORDER_REVERSED_NEWICK: &str =
        "((((D,E)MajorPair,F)MajorInner,G,H)MajorClade,((A,B)MinorPair,C)MinorClade)Root;";

    fn parsed_tree_data(newick: &str) -> Value {
        serde_json::from_slice(
            &parse_newick(newick.as_bytes()).expect("Newick parsing should succeed"),
        )
        .expect("Parse result should deserialize into JSON")
    }

    fn move_names_to_label_ids(node: &mut Value) {
        let object = node
            .as_object_mut()
            .expect("tree nodes should be dictionaries");

        let label_id = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(std::string::ToString::to_string);
        if let Some(label_id) = label_id {
            object.insert("name".into(), Value::Null);
            object.insert("label-id".into(), Value::String(label_id));
        }

        if let Some(Value::Array(children)) = object.get_mut("children") {
            for child in children {
                move_names_to_label_ids(child);
            }
        }
    }

    fn parsed_tree_data_with_label_ids(newick: &str) -> Value {
        let mut value = parsed_tree_data(newick);
        move_names_to_label_ids(&mut value);
        value
    }

    fn normalized_unrooted_tree(newick: &str, hide_internal_labels: bool) -> NormalizedTreeData {
        normalize_raw_tree(parsed_tree_data(newick), false, true, hide_internal_labels)
            .expect("Tree normalization should succeed")
    }

    fn normalized_unrooted_tree_with_label_ids(
        newick: &str,
        hide_internal_labels: bool,
    ) -> NormalizedTreeData {
        normalize_raw_tree(
            parsed_tree_data_with_label_ids(newick),
            false,
            true,
            hide_internal_labels,
        )
        .expect("Tree normalization should succeed")
    }

    fn unrooted_layout(
        newick: &str,
        layout_kind: LayoutKind,
        hide_internal_labels: bool,
    ) -> LayoutTreeData {
        layout_normalized_tree(
            normalized_unrooted_tree(newick, hide_internal_labels),
            layout_kind,
        )
        .expect("Layout preparation should succeed")
    }

    fn unrooted_layout_with_label_ids(
        newick: &str,
        layout_kind: LayoutKind,
        hide_internal_labels: bool,
    ) -> LayoutTreeData {
        layout_normalized_tree(
            normalized_unrooted_tree_with_label_ids(newick, hide_internal_labels),
            layout_kind,
        )
        .expect("Layout preparation should succeed")
    }

    fn node_identity(node: &InternalNode) -> String {
        node.label_text
            .clone()
            .or_else(|| node.label_id.clone())
            .expect("nodes in these tests should keep an identity label")
    }

    fn collect_descendant_tip_labels(
        normalized: &NormalizedTreeData,
        node_id: usize,
        labels: &mut Vec<String>,
    ) {
        let node = &normalized.nodes[node_id];
        if node.is_leaf {
            labels.push(node_identity(node));
            return;
        }

        for &child_id in &node.children_ids {
            collect_descendant_tip_labels(normalized, child_id, labels);
        }
    }

    fn root_child_tip_sets(normalized: &NormalizedTreeData) -> Vec<Vec<String>> {
        let root = &normalized.nodes[normalized.root_id];
        root.children_ids
            .iter()
            .map(|&child_id| {
                let mut labels = Vec::new();
                collect_descendant_tip_labels(normalized, child_id, &mut labels);
                labels.sort();
                labels
            })
            .collect()
    }

    fn tip_labels(normalized: &NormalizedTreeData) -> Vec<String> {
        let mut labels = normalized
            .nodes
            .iter()
            .filter(|node| node.is_leaf)
            .map(node_identity)
            .collect::<Vec<_>>();
        labels.sort();
        labels
    }

    fn leaf_positions_by_label(layout: &LayoutTreeData) -> BTreeMap<String, (f64, f64)> {
        layout
            .normalized
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.is_leaf)
            .map(|(id, node)| {
                (
                    node_identity(node),
                    (layout.x_by_id[id], layout.y_by_id[id]),
                )
            })
            .collect()
    }

    fn assert_same_leaf_positions(first: &LayoutTreeData, second: &LayoutTreeData) {
        let first_positions = leaf_positions_by_label(first);
        let second_positions = leaf_positions_by_label(second);
        assert_eq!(
            first_positions.len(),
            second_positions.len(),
            "leaf-position maps should cover the same number of tips",
        );

        for (label, (first_x, first_y)) in first_positions {
            let (second_x, second_y) = second_positions
                .get(&label)
                .copied()
                .expect("every tip label should appear in both layouts");
            assert!(
                approx_eq(first_x, second_x, 1e-9),
                "x coordinate mismatch for {label}: {first_x} vs {second_x}",
            );
            assert!(
                approx_eq(first_y, second_y, 1e-9),
                "y coordinate mismatch for {label}: {first_y} vs {second_y}",
            );
        }
    }

    #[test]
    fn hide_internal_labels_preserves_suppressed_unrooted_structure() {
        let shown = normalized_unrooted_tree(HIDE_LABEL_REPRO_NEWICK, false);
        let hidden = normalized_unrooted_tree(HIDE_LABEL_REPRO_NEWICK, true);

        assert_eq!(root_child_tip_sets(&shown), root_child_tip_sets(&hidden));
        assert_eq!(tip_labels(&shown), tip_labels(&hidden));
        assert!(
            shown
                .nodes
                .iter()
                .filter(|node| !node.is_leaf)
                .any(|node| node.label_text.as_deref() == Some("AlphaInner"))
        );
        assert!(
            shown
                .nodes
                .iter()
                .filter(|node| !node.is_leaf)
                .any(|node| node.label_text.as_deref() == Some("ZetaInner"))
        );
        assert!(
            hidden
                .nodes
                .iter()
                .filter(|node| !node.is_leaf)
                .all(|node| node.label_text.is_none()),
            "hide-internal-labels should only clear non-leaf label text",
        );
    }

    #[test]
    fn hide_internal_labels_preserve_equal_angle_leaf_positions() {
        let shown = unrooted_layout(HIDE_LABEL_REPRO_NEWICK, LayoutKind::EqualAngle, false);
        let hidden = unrooted_layout(HIDE_LABEL_REPRO_NEWICK, LayoutKind::EqualAngle, true);

        assert_same_leaf_positions(&shown, &hidden);
    }

    #[test]
    fn hide_internal_labels_preserve_daylight_leaf_positions() {
        let shown = unrooted_layout(HIDE_LABEL_REPRO_NEWICK, LayoutKind::Daylight, false);
        let hidden = unrooted_layout(HIDE_LABEL_REPRO_NEWICK, LayoutKind::Daylight, true);

        assert_same_leaf_positions(&shown, &hidden);
    }

    #[test]
    fn swapped_root_children_preserve_equal_angle_leaf_positions() {
        let original = unrooted_layout(SWAPPED_ROOT_ORDER_NEWICK, LayoutKind::EqualAngle, false);
        let swapped = unrooted_layout(
            SWAPPED_ROOT_ORDER_REVERSED_NEWICK,
            LayoutKind::EqualAngle,
            false,
        );

        assert_same_leaf_positions(&original, &swapped);
    }

    #[test]
    fn swapped_root_children_preserve_daylight_leaf_positions() {
        let original = unrooted_layout(SWAPPED_ROOT_ORDER_NEWICK, LayoutKind::Daylight, false);
        let swapped = unrooted_layout(
            SWAPPED_ROOT_ORDER_REVERSED_NEWICK,
            LayoutKind::Daylight,
            false,
        );

        assert_same_leaf_positions(&original, &swapped);
    }

    #[test]
    fn hide_internal_labels_clear_content_backed_internal_ids() {
        let shown = normalized_unrooted_tree_with_label_ids(HIDE_LABEL_REPRO_NEWICK, false);
        let hidden = normalized_unrooted_tree_with_label_ids(HIDE_LABEL_REPRO_NEWICK, true);

        assert_eq!(root_child_tip_sets(&shown), root_child_tip_sets(&hidden));
        assert_eq!(tip_labels(&shown), tip_labels(&hidden));
        assert!(
            shown
                .nodes
                .iter()
                .filter(|node| !node.is_leaf)
                .any(|node| node.label_id.as_deref() == Some("AlphaInner"))
        );
        assert!(
            shown
                .nodes
                .iter()
                .filter(|node| !node.is_leaf)
                .any(|node| node.label_id.as_deref() == Some("ZetaInner"))
        );
        assert!(
            hidden
                .nodes
                .iter()
                .filter(|node| !node.is_leaf)
                .all(|node| node.label_id.is_none()),
            "hide-internal-labels should clear non-leaf label ids",
        );
    }

    #[test]
    fn content_backed_labels_preserve_equal_angle_leaf_positions_after_root_swap() {
        let original = unrooted_layout_with_label_ids(
            SWAPPED_ROOT_ORDER_NEWICK,
            LayoutKind::EqualAngle,
            false,
        );
        let swapped = unrooted_layout_with_label_ids(
            SWAPPED_ROOT_ORDER_REVERSED_NEWICK,
            LayoutKind::EqualAngle,
            false,
        );

        assert_same_leaf_positions(&original, &swapped);
    }

    #[test]
    fn content_backed_labels_preserve_daylight_leaf_positions_after_root_swap() {
        let original =
            unrooted_layout_with_label_ids(SWAPPED_ROOT_ORDER_NEWICK, LayoutKind::Daylight, false);
        let swapped = unrooted_layout_with_label_ids(
            SWAPPED_ROOT_ORDER_REVERSED_NEWICK,
            LayoutKind::Daylight,
            false,
        );

        assert_same_leaf_positions(&original, &swapped);
    }

    #[test]
    fn prepare_layout_wire_preserves_content_backed_label_ids() {
        let layout = normalize_raw_tree(
            json!({
                "rooted": true,
                "name": Value::Null,
                "label-id": "root-id",
                "length": Value::Null,
                "children": [
                    {
                        "name": Value::Null,
                        "label-id": "tip-a",
                        "length": 0.2,
                        "children": Value::Null,
                    },
                    {
                        "name": "TipB",
                        "length": 0.3,
                        "children": Value::Null,
                    },
                ],
            }),
            false,
            false,
            false,
        )
        .expect("Tree normalization should succeed");
        let layout = layout_tree_rectangular(layout);
        let wire = layout_tree_to_wire(&layout);

        assert_eq!(
            wire.nodes[wire.root_id].label_id.as_deref(),
            Some("root-id")
        );
        assert_eq!(wire.nodes[wire.root_id].label_text, None);
        assert_eq!(wire.nodes[1].label_id.as_deref(), Some("tip-a"));
        assert_eq!(wire.nodes[1].label_text, None);
        assert_eq!(wire.nodes[2].label_text.as_deref(), Some("TipB"));
        assert_eq!(wire.nodes[2].label_id, None);
    }
}
