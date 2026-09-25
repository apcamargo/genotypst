#import "../common/colors.typ": _medium-gray
#import "../common/layout_math.typ": (
  _assert-length, _render-width-is-valid, _resolve-length,
)
#import "../common/strokes.typ": (
  _default-axis-stroke, _default-branch-stroke, _default-tip-leader-stroke,
  _resolve-stroke,
)
#import "./tree_backend.typ": _tree-prepare-layout-backend
#import "./tree_fit.typ": _fit-prepared-tree-plan, _prepare-fit-tree-plan
#import "./tree_primitives.typ": _build-tree-plan
#import "./tree_render.typ": _build-scale-plan, _render-tree-plan

// `render-unrooted-tree` exposes a public `layout` parameter, so we keep a
// distinct handle to Typst's `layout(...)` function for both renderers.
#let _tree-render-layout = layout

/// Default outward spacing for tip labels.
#let _tip-label-gap = 0.315em

/// Vertical gap used for rectangular text internal labels.
#let _internal-text-y-gap = 0.17em
#let _auto-height-scale = 1.9em
#let _rectangular-fit-band-samples = 1
#let _tree-fit-max-bands = 24
#let _tree-content-label-id-prefix = "genotypst-content-label-"

/// Validates common tree rendering arguments shared by all tree renderers.
///
/// Length arguments are validated later, in context, by
/// `_resolve-tree-lengths`.
///
/// - hide-internal-labels (bool): Whether internal labels are suppressed.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// -> none
#let _validate-common-tree-args(hide-internal-labels, cladogram) = {
  assert(type(cladogram) == bool, message: "cladogram must be a boolean.")
  assert(
    type(hide-internal-labels) == bool,
    message: "hide-internal-labels must be a boolean.",
  )
}

/// Validates the length arguments of a tree render config and resolves them to
/// absolute lengths.
///
/// Lengths may mix em and absolute parts, so this must be called in context.
///
/// - config (dictionary): Canonical tree render config.
/// -> dictionary
#let _resolve-tree-lengths(config) = {
  assert(
    _render-width-is-valid(config.width),
    message: "width must be auto or a positive length, ratio, or relative width.",
  )
  if config.height != auto {
    config.height = _assert-length(
      config.height,
      "height must be auto or a positive length.",
      positive: true,
    )
  }
  config.internal-label-size = _assert-length(
    config.internal-label-size,
    "internal-label-size must be positive.",
    positive: true,
  )
  // Unrooted configs carry inert placeholders for these arguments.
  if config.layout-kind == "rectangular" {
    config.root-length = _assert-length(
      config.root-length,
      "root-length must be non-negative.",
    )
  }
  if config.scale-bar {
    config.scale-bar-gap = _assert-length(
      config.scale-bar-gap,
      "scale-bar-gap must be non-negative.",
    )
    config.scale-tick-height = _assert-length(
      config.scale-tick-height,
      "scale-tick-height must be positive.",
      positive: true,
    )
    config.scale-label-size = _assert-length(
      config.scale-label-size,
      "scale-label-size must be positive.",
      positive: true,
    )
  }
  config
}

