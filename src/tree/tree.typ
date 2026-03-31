#import "../common/colors.typ": _medium-gray
#import "./tree_backend.typ": _tree-prepare-layout-backend
#import "./tree_fit.typ": _fit-tree-plan
#import "./tree_primitives.typ": _build-tree-plan
#import "./tree_render.typ": _build-scale-plan, _render-tree-plan

// `render-unrooted-tree` exposes a public `layout` parameter, so we keep a
// distinct handle to Typst's `layout(...)` function for both renderers.
#let _tree-render-layout = layout

/// Tree layout constants.
#let _label-x-offset = 0.28em
#let _internal-label-gap = 0.38em
#let _auto-height-scale = 1.9em
#let _rectangular-fit-band-samples = 1
#let _rectangular-fit-max-bands = 24

/// Returns whether the public `width` argument is valid.
///
/// - width (length, auto, ratio, relative): Requested rendered width.
/// -> bool
#let _render-tree-width-is-valid(width) = {
  if width == auto {
    true
  } else if type(width) == length {
    width > 0pt
  } else if type(width) == ratio {
    width > 0%
  } else if type(width) == relative {
    width.ratio > 0% or width.length > 0pt
  } else {
    false
  }
}

/// Validates the `render-tree` arguments that affect layout and sizing.
///
/// - width (length, auto, ratio, relative): Requested rendered width.
/// - height (length, auto): Requested rendered tree height.
/// - branch-weight (length): Branch stroke thickness.
/// - tip-label-size (length): Tip label size.
/// - internal-label-size (length): Internal label size.
/// - root-length (length): Root-edge length.
/// - orientation (str): Tree orientation.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// - scale-bar (bool): Whether scale bar rendering is enabled.
/// - unit (str, none): Optional unit string.
/// - scale-bar-gap (length): Gap between tree and scale bar.
/// - scale-tick-height (length): Scale bar tick height.
/// - scale-label-size (length): Scale bar label size.
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
  unit,
  scale-bar-gap,
  scale-tick-height,
  scale-label-size,
) = {
  assert(type(cladogram) == bool, message: "cladogram must be a boolean.")
  assert(type(scale-bar) == bool, message: "scale-bar must be a boolean.")
  assert(branch-weight > 0pt, message: "branch-weight must be positive.")
  assert(tip-label-size > 0pt, message: "tip-label-size must be positive.")
  assert(
    internal-label-size > 0pt,
    message: "internal-label-size must be positive.",
  )
  assert(root-length >= 0pt, message: "root-length must be non-negative.")
  assert(
    _render-tree-width-is-valid(width),
    message: "width must be auto or a positive length, ratio, or relative width.",
  )
  assert(
    height == auto or height > 0pt,
    message: "height must be auto or a positive length.",
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
    assert(scale-bar-gap >= 0pt, message: "scale-bar-gap must be non-negative.")
    assert(
      scale-tick-height > 0pt,
      message: "scale-tick-height must be positive.",
    )
    assert(
      scale-label-size > 0pt,
      message: "scale-label-size must be positive.",
    )
  }
}

/// Validates the `render-unrooted-tree` arguments.
///
/// - width (length, auto, ratio, relative): Requested rendered width.
/// - height (length, auto): Requested rendered tree height.
/// - branch-weight (length): Branch stroke thickness.
/// - tip-label-size (length): Tip label size.
/// - internal-label-size (length): Internal label size.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// - layout (str): Unrooted layout name.
/// -> none
#let _validate-render-unrooted-tree-args(
  width,
  height,
  branch-weight,
  tip-label-size,
  internal-label-size,
  cladogram,
  layout,
) = {
  assert(type(cladogram) == bool, message: "cladogram must be a boolean.")
  assert(branch-weight > 0pt, message: "branch-weight must be positive.")
  assert(tip-label-size > 0pt, message: "tip-label-size must be positive.")
  assert(
    internal-label-size > 0pt,
    message: "internal-label-size must be positive.",
  )
  assert(
    _render-tree-width-is-valid(width),
    message: "width must be auto or a positive length, ratio, or relative width.",
  )
  assert(
    height == auto or height > 0pt,
    message: "height must be auto or a positive length.",
  )
  assert(
    layout in ("equal-angle", "daylight"),
    message: "layout must be 'equal-angle' or 'daylight'.",
  )
}

