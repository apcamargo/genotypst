#import "../common/layout_math.typ": _resolve-length
#import "../common/axis_scale.typ": (
  _draw-horizontal-segment, _draw-scale-bar-row, _draw-vertical-segment,
  _format-scale-label, _resolve-scale-bar-length,
)

#let _zero-point = (x: 0pt, y: 0pt)

/// Builds a label content element from a tree label primitive.
///
/// - label-primitive (dictionary): Label primitive metadata.
/// -> content
#let _build-tree-label-content(label-primitive) = {
  let bottom-edge = if label-primitive.placement-role == "internal-label" {
    "baseline"
  } else {
    "descender"
  }
  if label-primitive.text-fill == none {
    text(
      size: label-primitive.text-size,
      style: label-primitive.text-style,
      bottom-edge: bottom-edge,
    )[#label-primitive.text]
  } else {
    text(
      size: label-primitive.text-size,
      fill: label-primitive.text-fill,
      style: label-primitive.text-style,
      bottom-edge: bottom-edge,
    )[#label-primitive.text]
  }
}

/// Builds explicit tree primitives from a laid-out normalized tree.
///
/// - layout-tree (dictionary): Output from `_layout-tree`.
/// - style (dictionary): Tree styling configuration.
/// -> dictionary
#let _build-tree-plan(layout-tree, style) = {
  let primitives = ()
  let nodes = layout-tree.nodes
  let node-keys = layout-tree.node-keys
  let root = nodes.at(node-keys.at(layout-tree.root-id))

  if root.input-rooted {
    primitives.push((
      kind: "line",
      stroke: style.branch-stroke,
      stroke-thickness: style.branch-weight,
      start-anchor: (
        tree: (x: root.x-unit, y: root.y-unit),
        page: (x: -style.root-length, y: 0pt),
      ),
      end-anchor: (
        tree: (x: root.x-unit, y: root.y-unit),
        page: _zero-point,
      ),
    ))
  }

  for id in range(layout-tree.node-count) {
    let node = nodes.at(node-keys.at(id))
    let node-point = (x: node.x-unit, y: node.y-unit)

    if not node.is-root {
      let parent = nodes.at(node-keys.at(node.parent-id))
      primitives.push((
        kind: "line",
        stroke: style.branch-stroke,
        stroke-thickness: style.branch-weight,
        start-anchor: (
          tree: (x: parent.x-unit, y: node.y-unit),
          page: _zero-point,
        ),
        end-anchor: (tree: node-point, page: _zero-point),
      ))
    }

    if node.is-leaf {
      if node.label-text != none {
        primitives.push((
          kind: "label",
          placement-role: "tip-label",
          anchor-tree: node-point,
          text: node.label-text,
          text-size: style.tip-label-size,
          text-fill: style.tip-label-color,
          text-style: if style.tip-label-italics { "italic" } else { "normal" },
          rotation-mode: "rotate-with-tree",
        ))
      }
    } else {
      let first-child = nodes.at(node-keys.at(node.children-ids.first()))
      let last-child = nodes.at(node-keys.at(node.children-ids.last()))
      primitives.push((
        kind: "line",
        stroke: style.branch-stroke,
        stroke-thickness: style.branch-weight,
        start-anchor: (
          tree: (x: node.x-unit, y: first-child.y-unit),
          page: _zero-point,
        ),
        end-anchor: (
          tree: (x: node.x-unit, y: last-child.y-unit),
          page: _zero-point,
        ),
      ))

      if node.label-text != none {
        primitives.push((
          kind: "label",
          placement-role: "internal-label",
          anchor-tree: node-point,
          text: node.label-text,
          text-size: style.internal-label-size,
          text-fill: style.internal-label-color,
          text-style: "normal",
          rotation-mode: "stay-horizontal",
        ))
      }
    }
  }

  let plan = layout-tree
  plan.insert("tree-primitives", primitives)
  plan
}

/// Numeric tolerance used when fitting trees into a viewport.
#let _fit-tolerance = 0.1pt

/// Extra slack for the final post-fit acceptance check.
#let _fit-acceptance-tolerance = 0.2pt

/// Returns an empty bounds record.
///
/// -> dictionary
#let _empty-bounds() = (min-x: none, min-y: none, max-x: none, max-y: none)

/// Expands a bounds record to include a rectangle.
///
/// - bounds (dictionary): Current bounds.
/// - min-x (length): Rectangle left edge.
/// - min-y (length): Rectangle top edge.
/// - max-x (length): Rectangle right edge.
/// - max-y (length): Rectangle bottom edge.
/// -> dictionary
#let _expand-bounds(bounds, min-x, min-y, max-x, max-y) = {
  if bounds.min-x == none {
    (min-x: min-x, min-y: min-y, max-x: max-x, max-y: max-y)
  } else {
    (
      min-x: calc.min(bounds.min-x, min-x),
      min-y: calc.min(bounds.min-y, min-y),
      max-x: calc.max(bounds.max-x, max-x),
      max-y: calc.max(bounds.max-y, max-y),
    )
  }
}