/// Resolves the canonical rectangular-tree render configuration.
///
/// - config (dictionary): Raw public arguments of `render-rectangular-tree`.
/// -> dictionary with keys:
///   - layout-kind (str): Backend layout identifier.
///   - orientation (str): Final render orientation.
///   - suppress-unrooted (bool): Whether the backend suppresses the artificial
///     rooted handle used for unrooted trees.
///   - cladogram (bool): Whether cladogram mode is enabled.
///   - hide-internal-labels (bool): Whether internal labels are omitted.
///   - scale-bar (bool): Whether scale-bar rendering is enabled.
///   - optimize-uniform-rotation (bool): Whether fit may optimize global rotation.
///   - fit-band-samples (int, none): Band samples used by the fit search.
///   - align-tip-labels (bool): Resolved tip-label alignment setting.
///   - width, height, branch-stroke, tip-label-color, tip-label-italics,
///     internal-label-size, internal-label-color, root-length,
///     tip-leader-stroke, scale-stroke, scale-length, unit,
///     min-auto-bar-width, scale-tick-height, scale-label-size, scale-bar-gap:
///     validated public arguments, forwarded for the style, fit, and scale-bar
///     stages.
#let _resolve-rectangular-tree-render-config(config) = {
  let (
    width,
    height,
    branch-stroke,
    tip-label-color,
    tip-label-italics,
    align-tip-labels,
    tip-leader-stroke,
    internal-label-size,
    internal-label-color,
    hide-internal-labels,
    root-length,
    orientation,
    cladogram,
    scale-bar,
    scale-length,
    unit,
    min-auto-bar-width,
    scale-stroke,
    scale-bar-gap,
    scale-tick-height,
    scale-label-size,
  ) = config

  _validate-common-tree-args(hide-internal-labels, cladogram)
  assert(type(scale-bar) == bool, message: "scale-bar must be a boolean.")
  assert(
    type(align-tip-labels) == bool,
    message: "align-tip-labels must be a boolean.",
  )
  assert(
    orientation in ("horizontal", "vertical"),
    message: "orientation must be 'horizontal' or 'vertical'.",
  )
  if scale-bar {
    assert(
      unit == none or type(unit) == str,
      message: "unit must be a string or none.",
    )
  }
  (
    layout-kind: "rectangular",
    orientation: orientation,
    suppress-unrooted: false,
    cladogram: cladogram,
    hide-internal-labels: hide-internal-labels,
    scale-bar: scale-bar,
    optimize-uniform-rotation: false,
    // Rectangular occupied span is monotone enough that one sample per band
    // keeps the search cheap without changing the generic solver structure.
    fit-band-samples: _rectangular-fit-band-samples,
    align-tip-labels: align-tip-labels,
    width: width,
    height: height,
    branch-stroke: branch-stroke,
    tip-label-color: tip-label-color,
    tip-label-italics: tip-label-italics,
    internal-label-size: internal-label-size,
    internal-label-color: internal-label-color,
    root-length: root-length,
    tip-leader-stroke: tip-leader-stroke,
    scale-stroke: scale-stroke,
    scale-length: scale-length,
    unit: unit,
    min-auto-bar-width: min-auto-bar-width,
    scale-tick-height: scale-tick-height,
    scale-label-size: scale-label-size,
    scale-bar-gap: scale-bar-gap,
  )
}

/// Resolves the canonical unrooted-tree render configuration.
///
/// - config (dictionary): Raw public arguments of `render-unrooted-tree`.
/// -> dictionary with keys:
///   - layout-kind (str): Backend layout identifier.
///   - orientation (str): Final render orientation.
///   - suppress-unrooted (bool): Whether the backend suppresses the artificial
///     rooted handle used for unrooted trees.
///   - cladogram (bool): Whether cladogram mode is enabled.
///   - hide-internal-labels (bool): Whether internal labels are omitted.
///   - scale-bar (bool): Whether scale-bar rendering is enabled.
///   - optimize-uniform-rotation (bool): Whether fit may optimize global rotation.
///   - fit-band-samples (int, none): Band samples used by the fit search.
///   - align-tip-labels (bool): Always `false`; unrooted layouts do not align
///     tip labels.
///   - width, height, branch-stroke, tip-label-color, tip-label-italics,
///     internal-label-size, internal-label-color: validated public arguments,
///     forwarded for the style and fit stages.
///   - root-length, tip-leader-stroke, scale-stroke, scale-length, unit,
///     min-auto-bar-width, scale-tick-height, scale-label-size, scale-bar-gap:
///     inert defaults; unrooted layouts draw no root edge, leader lines, or
///     scale bar. Present so this config carries the same key set as the
///     rectangular one, which `_render-tree` and its stages consume.
#let _resolve-unrooted-tree-render-config(config) = {
  let (
    width,
    height,
    branch-stroke,
    tip-label-color,
    tip-label-italics,
    internal-label-size,
    internal-label-color,
    hide-internal-labels,
    cladogram,
    layout,
  ) = config

  _validate-common-tree-args(hide-internal-labels, cladogram)
  assert(
    layout in ("equal-angle", "daylight"),
    message: "layout must be 'equal-angle' or 'daylight'.",
  )
  (
    layout-kind: layout,
    orientation: "horizontal",
    suppress-unrooted: true,
    cladogram: cladogram,
    hide-internal-labels: hide-internal-labels,
    scale-bar: false,
    optimize-uniform-rotation: true,
    fit-band-samples: none,
    align-tip-labels: false,
    width: width,
    height: height,
    branch-stroke: branch-stroke,
    tip-label-color: tip-label-color,
    tip-label-italics: tip-label-italics,
    internal-label-size: internal-label-size,
    internal-label-color: internal-label-color,
    root-length: none,
    tip-leader-stroke: none,
    scale-stroke: none,
    scale-length: auto,
    unit: none,
    min-auto-bar-width: 0pt,
    scale-tick-height: 0pt,
    scale-label-size: 0pt,
    scale-bar-gap: 0pt,
  )
}