/// Builds the shared style record for tree rendering.
///
/// - branch-weight (length): Branch stroke thickness.
/// - branch-color (color): Branch color.
/// - tip-label-size (length): Tip label size.
/// - tip-label-color (color, none): Tip label color.
/// - tip-label-italics (bool): Whether tip labels are italicized.
/// - internal-label-size (length): Internal label size.
/// - internal-label-color (color, none): Internal label color.
/// -> dictionary
#let _build-render-tree-style(
  branch-weight,
  branch-color,
  tip-label-size,
  tip-label-color,
  tip-label-italics,
  internal-label-size,
  internal-label-color,
) = (
  branch-stroke: stroke(
    thickness: branch-weight,
    paint: branch-color,
    cap: "square",
  ),
  branch-weight: branch-weight,
  tip-label-size: tip-label-size,
  tip-label-color: tip-label-color,
  tip-label-italics: tip-label-italics,
  internal-label-size: internal-label-size,
  internal-label-color: internal-label-color,
  label-x-offset: _label-x-offset,
  internal-label-gap: _internal-label-gap,
  auto-height-scale: _auto-height-scale,
)

/// Builds the rectangular-tree style record and resolves label offsets.
///
/// - branch-weight (length): Branch stroke thickness.
/// - branch-color (color): Branch color.
/// - tip-label-size (length): Tip label size.
/// - tip-label-color (color, none): Tip label color.
/// - tip-label-italics (bool): Whether tip labels are italicized.
/// - internal-label-size (length): Internal label size.
/// - internal-label-color (color, none): Internal label color.
/// - root-length (length): Rendered root-edge length.
/// -> dictionary
#let _build-rectangular-tree-style(
  branch-weight,
  branch-color,
  tip-label-size,
  tip-label-color,
  tip-label-italics,
  internal-label-size,
  internal-label-color,
  root-length,
) = {
  let style = _build-render-tree-style(
    branch-weight,
    branch-color,
    tip-label-size,
    tip-label-color,
    tip-label-italics,
    internal-label-size,
    internal-label-color,
  )
  let x-height-text = text(
    size: tip-label-size,
    top-edge: "x-height",
    bottom-edge: "baseline",
    "x",
  )
  style.insert("root-length", root-length)
  style.insert("label-y-offset", measure(x-height-text).height)
  style
}