/// Finalizes a bounds record by adding width and height.
///
/// - bounds (dictionary): Bounds record.
/// -> dictionary
#let _finalize-bounds(bounds) = {
  if bounds.min-x == none {
    (min-x: 0pt, min-y: 0pt, max-x: 0pt, max-y: 0pt, width: 0pt, height: 0pt)
  } else {
    let finalized = bounds
    finalized.insert("width", bounds.max-x - bounds.min-x)
    finalized.insert("height", bounds.max-y - bounds.min-y)
    finalized
  }
}

/// Applies the orientation transform to a point.
///
/// - x (length): Canonical x-coordinate.
/// - y (length): Canonical y-coordinate.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _transform-point(x, y, orientation) = {
  if orientation == "vertical" {
    (x: y, y: -x)
  } else {
    (x: x, y: y)
  }
}

/// Translates a point by a viewport offset.
///
/// - point (dictionary): Point to translate.
/// - translate-x (length): Horizontal translation.
/// - translate-y (length): Vertical translation.
/// -> dictionary
#let _translate-point(point, translate-x, translate-y) = {
  (x: point.x + translate-x, y: point.y + translate-y)
}

/// Measures all label primitives using the final label-construction helper.
///
/// - tree-plan (dictionary): Tree primitive plan.
/// -> dictionary
#let _measure-tree-primitives(tree-plan) = {
  let measured-primitives = ()
  for primitive in tree-plan.tree-primitives {
    if primitive.kind == "label" {
      let label-content = _build-tree-label-content(primitive)
      let label-size = measure(label-content)
      let measured-primitive = primitive
      measured-primitive.insert("content", label-content)
      measured-primitive.insert("measure-width", label-size.width)
      measured-primitive.insert("measure-height", label-size.height)
      measured-primitives.push(measured-primitive)
    } else {
      measured-primitives.push(primitive)
    }
  }
  let measured-plan = tree-plan
  measured-plan.insert("tree-primitives", measured-primitives)
  measured-plan
}

/// Resolves fit-time style lengths and prepares absolute primitive data.
///
/// - tree-plan (dictionary): Measured tree primitive plan.
/// - style (dictionary): Tree style configuration.
/// -> dictionary
#let _prepare-fit-inputs(tree-plan, style) = {
  let fit-offsets = (
    label-x-offset: _resolve-length(style.label-x-offset),
    internal-label-gap: _resolve-length(style.internal-label-gap),
    label-y-offset: _resolve-length(style.label-y-offset),
  )
  let prepared-lines = ()
  let prepared-labels = ()
  for primitive in tree-plan.tree-primitives {
    if primitive.kind == "line" {
      let start-page = primitive.start-anchor.page
      let end-page = primitive.end-anchor.page
      prepared-lines.push((
        start-anchor: (
          tree: primitive.start-anchor.tree,
          page: (
            x: if start-page.x < 0pt {
              -_resolve-length(-start-page.x)
            } else {
              _resolve-length(start-page.x)
            },
            y: if start-page.y < 0pt {
              -_resolve-length(-start-page.y)
            } else {
              _resolve-length(start-page.y)
            },
          ),
        ),
        end-anchor: (
          tree: primitive.end-anchor.tree,
          page: (
            x: if end-page.x < 0pt {
              -_resolve-length(-end-page.x)
            } else {
              _resolve-length(end-page.x)
            },
            y: if end-page.y < 0pt {
              -_resolve-length(-end-page.y)
            } else {
              _resolve-length(end-page.y)
            },
          ),
        ),
        half-stroke: _resolve-length(primitive.stroke-thickness) / 2,
        stroke: primitive.stroke,
      ))
    } else {
      prepared-labels.push((
        placement-role: primitive.placement-role,
        anchor-tree: primitive.anchor-tree,
        anchor-page: _zero-point,
        rotation-mode: primitive.rotation-mode,
        measure-width: primitive.measure-width,
        measure-height: primitive.measure-height,
        content: primitive.content,
      ))
    }
  }
  let root = tree-plan.nodes.at(tree-plan.node-keys.at(tree-plan.root-id))
  (
    fit-offsets: fit-offsets,
    prepared-lines: prepared-lines,
    prepared-labels: prepared-labels,
    root-tree-point: (x: root.x-unit, y: root.y-unit),
    // `tree-depth` excludes the separately rendered root edge and covers only
    // descendant branch-length geometry used for fitting and scale bars.
    tree-depth: tree-plan.tree-depth,
    tree-height: tree-plan.tree-height,
  )
}

/// Builds an affine formula `coeff * scale + offset`.
///
/// - coeff (float): Multiplier applied to the candidate solve scale.
/// - offset (length): Fixed page-space offset.
/// -> dictionary
#let _affine-formula(coeff, offset) = (coeff: coeff, offset: offset)

/// Returns the additive inverse of an affine formula.
///
/// - formula (dictionary): Formula record.
/// -> dictionary
#let _negate-affine-formula(formula) = {
  _affine-formula(-formula.coeff, -formula.offset)
}

/// Adds a constant offset to an affine formula.
///
/// - formula (dictionary): Formula record.
/// - delta (length): Length offset to add.
/// -> dictionary
#let _shift-affine-formula(formula, delta) = {
  _affine-formula(formula.coeff, formula.offset + delta)
}

