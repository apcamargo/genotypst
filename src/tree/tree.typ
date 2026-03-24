#import "../common/colors.typ": _medium-gray
#import "./tree_composition.typ": (
  _build-scale-plan, _build-tree-plan, _fit-tree-plan, _render-tree-plan,
)
#import "./tree_layout.typ": _layout-tree, _normalize-tree

/// Tree layout constants.
#let _label-x-offset = 0.32em
#let _internal-label-gap = 0.42em
#let _auto-height-scale = 1.9em

/// Validates the `render-tree` arguments that affect layout and sizing.
///
/// - width (length, fraction): Requested rendered width.
/// - height (length, auto): Requested tree viewport height.
/// - branch-weight (length): Branch stroke thickness.
/// - tip-label-size (length): Tip label size.
/// - internal-label-size (length): Internal label size.
/// - root-length (length): Root-edge length.
/// - orientation (str): Tree orientation.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// - scale-bar (bool): Whether scale bar rendering is enabled.
/// - scale-unit (str, none): Optional scale unit string.
/// - scale-bar-gap (length): Gap between tree and scale bar.
/// - scale-tick-height (length): Scale bar tick height.
/// - scale-label-size (length): Scale bar label size.
/// - scale-label-gap (length): Gap between bar and scale label.
/// -> none
#let _validate-render-tree-args(
  width,
  height,
  branch-weight,
  tip-label-size,
  internal-label-size,
  root-length,
  orientation,
  cladogram,
  scale-bar,
  scale-unit,
  scale-bar-gap,
  scale-tick-height,
  scale-label-size,
  scale-label-gap,
) = {
  assert(type(cladogram) == bool, message: "cladogram must be a boolean")
  assert(type(scale-bar) == bool, message: "scale-bar must be a boolean")
  assert(branch-weight > 0pt, message: "branch-weight must be positive.")
  assert(tip-label-size > 0pt, message: "tip-label-size must be positive.")
  assert(
    internal-label-size > 0pt,
    message: "internal-label-size must be positive.",
  )
  assert(root-length >= 0pt, message: "root-length must be non-negative.")
  assert(
    if type(width) == fraction { width > 0fr } else { width > 0pt },
    message: "width must be positive.",
  )
  assert(
    height == auto or height > 0pt,
    message: "height must be auto or a positive length.",
  )
  assert(
    orientation in ("horizontal", "vertical"),
    message: "orientation must be 'horizontal' or 'vertical'",
  )
  if scale-bar {
    assert(
      scale-unit == none or type(scale-unit) == str,
      message: "scale-unit must be a string or none.",
    )
    assert(scale-bar-gap >= 0pt, message: "scale-bar-gap must be non-negative.")
    assert(
      scale-tick-height > 0pt,
      message: "scale-tick-height must be positive.",
    )
    assert(
      scale-label-size > 0pt,
      message: "scale-label-size must be positive.",
    )
    assert(
      scale-label-gap >= 0pt,
      message: "scale-label-gap must be non-negative.",
    )
  }
}

/// Builds the style record consumed by the private tree composition module.
///
/// - branch-weight (length): Branch stroke thickness.
/// - branch-color (color): Branch color.
/// - tip-label-size (length): Tip label size.
/// - tip-label-color (color): Tip label color.
/// - tip-label-italics (bool): Whether tip labels are italicized.
/// - internal-label-size (length): Internal label size.
/// - internal-label-color (color): Internal label color.
/// - root-length (length): Root-edge length.
/// -> dictionary
#let _build-render-tree-style(
  branch-weight,
  branch-color,
  tip-label-size,
  tip-label-color,
  tip-label-italics,
  internal-label-size,
  internal-label-color,
  root-length,
) = (
  branch-stroke: stroke(
    thickness: branch-weight,
    paint: branch-color,
    cap: "square",
  ),
  root-stroke: stroke(
    thickness: branch-weight,
    paint: branch-color,
    dash: "dotted",
    cap: "round",
  ),
  branch-color: branch-color,
  branch-weight: branch-weight,
  tip-label-size: tip-label-size,
  tip-label-color: tip-label-color,
  tip-label-italics: tip-label-italics,
  internal-label-size: internal-label-size,
  internal-label-color: internal-label-color,
  root-length: root-length,
  label-x-offset: _label-x-offset,
  internal-label-gap: _internal-label-gap,
  auto-height-scale: _auto-height-scale,
)