/// Prepares tree data for rendering.
///
/// - tree-data (dictionary): Parsed or manual tree data.
/// - width (length, auto, ratio, relative): Original rendered width argument.
/// - height (length, auto): Height of the rendered tree area.
/// - branch-weight (length): Branch stroke thickness.
/// - branch-color (color): Branch color.
/// - tip-label-size (length): Tip label size.
/// - tip-label-color (color, none): Tip label color.
/// - tip-label-italics (bool): Whether tip labels are italicized.
/// - internal-label-size (length): Internal label size.
/// - internal-label-color (color, none): Internal label color.
/// - root-length (length): Rendered root-edge length.
/// - orientation (str): Tree orientation.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// - scale-bar (bool): Whether scale bar rendering is enabled.
/// - scale-length (auto, int, float): Requested scale-bar length in branch-length units. Positive when not auto.
/// - unit (str, none): Optional scale-bar unit.
/// - min-auto-bar-width (length): Minimum rendered width used in auto mode.
/// - scale-bar-gap (length): Gap between tree and scale bar.
/// - scale-tick-height (length): Scale-bar tick height.
/// - scale-label-size (length): Scale-bar label size.
/// - layout-size (dictionary): Available layout size.
/// -> dictionary with keys:
///   - fitted-plan (dictionary): Prepared tree layout data.
///   - scale-plan (content, none): Optional scale-bar row.
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
  unit,
  min-auto-bar-width,
  scale-bar-gap,
  scale-tick-height,
  scale-label-size,
  layout-size,
) = {
  let style = _build-rectangular-tree-style(
    branch-weight,
    branch-color,
    tip-label-size,
    tip-label-color,
    tip-label-italics,
    internal-label-size,
    internal-label-color,
    root-length,
  )
  let layout-tree = _tree-prepare-layout-backend(
    tree-data,
    cladogram: cladogram,
    suppress-unrooted: false,
    layout-kind: "rectangular",
  )
  assert(
    not (layout-tree.effective-cladogram and scale-bar),
    message: "scale-bar cannot be used when the tree has no branch length information or when it is rendered as a cladogram.",
  )
  let tree-plan = _build-tree-plan(layout-tree, style, orientation: orientation)

  let fitted-plan = _fit-tree-plan(
    tree-plan,
    style,
    orientation,
    width,
    height,
    layout-size,
    _rectangular-fit-max-bands,
    // Rectangular occupied span is monotone enough that one sample per band
    // keeps the search cheap without changing the generic solver structure.
    fit-band-samples: _rectangular-fit-band-samples,
    optimize-uniform-rotation: false,
  )
  let scale-plan = if scale-bar and not fitted-plan.width-unresolved {
    _build-scale-plan(
      fitted-plan,
      branch-color,
      branch-weight,
      scale-length,
      unit,
      min-auto-bar-width,
      scale-tick-height,
      scale-label-size,
    )
  } else {
    none
  }
  (
    fitted-plan: fitted-plan,
    scale-plan: scale-plan,
  )
}

/// Prepares unrooted tree data for rendering.
///
/// - tree-data (dictionary): Parsed or manual tree data.
/// - width (length, auto, ratio, relative): Original rendered width argument.
/// - height (length, auto): Height of the rendered tree area.
/// - branch-weight (length): Branch stroke thickness.
/// - branch-color (color): Branch color.
/// - tip-label-size (length): Tip label size.
/// - tip-label-color (color, none): Tip label color.
/// - tip-label-italics (bool): Whether tip labels are italicized.
/// - internal-label-size (length): Internal label size.
/// - internal-label-color (color, none): Internal label color.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// - layout (str): Unrooted layout name.
/// - layout-size (dictionary): Available layout size.
/// - optimize-rotation (bool): Whether automatic global rotation search is enabled.
/// -> dictionary with key `fitted-plan`
#let _prepare-unrooted-tree-render-at-size(
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
  cladogram,
  layout,
  layout-size,
  optimize-rotation: true,
) = {
  let style = _build-render-tree-style(
    branch-weight,
    branch-color,
    tip-label-size,
    tip-label-color,
    tip-label-italics,
    internal-label-size,
    internal-label-color,
  )
  let layout-tree = _tree-prepare-layout-backend(
    tree-data,
    cladogram: cladogram,
    suppress-unrooted: true,
    layout-kind: layout,
  )
  let tree-plan = _build-tree-plan(
    layout-tree,
    style,
    orientation: "horizontal",
  )
  let fitted-plan = _fit-tree-plan(
    tree-plan,
    style,
    "horizontal",
    width,
    height,
    layout-size,
    _rectangular-fit-max-bands,
    optimize-uniform-rotation: optimize-rotation,
  )
  (fitted-plan: fitted-plan)
}