/// Builds the shared style record for tree rendering and tip-label metrics.
///
/// - config (dictionary): Canonical tree render config. Reads `branch-stroke`,
///   `tip-label-color`, `tip-label-italics`, `internal-label-size`,
///   `internal-label-color`, and `root-length` (`none` for layouts that draw no
///   root edge).
/// -> dictionary
#let _build-render-tree-style(config) = {
  let (
    branch-stroke,
    tip-label-color,
    tip-label-italics,
    internal-label-size,
    internal-label-color,
    root-length,
    ..,
  ) = config

  // Fitting reserves half a stroke of bleed. Rectangular trees draw
  // horizontals and verticals as separate segments, which only meet flush at
  // corners with square caps.
  let resolved-branch-stroke = _resolve-stroke(
    branch-stroke,
    "branch-stroke",
    cap: "square",
  )
  let tip-label-style = if tip-label-italics { "italic" } else { "normal" }
  let ascender-to-baseline = measure(text(
    style: tip-label-style,
    top-edge: "ascender",
    bottom-edge: "baseline",
    "x",
  )).height
  let x-height-span = measure(text(
    style: tip-label-style,
    top-edge: "x-height",
    bottom-edge: "baseline",
    "x",
  )).height
  let full-height = measure(text(
    style: tip-label-style,
    top-edge: "ascender",
    bottom-edge: "descender",
    "x",
  )).height
  (
    branch-stroke: resolved-branch-stroke.style,
    // Every gap is resolved to an absolute length once here, so the per-node
    // fit pass never has to measure an em-relative value.
    branch-thickness: resolved-branch-stroke.thickness,
    tip-label-color: tip-label-color,
    tip-label-italics: tip-label-italics,
    tip-label-style: tip-label-style,
    internal-label-size: internal-label-size,
    internal-label-color: internal-label-color,
    tip-label-gap: _resolve-length(_tip-label-gap),
    internal-text-y-gap: _resolve-length(_internal-text-y-gap),
    auto-height-scale: _resolve-length(_auto-height-scale),
    root-length: root-length,
    tip-label-metrics: (
      // Rectangular and unrooted tip labels intentionally use the same
      // branch/text intersection height.
      branch-midpoint: ascender-to-baseline - x-height-span / 2,
      full-height: full-height,
    ),
  )
}

/// Rewrites manual tree labels into a backend-safe representation.
///
/// String labels pass through unchanged. Empty content labels are normalized to
/// `none`, and non-empty content labels are replaced with a private `label-id`
/// so the Rust backend never has to serialize Typst `content` values.
///
/// - tree-data (dictionary): Parsed or manually constructed tree data.
/// -> dictionary with keys:
///   - backend-tree (dictionary): Backend-safe tree data.
///   - content-labels (dictionary): `label-id` to original content.
#let _prepare-tree-data-for-layout(tree-data) = {
  assert(
    type(tree-data) == dictionary,
    message: "tree-data must be a dictionary.",
  )

  let visit(node, next-label-id) = {
    assert(
      type(node) == dictionary,
      message: "tree nodes must be dictionaries.",
    )
    // All other keys pass through untouched; `label-id` is a private backend
    // channel, so a user-supplied one is always dropped.
    let prepared = node
    let _ = prepared.remove("label-id", default: none)
    let next-id = next-label-id
    let content-label-pairs = ()

    if "name" in prepared {
      let value = prepared.name
      if value == none or type(value) == str {
        // Already backend-safe.
      } else if type(value) == content {
        prepared.insert("name", none)
        // Empty content renders nothing and should not affect layout.
        if value != [] {
          let label-id = _tree-content-label-id-prefix + str(next-id)
          next-id += 1
          prepared.insert("label-id", label-id)
          content-label-pairs.push((label-id, value))
        }
      } else {
        assert(
          false,
          message: "manual tree node name must be a string, content, or none.",
        )
      }
    }

    if "children" in prepared {
      let value = prepared.children
      assert(
        value == none or type(value) == array,
        message: "children must be an array or none.",
      )
      if value != none {
        let children = ()
        for child in value {
          let prepared-child = visit(child, next-id)
          next-id = prepared-child.next-label-id
          // Collected flat and folded into a dictionary once at the root, so a
          // deep tree does not recopy the accumulated labels per child.
          content-label-pairs += prepared-child.content-label-pairs
          children.push(prepared-child.node)
        }
        prepared.insert("children", children)
      }
    }

    (
      node: prepared,
      content-label-pairs: content-label-pairs,
      next-label-id: next-id,
    )
  }

  let prepared-root = visit(tree-data, 0)
  let content-labels = (:)
  for (label-id, value) in prepared-root.content-label-pairs {
    content-labels.insert(label-id, value)
  }

  (
    backend-tree: prepared-root.node,
    content-labels: content-labels,
  )
}