/// Prepares the normalized/layout/plan/fitted tree data for rendering.
///
/// Call this helper from within `context` and a `layout` callback because it
/// measures text and resolves fractional widths against `layout-size`.
///
/// - tree-data (dictionary): Parsed or manual tree data.
/// - width (length, fraction): Width of the rendered tree including labels.
/// - height (length, auto): Height of the tree viewport.
/// - branch-weight (length): Branch stroke thickness.
/// - branch-color (color): Branch color.
/// - tip-label-size (length): Tip label size.
/// - tip-label-color (color): Tip label color.
/// - tip-label-italics (bool): Whether tip labels are italicized.
/// - internal-label-size (length): Internal label size.
/// - internal-label-color (color): Internal label color.
/// - root-length (length): Root-edge length.
/// - orientation (str): Tree orientation.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// - scale-bar (bool): Whether scale bar rendering is enabled.
/// - scale-length (auto, int, float): Requested scale-bar length.
/// - scale-unit (str, none): Optional scale-bar unit.
/// - min-auto-bar-width (length): Minimum rendered width used in auto mode.
/// - scale-bar-gap (length): Gap between tree and scale bar.
/// - scale-tick-height (length): Scale-bar tick height.
/// - scale-label-size (length): Scale-bar label size.
/// - scale-label-gap (length): Gap between scale bar and label.
/// - layout-size (dictionary): Layout callback size used to resolve fractional widths.
/// -> dictionary
#let _prepare-tree-render-at-size(
  tree-data,
  width,
  height,
  branch-weight,
  branch-color,
  tip-label-size,
  tip-label-color,
  tip-label-italics,
  internal-label-size,
  internal-label-color,
  root-length,
  orientation,
  cladogram,
  scale-bar,
  scale-length,
  scale-unit,
  min-auto-bar-width,
  scale-bar-gap,
  scale-tick-height,
  scale-label-size,
  scale-label-gap,
  layout-size,
) = {
  _validate-render-tree-args(
    width,
    height,
    branch-weight,
    tip-label-size,
    internal-label-size,
    root-length,
    orientation,
    cladogram,
    scale-bar,
    scale-unit,
    scale-bar-gap,
    scale-tick-height,
    scale-label-size,
    scale-label-gap,
  )

  let style = _build-render-tree-style(
    branch-weight,
    branch-color,
    tip-label-size,
    tip-label-color,
    tip-label-italics,
    internal-label-size,
    internal-label-color,
    root-length,
  )
  let normalized-tree = _normalize-tree(tree-data, cladogram: cladogram)
  assert(
    not (normalized-tree.effective-cladogram and scale-bar),
    message: "scale-bar cannot be used when the tree has no branch length information or when it is rendered as a cladogram.",
  )
  let layout-tree = _layout-tree(normalized-tree)
  let tree-plan = _build-tree-plan(layout-tree, style)

  let x-height-text = text(
    size: tip-label-size,
    top-edge: "x-height",
    bottom-edge: "baseline",
    "x",
  )
  let style = style
  style.insert("label-y-offset", measure(x-height-text).height)

  let fitted-plan = _fit-tree-plan(
    tree-plan,
    style,
    orientation,
    width,
    height,
    layout-size,
  )
  let scale-plan = if scale-bar {
    _build-scale-plan(
      fitted-plan,
      branch-color,
      branch-weight,
      scale-length,
      scale-unit,
      min-auto-bar-width,
      scale-tick-height,
      scale-label-size,
      scale-label-gap,
    )
  } else {
    none
  }
  (
    normalized-tree: normalized-tree,
    layout-tree: layout-tree,
    tree-plan: tree-plan,
    fitted-plan: fitted-plan,
    scale-plan: scale-plan,
  )
}