/// Returns whether two affine formulas are identical.
///
/// - first (dictionary): Formula record.
/// - second (dictionary): Formula record.
/// -> bool
#let _affine-formulas-equal(first, second) = {
  first.coeff == second.coeff and first.offset == second.offset
}

/// Selects one screen axis from a transformed affine point.
///
/// - point (dictionary): Point with `x` and `y` affine formulas.
/// - axis (str): Screen axis name.
/// -> dictionary
#let _point-axis-formula(point, axis) = {
  if axis == "x" { point.x } else { point.y }
}

/// Returns the constrained screen axis for a solve mode.
///
/// - orientation (str): Tree orientation.
/// - axis-kind (str): Solve mode, either `"depth"` or `"spread"`.
/// -> str
#let _solve-screen-axis(orientation, axis-kind) = {
  if orientation == "vertical" {
    if axis-kind == "depth" { "y" } else { "x" }
  } else if axis-kind == "depth" {
    "x"
  } else {
    "y"
  }
}

/// Orders two affine formulas by their occupied interval edge.
///
/// This assumes the ordering does not change over the non-negative solve range,
/// which holds for the tree primitive geometry used here.
///
/// - first (dictionary): Candidate lower/upper edge formula.
/// - second (dictionary): Candidate lower/upper edge formula.
/// -> dictionary
#let _order-affine-interval(first, second) = {
  let first-precedes = (
    first.coeff < second.coeff
      or (first.coeff == second.coeff and first.offset <= second.offset)
  )
  if first-precedes {
    (min: first, max: second)
  } else {
    (min: second, max: first)
  }
}

/// Resolves canonical point formulas for one-axis solving.
///
/// - anchor-tree (dictionary): Tree-space point.
/// - anchor-page (dictionary): Page-space point.
/// - axis-kind (str): Solve mode, either `"depth"` or `"spread"`.
/// -> dictionary
#let _solve-canonical-point(anchor-tree, anchor-page, axis-kind) = {
  if axis-kind == "depth" {
    (
      x: _affine-formula(anchor-tree.x, anchor-page.x),
      y: _affine-formula(0.0, anchor-page.y),
    )
  } else {
    (
      x: _affine-formula(0.0, anchor-page.x),
      y: _affine-formula(anchor-tree.y, anchor-page.y),
    )
  }
}

/// Applies the orientation transform to affine point formulas.
///
/// - x-formula (dictionary): Canonical x-coordinate formula.
/// - y-formula (dictionary): Canonical y-coordinate formula.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _transform-point-formulas(x-formula, y-formula, orientation) = {
  if orientation == "vertical" {
    (x: y-formula, y: _negate-affine-formula(x-formula))
  } else {
    (x: x-formula, y: y-formula)
  }
}

/// Builds a solve-time span descriptor for a prepared line primitive.
///
/// - primitive (dictionary): Prepared line primitive.
/// - orientation (str): Tree orientation.
/// - axis-kind (str): Solve mode, either `"depth"` or `"spread"`.
/// -> dictionary, none
#let _line-solve-descriptor(primitive, orientation, axis-kind) = {
  let start-canonical = _solve-canonical-point(
    primitive.start-anchor.tree,
    primitive.start-anchor.page,
    axis-kind,
  )
  let end-canonical = _solve-canonical-point(
    primitive.end-anchor.tree,
    primitive.end-anchor.page,
    axis-kind,
  )
  let start = _transform-point-formulas(
    start-canonical.x,
    start-canonical.y,
    orientation,
  )
  let end = _transform-point-formulas(
    end-canonical.x,
    end-canonical.y,
    orientation,
  )
  let is-degenerate = (
    _affine-formulas-equal(start.x, end.x)
      and _affine-formulas-equal(start.y, end.y)
  )

  if is-degenerate {
    none
  } else {
    let ordered = _order-affine-interval(
      _point-axis-formula(start, _solve-screen-axis(orientation, axis-kind)),
      _point-axis-formula(end, _solve-screen-axis(orientation, axis-kind)),
    )
    (
      min-coeff: ordered.min.coeff,
      min-offset: ordered.min.offset - primitive.half-stroke,
      max-coeff: ordered.max.coeff,
      max-offset: ordered.max.offset + primitive.half-stroke,
    )
  }
}