/// Draws a phylogenetic tree from parsed or manual tree data.
///
/// Renders a phylogenetic tree visualization from parsed or manual tree data.
/// Supports customization of dimensions, styling, and orientation.
///
/// - tree-data (dictionary): Parsed or manually constructed tree data.
/// - width (length, auto, ratio, relative): Width of the tree visualization
///   including labels (default: 100%).
/// - height (length, auto): Height of the tree area (default: auto).
/// - branch-weight (length): Thickness of tree branches (default: 1pt).
/// - branch-color (color): Color of tree branches (default: black).
/// - tip-label-size (length): Font size of tip labels (default: 1em).
/// - tip-label-color (color, none): Color of tip labels (default: none, inherits from the document).
/// - tip-label-italics (bool): Use italics to draw tip labels (default: false).
/// - internal-label-size (length): Font size of internal node labels (default: 0.85em).
/// - internal-label-color (color, none): Color of internal node labels (default: medium gray; `none` inherits from the document).
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
/// - scale-bar-gap (length): Gap between tree and scale bar (default: 0.6em).
/// - scale-tick-height (length): Scale-bar tick height (default: 4.25pt).
/// - scale-label-size (length): Scale-bar label size (default: 0.8em).
/// -> content
#let render-tree(
  tree-data,
  width: 100%,
  height: auto,
  branch-weight: 1pt,
  branch-color: black,
  tip-label-size: 1em,
  tip-label-color: none,
  tip-label-italics: false,
  internal-label-size: 0.85em,
  internal-label-color: _medium-gray,
  root-length: 1.2em,
  orientation: "horizontal",
  cladogram: false,
  scale-bar: false,
  scale-length: auto,
  unit: none,
  min-auto-bar-width: 2.5em,
  scale-bar-gap: 0.6em,
  scale-tick-height: 4.25pt,
  scale-label-size: 0.8em,
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
    unit,
    scale-bar-gap,
    scale-tick-height,
    scale-label-size,
  )
  block(width: width)[
    #_tree-render-layout(size => context {
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
        unit,
        min-auto-bar-width,
        scale-bar-gap,
        scale-tick-height,
        scale-label-size,
        size,
      )
      _render-tree-plan(
        prepared.fitted-plan,
        prepared.scale-plan,
        scale-bar-gap,
      )
    })
  ]
}

/// Draws an unrooted phylogenetic tree using an equal-angle or daylight layout.
///
/// - tree-data (dictionary): Parsed or manually constructed tree data.
/// - width (length, auto, ratio, relative): Width of the tree visualization including labels (default: 100%).
/// - height (length, auto): Height of the tree area (default: auto).
/// - branch-weight (length): Thickness of tree branches (default: 1pt).
/// - branch-color (color): Color of tree branches (default: black).
/// - tip-label-size (length): Font size of tip labels (default: 1em).
/// - tip-label-color (color, none): Color of tip labels (default: none, inherits from the document).
/// - tip-label-italics (bool): Use italics to draw tip labels (default: false).
/// - internal-label-size (length): Font size of internal node labels (default: 0.85em).
/// - internal-label-color (color, none): Color of internal node labels (default: medium gray; `none` inherits from the document).
/// - cladogram (bool): Whether to draw the tree as a cladogram with equal branch lengths (default: false).
/// - layout (str): "equal-angle" or "daylight" (default: "equal-angle").
/// -> content
#let render-unrooted-tree(
  tree-data,
  width: 100%,
  height: auto,
  branch-weight: 1pt,
  branch-color: black,
  tip-label-size: 1em,
  tip-label-color: none,
  tip-label-italics: false,
  internal-label-size: 0.85em,
  internal-label-color: _medium-gray,
  cladogram: false,
  layout: "equal-angle",
) = {
  _validate-render-unrooted-tree-args(
    width,
    height,
    branch-weight,
    tip-label-size,
    internal-label-size,
    cladogram,
    layout,
  )
  block(width: width)[
    #_tree-render-layout(size => context {
      let prepared = _prepare-unrooted-tree-render-at-size(
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
        cladogram,
        layout,
        size,
      )
      _render-tree-plan(
        prepared.fitted-plan,
        none,
        0pt,
      )
    })
  ]
}