/// Draws a phylogenetic tree from a parsed tree structure.
///
/// Renders a phylogenetic tree visualization from the parsed tree data.
/// Supports customization of dimensions, styling, and orientation.
///
/// - tree-data (dictionary): The parsed tree structure from `parse-newick`.
/// - width (length, fraction): Width of the tree visualization including labels (default: 25em).
/// - height (length, auto): Height of the tree area (default: auto).
/// - branch-weight (length): Thickness of tree branches (default: 1pt).
/// - branch-color (color): Color of tree branches (default: black).
/// - tip-label-size (length): Font size of tip labels (default: 1em).
/// - tip-label-color (color): Color of tip labels (default: black).
/// - tip-label-italics (bool): Use italics to draw tip labels (default: false).
/// - internal-label-size (length): Font size of internal node labels (default: 0.85em).
/// - internal-label-color (color): Color of internal node labels (default: medium gray).
/// - root-length (length): Length of the dotted root branch (default: 1.25em).
/// - orientation (str): "horizontal" (root left, tips right) or "vertical" (root bottom, tips up) (default: "horizontal").
/// - cladogram (bool): Whether to draw the tree as a cladogram with equal branch lengths (default: false).
/// - scale-bar (bool): Whether to draw a branch-length scale bar below the tree (default: false).
///   Scale bars are unavailable for cladograms.
/// - scale-length (auto, int, float): Scale-bar length in branch-length units (default: auto).
/// - scale-unit (str, none): Optional scale-bar unit suffix (default: none).
/// - min-auto-bar-width (length): Minimum auto-selected scale-bar width when space allows (default: 2em).
/// - scale-bar-gap (length): Gap between tree and scale bar (default: 0.6em).
/// - scale-tick-height (length): Scale-bar tick height (default: 4.25pt).
/// - scale-label-size (length): Scale-bar label size (default: 0.8em).
/// - scale-label-gap (length): Gap between scale bar and scale label (default: 2.5pt).
/// -> content
#let render-tree(
  tree-data,
  width: 25em,
  height: auto,
  branch-weight: 1pt,
  branch-color: black,
  tip-label-size: 1em,
  tip-label-color: black,
  tip-label-italics: false,
  internal-label-size: 0.85em,
  internal-label-color: _medium-gray,
  root-length: 1.25em,
  orientation: "horizontal",
  cladogram: false,
  scale-bar: false,
  scale-length: auto,
  scale-unit: none,
  min-auto-bar-width: 2em,
  scale-bar-gap: 0.6em,
  scale-tick-height: 4.25pt,
  scale-label-size: 0.8em,
  scale-label-gap: 2.5pt,
) = {
  _validate-render-tree-args(
    width,
    height,
    branch-weight,
    tip-label-size,
    internal-label-size,
    root-length,
    orientation,
    cladogram,
    scale-bar,
    scale-unit,
    scale-bar-gap,
    scale-tick-height,
    scale-label-size,
    scale-label-gap,
  )
  context {
    layout(size => {
      let prepared = _prepare-tree-render-at-size(
        tree-data,
        width,
        height,
        branch-weight,
        branch-color,
        tip-label-size,
        tip-label-color,
        tip-label-italics,
        internal-label-size,
        internal-label-color,
        root-length,
        orientation,
        cladogram,
        scale-bar,
        scale-length,
        scale-unit,
        min-auto-bar-width,
        scale-bar-gap,
        scale-tick-height,
        scale-label-size,
        scale-label-gap,
        size,
      )
      _render-tree-plan(
        prepared.fitted-plan,
        prepared.scale-plan,
        scale-bar-gap,
      )
    })
  }
}