/// Builds a solve-time span descriptor for a measured label primitive.
///
/// - primitive (dictionary): Prepared label primitive.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - orientation (str): Tree orientation.
/// - axis-kind (str): Solve mode, either `"depth"` or `"spread"`.
/// -> dictionary
#let _label-solve-descriptor(primitive, fit-offsets, orientation, axis-kind) = {
  let anchor = _solve-canonical-point(
    primitive.anchor-tree,
    primitive.anchor-page,
    axis-kind,
  )
  let canonical-origin = if primitive.placement-role == "tip-label" {
    (
      x: _shift-affine-formula(anchor.x, fit-offsets.label-x-offset),
      y: _shift-affine-formula(anchor.y, -fit-offsets.label-y-offset),
    )
  } else if orientation == "vertical" {
    (
      x: _shift-affine-formula(anchor.x, -fit-offsets.internal-label-gap),
      y: _shift-affine-formula(anchor.y, fit-offsets.label-x-offset),
    )
  } else {
    (
      x: _shift-affine-formula(
        anchor.x,
        -primitive.measure-width - fit-offsets.label-x-offset,
      ),
      y: _shift-affine-formula(
        anchor.y,
        -primitive.measure-height - fit-offsets.internal-label-gap,
      ),
    )
  }
  let origin = _transform-point-formulas(
    canonical-origin.x,
    canonical-origin.y,
    orientation,
  )
  let axis = _solve-screen-axis(orientation, axis-kind)
  let origin-axis = _point-axis-formula(origin, axis)
  let rotation = (
    orientation == "vertical" and primitive.rotation-mode == "rotate-with-tree"
  )

  if rotation {
    if axis == "x" {
      (
        min-coeff: origin-axis.coeff,
        min-offset: origin-axis.offset,
        max-coeff: origin-axis.coeff,
        max-offset: origin-axis.offset + primitive.measure-height,
      )
    } else {
      (
        min-coeff: origin-axis.coeff,
        min-offset: origin-axis.offset - primitive.measure-width,
        max-coeff: origin-axis.coeff,
        max-offset: origin-axis.offset,
      )
    }
  } else if axis == "x" {
    (
      min-coeff: origin-axis.coeff,
      min-offset: origin-axis.offset,
      max-coeff: origin-axis.coeff,
      max-offset: origin-axis.offset + primitive.measure-width,
    )
  } else {
    (
      min-coeff: origin-axis.coeff,
      min-offset: origin-axis.offset,
      max-coeff: origin-axis.coeff,
      max-offset: origin-axis.offset + primitive.measure-height,
    )
  }
}

/// Builds solve-time span descriptors for the active orientation.
///
/// - prepared-lines (array): Prepared line geometry records.
/// - prepared-labels (array): Prepared label geometry records.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _build-solve-descriptors(
  prepared-lines,
  prepared-labels,
  fit-offsets,
  orientation,
) = {
  let depth = ()
  let spread = ()

  for primitive in prepared-lines {
    let descriptor = _line-solve-descriptor(primitive, orientation, "depth")
    if descriptor != none { depth.push(descriptor) }

    let descriptor = _line-solve-descriptor(primitive, orientation, "spread")
    if descriptor != none { spread.push(descriptor) }
  }

  for primitive in prepared-labels {
    let descriptor = _label-solve-descriptor(
      primitive,
      fit-offsets,
      orientation,
      "depth",
    )
    if descriptor != none { depth.push(descriptor) }

    let descriptor = _label-solve-descriptor(
      primitive,
      fit-offsets,
      orientation,
      "spread",
    )
    if descriptor != none { spread.push(descriptor) }
  }

  (depth: depth, spread: spread)
}

/// Returns whether a public width value is still provisional in `layout()`.
///
/// - width (length, auto, ratio, relative): Requested rendered width.
/// - raw-width (length): Width reported by the wrapper block during layout.
/// -> bool
#let _tree-width-is-unresolved(width, raw-width) = {
  if width == auto {
    raw-width == float.inf * 1pt
  } else if type(width) == ratio {
    width != 0% and raw-width == 0pt
  } else if type(width) == relative {
    width.ratio != 0% and raw-width == _resolve-length(width.length)
  } else {
    false
  }
}

/// Returns whether a span fits within a viewport limit.
///
/// - span (length): Occupied screen span.
/// - viewport-limit (length): Available screen span.
/// -> bool
#let _span-fits(span, viewport-limit) = {
  span <= viewport-limit + _fit-tolerance
}

/// Returns whether a final fitted span is acceptable after fitting.
///
/// - span (length): Occupied screen span.
/// - viewport-limit (length): Available screen span.
/// -> bool
#let _span-acceptable(span, viewport-limit) = {
  span <= viewport-limit + _fit-acceptance-tolerance
}

/// Resolves a prepared line primitive into screen coordinates.
///
/// - primitive (dictionary): Line primitive with absolute anchor page offsets.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _materialize-line(primitive, x-scale, y-scale, orientation) = {
  (
    start: _transform-point(
      primitive.start-anchor.tree.x * x-scale + primitive.start-anchor.page.x,
      primitive.start-anchor.tree.y * y-scale + primitive.start-anchor.page.y,
      orientation,
    ),
    end: _transform-point(
      primitive.end-anchor.tree.x * x-scale + primitive.end-anchor.page.x,
      primitive.end-anchor.tree.y * y-scale + primitive.end-anchor.page.y,
      orientation,
    ),
  )
}

/// Computes the occupied bounds for a resolved line primitive.
///
/// - start (dictionary): Line start point.
/// - end (dictionary): Line end point.
/// - half-stroke (length): Half the rendered stroke thickness.
/// -> dictionary
#let _line-bounds(start, end, half-stroke) = (
  min-x: calc.min(start.x, end.x) - half-stroke,
  min-y: calc.min(start.y, end.y) - half-stroke,
  max-x: calc.max(start.x, end.x) + half-stroke,
  max-y: calc.max(start.y, end.y) + half-stroke,
)