/// Restores content-backed labels onto the prepared layout tree.
///
/// - layout-tree (dictionary): Backend-prepared normalized tree layout.
/// - content-labels (dictionary): `label-id` to original content.
/// -> dictionary
#let _hydrate-layout-tree-label-bodies(layout-tree, content-labels) = (
  ..layout-tree,
  nodes: layout-tree.nodes.map(node => {
    let label-id = node.at("label-id", default: none)
    node.insert(
      "label-body",
      if label-id == none { node.label-text } else {
        assert(
          label-id in content-labels,
          message: "Internal tree label hydration failed.",
        )
        content-labels.at(label-id)
      },
    )
    node
  }),
)

/// Prepares a tree render from the resolved mode config.
///
/// - tree-data (dictionary): Parsed or manually constructed tree data.
/// - style (dictionary): Tree style record.
/// - config (dictionary): Canonical tree render config from one of the
///   `_resolve-*-tree-render-config(...)` helpers.
/// -> dictionary with keys:
///   - style (dictionary): Tree style record passed into the helper.
///   - prepared-fit-plan (dictionary): Prepared fit payload for the tree-fitting
///     stage.
#let _prepare-tree-render(tree-data, style, config) = {
  let prepared-tree-data = _prepare-tree-data-for-layout(tree-data)
  let layout-tree = _tree-prepare-layout-backend(
    prepared-tree-data.backend-tree,
    cladogram: config.cladogram,
    suppress-unrooted: config.suppress-unrooted,
    hide-internal-labels: config.hide-internal-labels,
    layout-kind: config.layout-kind,
  )
  if config.scale-bar {
    assert(
      not layout-tree.effective-cladogram,
      message: "scale-bar cannot be used when the tree has no branch length information or when it is rendered as a cladogram.",
    )
  }
  let tree-plan = _build-tree-plan(
    _hydrate-layout-tree-label-bodies(
      layout-tree,
      prepared-tree-data.content-labels,
    ),
    style,
    orientation: config.orientation,
  )
  (
    style: style,
    prepared-fit-plan: _prepare-fit-tree-plan(tree-plan),
  )
}

/// Inserts leader lines for already aligned tip labels in a fitted tree plan.
///
/// - fitted-plan (dictionary): Fitted tree layout plan containing lines and labels.
/// - tip-leader-stroke (stroke, none): Stroke for the leader lines.
/// -> dictionary
#let _align-tip-labels-in-plan(fitted-plan, tip-leader-stroke) = {
  if tip-leader-stroke == none {
    return fitted-plan
  }

  let orientation = fitted-plan.orientation
  let tip-labels = fitted-plan.tree-labels.filter(l => (
    l.at("placement-role", default: none) == "tip-label"
  ))
  if tip-labels.len() == 0 {
    return fitted-plan
  }

  let aligned-tip-coord = if orientation == "horizontal" {
    calc.max(..tip-labels.map(l => l.anchor.x))
  } else {
    calc.min(..tip-labels.map(l => l.anchor.y))
  }

  // The leader runs from the label anchor to the shared alignment coordinate;
  // only the axis it runs along differs by orientation.
  let leader-end = l => if orientation == "horizontal" {
    (x: aligned-tip-coord, y: l.anchor.y)
  } else {
    (x: l.anchor.x, y: aligned-tip-coord)
  }
  let leader-padding = l => if orientation == "horizontal" {
    aligned-tip-coord - l.anchor.x
  } else {
    l.anchor.y - aligned-tip-coord
  }

  let leader-lines = tip-labels
    .filter(l => leader-padding(l) > 1e-3pt)
    .map(l => (start: l.anchor, end: leader-end(l), stroke: tip-leader-stroke))

  fitted-plan.insert("tree-lines", leader-lines + fitted-plan.tree-lines)
  fitted-plan
}