/// Returns whether a resolved line is degenerate for rendering.
///
/// - line (dictionary): Resolved line endpoints.
/// -> bool
#let _line-is-degenerate(line) = {
  let dx = calc.abs(line.end.x - line.start.x)
  let dy = calc.abs(line.end.y - line.start.y)
  dx <= _fit-tolerance and dy <= _fit-tolerance
}

/// Resolves the final label origin and rotation for a measured label primitive.
///
/// - primitive (dictionary): Label primitive.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _materialize-label-origin(
  primitive,
  fit-offsets,
  x-scale,
  y-scale,
  orientation,
) = {
  let anchor-x = primitive.anchor-tree.x * x-scale + primitive.anchor-page.x
  let anchor-y = primitive.anchor-tree.y * y-scale + primitive.anchor-page.y
  let canonical-origin = if primitive.placement-role == "tip-label" {
    (
      x: anchor-x + fit-offsets.label-x-offset,
      y: anchor-y - fit-offsets.label-y-offset,
    )
  } else if orientation == "vertical" {
    (
      x: anchor-x - fit-offsets.internal-label-gap,
      y: anchor-y + fit-offsets.label-x-offset,
    )
  } else {
    (
      x: anchor-x - primitive.measure-width - fit-offsets.label-x-offset,
      y: anchor-y - primitive.measure-height - fit-offsets.internal-label-gap,
    )
  }

  (
    origin: _transform-point(
      canonical-origin.x,
      canonical-origin.y,
      orientation,
    ),
    rotation: if orientation == "vertical"
      and primitive.rotation-mode == "rotate-with-tree" {
      -90deg
    } else {
      0deg
    },
  )
}

/// Computes the occupied bounds for a resolved label primitive.
///
/// - origin (dictionary): Final label origin.
/// - width (length): Measured label width.
/// - height (length): Measured label height.
/// - rotation (angle): Final label rotation.
/// -> dictionary
#let _label-bounds(origin, width, height, rotation) = {
  if rotation == -90deg {
    (
      min-x: origin.x,
      min-y: origin.y - width,
      max-x: origin.x + height,
      max-y: origin.y,
    )
  } else {
    (
      min-x: origin.x,
      min-y: origin.y,
      max-x: origin.x + width,
      max-y: origin.y + height,
    )
  }
}

/// Resolves a prepared line into fitted screen geometry and bounds.
///
/// Degenerate resolved lines are treated as nonexistent here so solve-time and
/// final fit evaluation use the same inclusion rule.
///
/// - primitive (dictionary): Prepared line geometry record.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary, none
#let _materialize-fitted-line(primitive, x-scale, y-scale, orientation) = {
  let resolved-line = _materialize-line(
    primitive,
    x-scale,
    y-scale,
    orientation,
  )
  if _line-is-degenerate(resolved-line) {
    none
  } else {
    (
      start: resolved-line.start,
      end: resolved-line.end,
      bounds: _line-bounds(
        resolved-line.start,
        resolved-line.end,
        primitive.half-stroke,
      ),
    )
  }
}

/// Resolves a prepared label into fitted screen geometry and bounds.
///
/// - primitive (dictionary): Prepared label geometry record.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _materialize-fitted-label(
  primitive,
  fit-offsets,
  x-scale,
  y-scale,
  orientation,
) = {
  let resolved-label = _materialize-label-origin(
    primitive,
    fit-offsets,
    x-scale,
    y-scale,
    orientation,
  )
  (
    origin: resolved-label.origin,
    rotation: resolved-label.rotation,
    bounds: _label-bounds(
      resolved-label.origin,
      primitive.measure-width,
      primitive.measure-height,
      resolved-label.rotation,
    ),
  )
}

/// Evaluates occupied bounds without materializing render payloads.
///
/// - prepared-lines (array): Prepared line geometry records.
/// - prepared-labels (array): Prepared label geometry records.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _evaluate-tree-bounds-only(
  prepared-lines,
  prepared-labels,
  fit-offsets,
  x-scale,
  y-scale,
  orientation,
) = {
  let bounds = _empty-bounds()

  for primitive in prepared-lines {
    let fitted-line = _materialize-fitted-line(
      primitive,
      x-scale,
      y-scale,
      orientation,
    )
    if fitted-line != none {
      bounds = _expand-bounds(
        bounds,
        fitted-line.bounds.min-x,
        fitted-line.bounds.min-y,
        fitted-line.bounds.max-x,
        fitted-line.bounds.max-y,
      )
    }
  }

  for primitive in prepared-labels {
    let fitted-label = _materialize-fitted-label(
      primitive,
      fit-offsets,
      x-scale,
      y-scale,
      orientation,
    )
    bounds = _expand-bounds(
      bounds,
      fitted-label.bounds.min-x,
      fitted-label.bounds.min-y,
      fitted-label.bounds.max-x,
      fitted-label.bounds.max-y,
    )
  }

  _finalize-bounds(bounds)
}

/// Materializes fitted geometry and occupied bounds for the final chosen scales.
///
/// - prepared-lines (array): Prepared line geometry records.
/// - prepared-labels (array): Prepared label geometry records.
/// - root-tree-point (dictionary): Root point in tree space.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
///
/// The returned line and label arrays stay untranslated. `_fit-tree-plan(...)`
/// attaches the viewport centering offset separately so rendering can reuse the
/// same fitted geometry without rebuilding primitive dictionaries.
/// -> dictionary
#let _materialize-fitted-tree(
  prepared-lines,
  prepared-labels,
  root-tree-point,
  fit-offsets,
  x-scale,
  y-scale,
  orientation,
) = {
  let tree-lines = ()
  let tree-labels = ()
  let bounds = _empty-bounds()
  let root-position = _transform-point(
    root-tree-point.x * x-scale,
    root-tree-point.y * y-scale,
    orientation,
  )

  for primitive in prepared-lines {
    let fitted-line = _materialize-fitted-line(
      primitive,
      x-scale,
      y-scale,
      orientation,
    )
    if fitted-line != none {
      bounds = _expand-bounds(
        bounds,
        fitted-line.bounds.min-x,
        fitted-line.bounds.min-y,
        fitted-line.bounds.max-x,
        fitted-line.bounds.max-y,
      )
      tree-lines.push((
        start: fitted-line.start,
        end: fitted-line.end,
        stroke: primitive.stroke,
      ))
    }
  }

  for primitive in prepared-labels {
    let fitted-label = _materialize-fitted-label(
      primitive,
      fit-offsets,
      x-scale,
      y-scale,
      orientation,
    )
    bounds = _expand-bounds(
      bounds,
      fitted-label.bounds.min-x,
      fitted-label.bounds.min-y,
      fitted-label.bounds.max-x,
      fitted-label.bounds.max-y,
    )
    tree-labels.push((
      origin: fitted-label.origin,
      rotation: fitted-label.rotation,
      content: primitive.content,
    ))
  }

  (
    tree-lines: tree-lines,
    tree-labels: tree-labels,
    root-position: root-position,
    tree-occupied-bounds: _finalize-bounds(bounds),
  )
}

/// Evaluates the occupied span for one descriptor array at a candidate scale.
///
/// - solve-descriptors (array): Solve-time interval descriptors.
/// - scale (length): Candidate axis scale.
/// -> length
#let _evaluate-solve-span(solve-descriptors, scale) = {
  let min-edge = none
  let max-edge = none

  for descriptor in solve-descriptors {
    let min-edge-at-scale = descriptor.min-coeff * scale + descriptor.min-offset
    let max-edge-at-scale = descriptor.max-coeff * scale + descriptor.max-offset
    if min-edge == none {
      min-edge = min-edge-at-scale
      max-edge = max-edge-at-scale
    } else {
      min-edge = calc.min(min-edge, min-edge-at-scale)
      max-edge = calc.max(max-edge, max-edge-at-scale)
    }
  }

  if min-edge == none { 0pt } else { max-edge - min-edge }
}

/// Solves one axis scale by finding the right edge of the feasible fit interval.
///
/// - tree-extent (float): Extent of the solved tree axis in tree-space units.
/// - solve-descriptors (array): Solve-time interval descriptors.
/// - viewport-limit (length): Available size on the constrained screen axis.
/// - fit-band-samples (int): Number of samples evaluated per fit band.
/// - fit-max-bands (int): Maximum number of exponentially growing bands.
/// -> length
#let _solve-axis-scale(
  tree-extent,
  solve-descriptors,
  viewport-limit,
  fit-band-samples,
  fit-max-bands,
) = {
  assert(
    type(fit-band-samples) == int and fit-band-samples > 0,
    message: "fit-band-samples must be a positive integer.",
  )
  assert(
    type(fit-max-bands) == int and fit-max-bands > 0,
    message: "fit-max-bands must be a positive integer.",
  )
  if tree-extent <= 0 { return 0pt }

  let evaluate-span = scale => {
    _evaluate-solve-span(solve-descriptors, scale)
  }

  let best-fit = none
  let band-left = 0pt
  let band-right = 1pt
  for _ in range(fit-max-bands) {
    let last-fit = none
    let first-fail-after-fit = none

    for sample in range(fit-band-samples + 1) {
      let t = sample / fit-band-samples
      let scale = band-left + (band-right - band-left) * t
      if _span-fits(evaluate-span(scale), viewport-limit) {
        best-fit = scale
        last-fit = scale
      } else if last-fit != none and first-fail-after-fit == none {
        first-fail-after-fit = scale
      }
    }

    if last-fit != none and first-fail-after-fit != none {
      let low = last-fit
      let high = first-fail-after-fit
      for _ in range(48) {
        let mid = (low + high) / 2
        if _span-fits(evaluate-span(mid), viewport-limit) {
          low = mid
        } else {
          high = mid
        }
      }
      return low
    }

    if last-fit != none {
      best-fit = last-fit
    }

    band-left = band-right
    band-right *= 2
  }

  if best-fit != none { best-fit } else { 0pt }
}