/// Renders a tree from a resolved render config.
///
/// Shared by both public tree renderers: the layout kind, styling, tip-label
/// alignment, and scale bar are all driven by `config`.
///
/// - tree-data (dictionary): Parsed or manually constructed tree data.
/// - config (dictionary): Canonical tree render config.
/// -> content
#let _render-tree(tree-data, config) = context {
  let config = _resolve-tree-lengths(config)
  block(width: config.width, {
    let style = _build-render-tree-style(config)
    let prepared = _prepare-tree-render(tree-data, style, config)
    _tree-render-layout(size => context {
      let fitted-plan = _fit-prepared-tree-plan(
        prepared.prepared-fit-plan,
        prepared.style,
        config.orientation,
        config.width,
        config.height,
        size,
        _tree-fit-max-bands,
        fit-band-samples: config.fit-band-samples,
        optimize-uniform-rotation: config.optimize-uniform-rotation,
        align-tip-labels: config.align-tip-labels,
      )
      let fitted-plan = if config.align-tip-labels {
        _align-tip-labels-in-plan(fitted-plan, config.tip-leader-stroke)
      } else {
        fitted-plan
      }
      let scale-plan = if (
        config.scale-bar and not fitted-plan.width-unresolved
      ) {
        _build-scale-plan(fitted-plan, config)
      } else {
        none
      }
      _render-tree-plan(fitted-plan, scale-plan, config.scale-bar-gap)
    })
  })
}