/// Resolves the tree viewport and fitted primitives for rendering.
///
/// - tree-plan (dictionary): Tree primitive plan.
/// - style (dictionary): Tree style configuration.
/// - orientation (str): Tree orientation.
/// - width (length, auto, relative): Original rendered width argument.
/// - height (length, auto): Target rendered tree height.
/// - layout-size (dictionary): Layout callback size.
/// - fit-band-samples (int): Number of samples evaluated per fit band.
/// - fit-max-bands (int): Maximum number of exponentially growing bands.
///
/// The returned fitted geometry stays split into untranslated line/label
/// arrays. The viewport-centering offset is returned separately in
/// `tree-translation`, while `root-position` is already translated into
/// viewport coordinates.
/// -> dictionary
#let _fit-tree-plan(
  tree-plan,
  style,
  orientation,
  width,
  height,
  layout-size,
  fit-band-samples,
  fit-max-bands,
) = {
  let measured-plan = _measure-tree-primitives(tree-plan)
  let fit-inputs = _prepare-fit-inputs(measured-plan, style)
  let fit-offsets = fit-inputs.fit-offsets
  let solve-descriptors = _build-solve-descriptors(
    fit-inputs.prepared-lines,
    fit-inputs.prepared-labels,
    fit-offsets,
    orientation,
  )
  let raw-width = layout-size.width
  let viewport-height = if height == auto {
    let label-only-bounds = _evaluate-tree-bounds-only(
      fit-inputs.prepared-lines,
      fit-inputs.prepared-labels,
      fit-offsets,
      0pt,
      0pt,
      orientation,
    )
    calc.max(
      _resolve-length(style.auto-height-scale * measured-plan.tree-height),
      label-only-bounds.height,
    )
  } else {
    _resolve-length(height)
  }

  // Typst reports parent-dependent widths provisionally during measurement.
  // `%`-based widths should stay deferred until the parent width resolves,
  // while `auto` gets a dedicated intrinsic-width fit below.
  let provisional-width = _tree-width-is-unresolved(width, raw-width)
  let fitted-width = if width == auto {
    let intrinsic-scale = _solve-axis-scale(
      if orientation == "vertical" {
        fit-inputs.tree-depth
      } else {
        fit-inputs.tree-height
      },
      if orientation == "vertical" {
        solve-descriptors.depth
      } else {
        solve-descriptors.spread
      },
      viewport-height,
      fit-band-samples,
      fit-max-bands,
    )
    let materialized-tree = _materialize-fitted-tree(
      fit-inputs.prepared-lines,
      fit-inputs.prepared-labels,
      fit-inputs.root-tree-point,
      fit-offsets,
      intrinsic-scale,
      intrinsic-scale,
      orientation,
    )
    (
      width-unresolved: false,
      viewport-width: materialized-tree.tree-occupied-bounds.width,
      x-scale: intrinsic-scale,
      y-scale: intrinsic-scale,
      materialized-tree: materialized-tree,
    )
  } else {
    let width-unresolved = provisional-width
    let viewport-width = if width-unresolved { 0pt } else {
      _resolve-length(raw-width)
    }
    let x-scale = _solve-axis-scale(
      fit-inputs.tree-depth,
      solve-descriptors.depth,
      if orientation == "vertical" { viewport-height } else { viewport-width },
      fit-band-samples,
      fit-max-bands,
    )
    let y-scale = _solve-axis-scale(
      fit-inputs.tree-height,
      solve-descriptors.spread,
      if orientation == "vertical" { viewport-width } else { viewport-height },
      fit-band-samples,
      fit-max-bands,
    )
    let materialized-tree = _materialize-fitted-tree(
      fit-inputs.prepared-lines,
      fit-inputs.prepared-labels,
      fit-inputs.root-tree-point,
      fit-offsets,
      x-scale,
      y-scale,
      orientation,
    )
    (
      width-unresolved: width-unresolved,
      viewport-width: viewport-width,
      x-scale: x-scale,
      y-scale: y-scale,
      materialized-tree: materialized-tree,
    )
  }
  let width-unresolved = fitted-width.width-unresolved
  let viewport-width = fitted-width.viewport-width
  let x-scale = fitted-width.x-scale
  let y-scale = fitted-width.y-scale
  let materialized-tree = fitted-width.materialized-tree

  let issues = ()
  if not width-unresolved and not _span-acceptable(
    materialized-tree.tree-occupied-bounds.width,
    viewport-width,
  ) {
    issues.push(
      "width is too small for the tree labels and fixed margins (current: "
        + repr(viewport-width)
        + ", required: >= "
        + repr(materialized-tree.tree-occupied-bounds.width)
        + ")",
    )
  }
  if not _span-acceptable(
    materialized-tree.tree-occupied-bounds.height,
    viewport-height,
  ) {
    issues.push(
      "height is too small for the tree labels and fixed margins (current: "
        + repr(viewport-height)
        + ", required: >= "
        + repr(materialized-tree.tree-occupied-bounds.height)
        + ")",
    )
  }
  assert(
    issues.len() == 0,
    message: "Tree cannot be rendered: "
      + issues.join("; ")
      + ". Increase width or height, reduce labels, reduce label size, or reduce root-length.",
  )
  let translate-x = if width-unresolved {
    -materialized-tree.tree-occupied-bounds.min-x
  } else {
    (
      (viewport-width - materialized-tree.tree-occupied-bounds.width) / 2
        - materialized-tree.tree-occupied-bounds.min-x
    )
  }
  let translate-y = (
    (viewport-height - materialized-tree.tree-occupied-bounds.height) / 2
      - materialized-tree.tree-occupied-bounds.min-y
  )
  let tree-translation = (x: translate-x, y: translate-y)

  (
    tree-lines: materialized-tree.tree-lines,
    tree-labels: materialized-tree.tree-labels,
    tree-translation: tree-translation,
    width-unresolved: width-unresolved,
    root-position: _translate-point(
      materialized-tree.root-position,
      translate-x,
      translate-y,
    ),
    tree-depth: fit-inputs.tree-depth,
    x-scale: x-scale,
    orientation: orientation,
    tree-viewport-width: viewport-width,
    tree-viewport-height: viewport-height,
    tree-depth-span: x-scale * fit-inputs.tree-depth,
  )
}

/// Builds the optional scale-bar row.
///
/// - fitted-plan (dictionary): Output from `_fit-tree-plan`.
/// - branch-color (color): Scale bar color.
/// - branch-weight (length): Scale bar stroke thickness.
/// - scale-length (auto, int, float): Requested scale length in branch-length units.
/// - unit (str, none): Optional scale-bar unit.
/// - min-auto-bar-width (length): Minimum rendered width used in auto mode.
/// - scale-tick-height (length): Tick height.
/// - scale-label-size (length): Label size.
/// -> content
#let _build-scale-plan(
  fitted-plan,
  branch-color,
  branch-weight,
  scale-length,
  unit,
  min-auto-bar-width,
  scale-tick-height,
  scale-label-size,
) = {
  let row-width = fitted-plan.tree-viewport-width
  let root-position = fitted-plan.root-position
  let bar-left = if fitted-plan.orientation == "vertical" { 0pt } else {
    root-position.x
  }
  let max-bar-width = if fitted-plan.orientation == "vertical" {
    row-width
  } else {
    calc.max(0pt, fitted-plan.tree-depth-span)
  }
  let resolved-scale = _resolve-scale-bar-length(
    scale-length,
    // `tree-depth` reflects descendant branch lengths only; the visible root
    // stub is controlled separately by `root-length`.
    fitted-plan.tree-depth,
    fitted-plan.x-scale,
    max-bar-width,
    min-auto-bar-width: min-auto-bar-width,
    zero-length-message: "Cannot render scale bar for zero-depth tree.",
  )
  let scale-label = _format-scale-label(resolved-scale.length, unit)
  let scale-label-gap = 1.5pt
  _draw-scale-bar-row(
    row-width,
    0pt,
    bar-left,
    resolved-scale.width,
    scale-tick-height,
    scale-label-gap,
    scale-label-size,
    none,
    scale-label,
    branch-color,
    branch-weight,
  )
}

/// Renders a fitted tree plan and optional scale-bar row.
///
/// - fitted-plan (dictionary): Output from `_fit-tree-plan`.
/// - scale-plan (content, none): Optional scale row.
/// - scale-bar-gap (length): Gap between tree and scale bar.
///
/// The fitted line and label arrays stay untranslated. This renderer applies
/// `tree-translation` at draw time so the same centering offset can be reused
/// without rebuilding primitive dictionaries.
/// -> content
#let _render-tree-plan(fitted-plan, scale-plan, scale-bar-gap) = {
  let tree-translation = fitted-plan.tree-translation
  let tree-box = box(
    width: fitted-plan.tree-viewport-width,
    height: fitted-plan.tree-viewport-height,
    {
      for primitive in fitted-plan.tree-lines {
        let start = _translate-point(
          primitive.start,
          tree-translation.x,
          tree-translation.y,
        )
        let end = _translate-point(
          primitive.end,
          tree-translation.x,
          tree-translation.y,
        )
        let dx = calc.abs(end.x - start.x)
        let dy = calc.abs(end.y - start.y)
        if dy <= _fit-tolerance {
          _draw-horizontal-segment(
            calc.min(start.x, end.x),
            start.y,
            dx,
            primitive.stroke,
          )
        } else if dx <= _fit-tolerance {
          _draw-vertical-segment(
            start.x,
            calc.min(start.y, end.y),
            dy,
            primitive.stroke,
          )
        } else {
          place(top + left, line(
            start: (start.x, start.y),
            end: (end.x, end.y),
            stroke: primitive.stroke,
          ))
        }
      }
      for primitive in fitted-plan.tree-labels {
        let origin = _translate-point(
          primitive.origin,
          tree-translation.x,
          tree-translation.y,
        )
        place(
          top + left,
          dx: origin.x,
          dy: origin.y,
          if primitive.rotation == 0deg {
            primitive.content
          } else {
            rotate(primitive.rotation, origin: top + left, primitive.content)
          },
        )
      }
    },
  )

  if scale-plan == none {
    tree-box
  } else {
    block(breakable: false, stack(
      spacing: scale-bar-gap,
      tree-box,
      scale-plan,
    ))
  }
}