/// Renders a rectangular phylogenetic tree from parsed or manual tree data.
///
/// Supports customization of dimensions, styling, and orientation.
///
/// - tree-data (dictionary): Parsed or manually constructed tree data. Manual
///   node dictionaries accept `name: str`, `name: content`, or `name: none`.
///   Trees returned by `parse-newick(...)` remain string-labeled.
/// - width (length, auto, ratio, relative): Width of the tree visualization
///   including labels (default: 100%).
/// - height (length, auto): Height of the tree area (default: auto).
/// - branch-stroke (stroke, none): Stroke for tree branches. `none` hides them (default: 0.75pt black, square caps).
/// - tip-label-color (color, none): Color of tip labels (default: none, inherits from the document).
/// - tip-label-italics (bool): Whether to use italics for tip labels (default: false).
/// - align-tip-labels (bool): Whether to align tip labels and connect them to branches with leader lines (default: false).
/// - tip-leader-stroke (stroke, none): Stroke for the tip-label leader lines. `none` hides them (default: 0.75pt medium gray, dashed, square caps).
/// - internal-label-size (length): Font size of internal node labels (default: 0.85em).
/// - internal-label-color (color, none): Color of internal node labels (default: medium gray; `none` inherits from the document).
/// - hide-internal-labels (bool): Whether to hide all non-leaf labels
///   (default: false).
/// - root-length (length): Length of the rendered root edge (default: 1.2em).
/// - orientation (str): "horizontal" (root left, tips right) or "vertical" (root bottom, tips up) (default: "horizontal").
/// - cladogram (bool): Whether to draw the tree as a cladogram with equal branch lengths (default: false).
/// - scale-bar (bool): Whether to draw a branch-length scale bar below the tree (default: false).
///   Scale bars are unavailable for cladograms and for trees that fall back to
///   cladogram rendering because they lack branch length information.
///   In vertical orientation, the scale bar can use the full rendered row width.
/// - scale-length (auto, int, float): Scale-bar length in branch-length units. Positive when specified (default: auto).
/// - unit (str, none): Optional scale-bar unit suffix (default: none).
/// - min-auto-bar-width (length): Minimum auto-selected scale-bar width when space allows (default: 2.5em).
/// - scale-stroke (stroke, none): Stroke for the scale-bar line and ticks. `none` hides them (default: 0.75pt black, butt caps).
/// - scale-bar-gap (length): Gap between tree and scale bar (default: 0.6em).
/// - scale-tick-height (length): Scale-bar tick height (default: 5pt).
/// - scale-label-size (length): Scale-bar label size (default: 0.85em).
/// -> content
#let render-rectangular-tree(
  tree-data,
  width: 100%,
  height: auto,
  branch-stroke: _default-branch-stroke,
  tip-label-color: none,
  tip-label-italics: false,
  align-tip-labels: false,
  tip-leader-stroke: _default-tip-leader-stroke,
  internal-label-size: 0.85em,
  internal-label-color: _medium-gray,
  hide-internal-labels: false,
  root-length: 1.2em,
  orientation: "horizontal",
  cladogram: false,
  scale-bar: false,
  scale-length: auto,
  unit: none,
  min-auto-bar-width: 2.5em,
  scale-stroke: _default-axis-stroke,
  scale-bar-gap: 0.6em,
  scale-tick-height: 5pt,
  scale-label-size: 0.85em,
) = {
  let config = _resolve-rectangular-tree-render-config((
    width: width,
    height: height,
    branch-stroke: branch-stroke,
    tip-label-color: tip-label-color,
    tip-label-italics: tip-label-italics,
    align-tip-labels: align-tip-labels,
    tip-leader-stroke: tip-leader-stroke,
    internal-label-size: internal-label-size,
    internal-label-color: internal-label-color,
    hide-internal-labels: hide-internal-labels,
    root-length: root-length,
    orientation: orientation,
    cladogram: cladogram,
    scale-bar: scale-bar,
    scale-length: scale-length,
    unit: unit,
    min-auto-bar-width: min-auto-bar-width,
    scale-stroke: scale-stroke,
    scale-bar-gap: scale-bar-gap,
    scale-tick-height: scale-tick-height,
    scale-label-size: scale-label-size,
  ))
  _render-tree(tree-data, config)
}

/// Renders an unrooted phylogenetic tree using an equal-angle or daylight layout.
///
/// - tree-data (dictionary): Parsed or manually constructed tree data. Manual
///   node dictionaries accept `name: str`, `name: content`, or `name: none`.
///   Trees returned by `parse-newick(...)` remain string-labeled.
/// - width (length, auto, ratio, relative): Width of the tree visualization including labels (default: 100%).
/// - height (length, auto): Height of the tree area (default: auto).
/// - branch-stroke (stroke, none): Stroke for tree branches. `none` hides them (default: 0.75pt black, square caps).
/// - tip-label-color (color, none): Color of tip labels (default: none, inherits from the document).
/// - tip-label-italics (bool): Whether to use italics for tip labels (default: false).
/// - internal-label-size (length): Font size of internal node labels (default: 0.85em).
/// - internal-label-color (color, none): Color of internal node labels (default: medium gray; `none` inherits from the document).
/// - hide-internal-labels (bool): Whether to hide all non-leaf labels
///   (default: false).
/// - cladogram (bool): Whether to draw the tree as a cladogram with equal branch lengths (default: false).
/// - layout (str): "equal-angle" or "daylight" (default: "equal-angle").
/// -> content
#let render-unrooted-tree(
  tree-data,
  width: 100%,
  height: auto,
  branch-stroke: _default-branch-stroke,
  tip-label-color: none,
  tip-label-italics: false,
  internal-label-size: 0.85em,
  internal-label-color: _medium-gray,
  hide-internal-labels: false,
  cladogram: false,
  layout: "equal-angle",
) = {
  let config = _resolve-unrooted-tree-render-config((
    width: width,
    height: height,
    branch-stroke: branch-stroke,
    tip-label-color: tip-label-color,
    tip-label-italics: tip-label-italics,
    internal-label-size: internal-label-size,
    internal-label-color: internal-label-color,
    hide-internal-labels: hide-internal-labels,
    cladogram: cladogram,
    layout: layout,
  ))
  _render-tree(tree-data, config)
}
