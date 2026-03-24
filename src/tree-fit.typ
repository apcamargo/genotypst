#import "tree-layout.typ": _tree-node-key
#import "tree-plan.typ": _build-tree-label-content
#import "utils.typ": (
  _draw-horizontal-segment, _draw-scale-bar-row, _draw-vertical-segment,
  _format-scale-label, _resolve-length, _resolve-scale-bar-length,
)

/// Numeric tolerance used when fitting trees into a viewport.
#let _fit-tolerance = 0.1pt

/// Extra slack for the final post-fit acceptance check.
#let _fit-acceptance-tolerance = 0.2pt

/// Convergence threshold for alternating axis solves.
#let _fit-scale-convergence = 0.0001pt

/// Number of samples used while locating the feasible fit band.
#let _fit-band-samples = 24

/// Maximum number of exponentially growing fit bands to explore.
#let _fit-max-bands = 24

/// Resolves a potentially signed mixed-unit length with a render-specific anchor.
///
/// - value (length): Length expression that may be negative or mixed-unit.
/// - anchor (length): Positive anchor larger than the negative excursion.
/// -> length
#let _resolve-signed-length(value, anchor) = {
  _resolve-length(anchor + value) - anchor
}

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

/// Resolves the target tree viewport width.
///
/// - width (length, fraction): Requested tree width.
/// - layout-size (dictionary): Layout callback size.
/// -> length
#let _resolve-tree-width(width, layout-size) = {
  if type(width) == fraction {
    if layout-size.width == float.inf * 1em { 30em } else {
      layout-size.width * (width / 1fr)
    }
  } else {
    width
  }
}

/// Builds a safe coordinate anchor for mixed-unit point resolution.
///
/// - tree-plan (dictionary): Measured tree primitive plan.
/// - style (dictionary): Tree style configuration.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// -> length
#let _coordinate-resolve-anchor(tree-plan, style, x-scale, y-scale) = {
  let max-label-width = 0pt
  let max-label-height = 0pt
  for primitive in tree-plan.tree-primitives {
    if primitive.kind == "label" {
      max-label-width = calc.max(max-label-width, primitive.measure-width)
      max-label-height = calc.max(max-label-height, primitive.measure-height)
    }
  }

  _resolve-length(
    tree-plan.tree-depth * x-scale
      + tree-plan.tree-height * y-scale
      + style.root-length
      + max-label-width
      + max-label-height
      + style.label-x-offset
      + style.internal-label-gap
      + style.label-y-offset
      + 4pt,
  )
}

/// Applies the orientation transform to a point.
///
/// - x (length): Canonical x-coordinate.
/// - y (length): Canonical y-coordinate.
/// - orientation (str): Tree orientation.
/// - resolve-anchor (length): Anchor for mixed-unit signed coordinate resolution.
/// -> dictionary
#let _transform-point(x, y, orientation, resolve-anchor) = {
  if orientation == "vertical" {
    (
      x: _resolve-signed-length(y, resolve-anchor),
      y: _resolve-signed-length(-x, resolve-anchor),
    )
  } else {
    (
      x: _resolve-signed-length(x, resolve-anchor),
      y: _resolve-signed-length(y, resolve-anchor),
    )
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

/// Returns the occupied span for a bounds axis.
///
/// - bounds (dictionary): Finalized bounds record.
/// - axis (str): "x" or "y".
/// -> length
#let _bounds-span(bounds, axis) = {
  if axis == "x" { bounds.width } else { bounds.height }
}

/// Returns whether a span fits within a viewport limit.
///
/// - span (length): Occupied screen span.
/// - viewport-limit (length): Available screen span.
/// -> bool
#let _span-fits(span, viewport-limit) = {
  _resolve-length(span) <= _resolve-length(viewport-limit) + _fit-tolerance
}

/// Returns whether a final fitted span is acceptable after search convergence.
///
/// - span (length): Occupied screen span.
/// - viewport-limit (length): Available screen span.
/// -> bool
#let _span-acceptable(span, viewport-limit) = {
  _resolve-length(span) <= _resolve-length(viewport-limit) + _fit-acceptance-tolerance
}

/// Computes a mixed-unit line endpoint.
///
/// - tree-point (dictionary): Tree-space point.
/// - page-point (dictionary): Page-space offset.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// - resolve-anchor (length): Anchor for mixed-unit signed coordinate resolution.
/// -> dictionary
#let _materialize-line-point(
  tree-point,
  page-point,
  x-scale,
  y-scale,
  orientation,
  resolve-anchor,
) = {
  _transform-point(
    tree-point.x * x-scale + page-point.x,
    tree-point.y * y-scale + page-point.y,
    orientation,
    resolve-anchor,
  )
}

/// Resolves the final label origin and rotation for a measured label primitive.
///
/// - primitive (dictionary): Label primitive.
/// - style (dictionary): Tree style configuration.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// - resolve-anchor (length): Anchor for mixed-unit signed coordinate resolution.
/// -> dictionary
#let _materialize-label-origin(
  primitive,
  style,
  x-scale,
  y-scale,
  orientation,
  resolve-anchor,
) = {
  let anchor-x = primitive.anchor-tree.x * x-scale + primitive.anchor-page.x
  let anchor-y = primitive.anchor-tree.y * y-scale + primitive.anchor-page.y
  let canonical-origin = if primitive.placement-role == "tip-label" {
    (
      x: anchor-x + style.label-x-offset,
      y: anchor-y - style.label-y-offset,
    )
  } else if orientation == "vertical" {
    (
      x: anchor-x - style.internal-label-gap,
      y: anchor-y + style.label-x-offset,
    )
  } else {
    (
      x: anchor-x - primitive.measure-width - style.label-x-offset,
      y: anchor-y - primitive.measure-height - style.internal-label-gap,
    )
  }

  (
    origin: _transform-point(
      canonical-origin.x,
      canonical-origin.y,
      orientation,
      resolve-anchor,
    ),
    rotation: if orientation == "vertical" and primitive.rotation-mode == "rotate-with-tree" {
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

/// Evaluates bounds and fitted primitives for a pair of axis scales.
///
/// - tree-plan (dictionary): Measured tree primitive plan.
/// - style (dictionary): Tree style configuration.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _evaluate-tree-bounds(tree-plan, style, x-scale, y-scale, orientation) = {
  let fitted-primitives = ()
  let bounds = _empty-bounds()
  let node-positions = (:)
  let resolve-anchor = _coordinate-resolve-anchor(tree-plan, style, x-scale, y-scale)

  for id in range(tree-plan.node-count) {
    let node = tree-plan.nodes.at(_tree-node-key(id))
    node-positions.insert(_tree-node-key(id), _transform-point(
      node.x-unit * x-scale,
      node.y-unit * y-scale,
      orientation,
      resolve-anchor,
    ))
  }

  for primitive in tree-plan.tree-primitives {
    if primitive.kind == "line" {
      let start = _materialize-line-point(
        primitive.tree-start,
        primitive.page-start,
        x-scale,
        y-scale,
        orientation,
        resolve-anchor,
      )
      let end = _materialize-line-point(
        primitive.tree-end,
        primitive.page-end,
        x-scale,
        y-scale,
        orientation,
        resolve-anchor,
      )
      let dx = _resolve-length(calc.abs(end.x - start.x))
      let dy = _resolve-length(calc.abs(end.y - start.y))
      if dx > _fit-tolerance or dy > _fit-tolerance {
        let half-stroke = primitive.stroke-thickness / 2
        let line-bounds = (
          min-x: calc.min(start.x, end.x) - half-stroke,
          min-y: calc.min(start.y, end.y) - half-stroke,
          max-x: calc.max(start.x, end.x) + half-stroke,
          max-y: calc.max(start.y, end.y) + half-stroke,
        )
        bounds = _expand-bounds(
          bounds,
          line-bounds.min-x,
          line-bounds.min-y,
          line-bounds.max-x,
          line-bounds.max-y,
        )
        let fitted-line = primitive
        fitted-line.insert("start", start)
        fitted-line.insert("end", end)
        fitted-primitives.push(fitted-line)
      }
    } else {
      let resolved-label = _materialize-label-origin(
        primitive,
        style,
        x-scale,
        y-scale,
        orientation,
        resolve-anchor,
      )
      let label-bounds = _label-bounds(
        resolved-label.origin,
        primitive.measure-width,
        primitive.measure-height,
        resolved-label.rotation,
      )
      bounds = _expand-bounds(
        bounds,
        label-bounds.min-x,
        label-bounds.min-y,
        label-bounds.max-x,
        label-bounds.max-y,
      )
      let fitted-label = primitive
      fitted-label.insert("origin", resolved-label.origin)
      fitted-label.insert("rotation", resolved-label.rotation)
      fitted-primitives.push(fitted-label)
    }
  }

  (
    tree-primitives: fitted-primitives,
    node-positions: node-positions,
    tree-occupied-bounds: _finalize-bounds(bounds),
  )
}

/// Solves one axis scale by finding the right edge of the feasible fit interval.
///
/// - tree-plan (dictionary): Measured tree primitive plan.
/// - style (dictionary): Tree style configuration.
/// - orientation (str): Tree orientation.
/// - viewport-limit (length): Available size on the constrained screen axis.
/// - axis-kind (str): "depth" or "spread".
/// - other-scale (length): Current candidate scale on the other tree axis.
/// -> length
#let _solve-axis-scale(
  tree-plan,
  style,
  orientation,
  viewport-limit,
  axis-kind,
  other-scale: 0pt,
) = {
  let tree-extent = if axis-kind == "depth" {
    tree-plan.tree-depth
  } else {
    tree-plan.tree-height
  }
  if tree-extent <= 0 { return 0pt }

  let screen-axis = if orientation == "vertical" {
    if axis-kind == "depth" { "y" } else { "x" }
  } else {
    if axis-kind == "depth" { "x" } else { "y" }
  }
  let viewport-limit-abs = _resolve-length(viewport-limit)
  let evaluate-span = scale => {
    let x-scale = if axis-kind == "depth" { scale } else { other-scale }
    let y-scale = if axis-kind == "spread" { scale } else { other-scale }
    let evaluated = _evaluate-tree-bounds(
      tree-plan,
      style,
      x-scale,
      y-scale,
      orientation,
    )
    _bounds-span(evaluated.tree-occupied-bounds, screen-axis)
  }
  let best-fit = none
  let band-left = 0pt
  let band-right = 1pt

  for _ in range(_fit-max-bands) {
    let last-fit = none
    let first-fail-after-fit = none

    for sample in range(_fit-band-samples + 1) {
      let t = sample / _fit-band-samples
      let scale = band-left + (band-right - band-left) * t
      if _span-fits(evaluate-span(scale), viewport-limit-abs) {
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
        if _span-fits(evaluate-span(mid), viewport-limit-abs) {
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
/// - width (length, fraction): Target rendered width.
/// - height (length, auto): Target tree viewport height.
/// - layout-size (dictionary): Layout callback size.
/// -> dictionary
#let _fit-tree-plan(
  tree-plan,
  style,
  orientation,
  width,
  height,
  layout-size,
) = {
  let measured-plan = _measure-tree-primitives(tree-plan)
  let label-only-plan = _evaluate-tree-bounds(
    measured-plan,
    style,
    0pt,
    0pt,
    orientation,
  )
  let viewport-width = _resolve-length(_resolve-tree-width(width, layout-size))
  let viewport-height = if height == auto {
    calc.max(
      _resolve-length(style.auto-height-scale * measured-plan.tree-height),
      label-only-plan.tree-occupied-bounds.height,
    )
  } else {
    _resolve-length(height)
  }

  let x-scale = 0pt
  let y-scale = 0pt
  for _ in range(12) {
    let next-x = _solve-axis-scale(
      measured-plan,
      style,
      orientation,
      if orientation == "vertical" { viewport-height } else { viewport-width },
      "depth",
      other-scale: y-scale,
    )
    let next-y = _solve-axis-scale(
      measured-plan,
      style,
      orientation,
      if orientation == "vertical" { viewport-width } else { viewport-height },
      "spread",
      other-scale: next-x,
    )
    let x-converged = _resolve-length(calc.abs(next-x - x-scale)) <= _fit-scale-convergence
    let y-converged = _resolve-length(calc.abs(next-y - y-scale)) <= _fit-scale-convergence
    let converged = x-converged and y-converged
    if converged {
      x-scale = next-x
      y-scale = next-y
      break
    }
    x-scale = next-x
    y-scale = next-y
  }
  let evaluated-plan = _evaluate-tree-bounds(
    measured-plan,
    style,
    x-scale,
    y-scale,
    orientation,
  )

  let issues = ()
  if not _span-acceptable(evaluated-plan.tree-occupied-bounds.width, viewport-width) {
    issues.push(
      "width is too small for the tree labels and fixed margins (current: "
        + repr(_resolve-length(viewport-width))
        + ", required: >= "
        + repr(_resolve-length(evaluated-plan.tree-occupied-bounds.width))
        + ")",
    )
  }
  if not _span-acceptable(evaluated-plan.tree-occupied-bounds.height, viewport-height) {
    issues.push(
      "height is too small for the tree labels and fixed margins (current: "
        + repr(_resolve-length(viewport-height))
        + ", required: >= "
        + repr(_resolve-length(evaluated-plan.tree-occupied-bounds.height))
        + ")",
    )
  }
  assert(
    issues.len() == 0,
    message: "Tree cannot be rendered: "
      + issues.join("; ")
      + ". Increase width or height, reduce labels, reduce label size, or reduce root-length.",
  )

  let translate-x = (viewport-width - evaluated-plan.tree-occupied-bounds.width) / 2 - evaluated-plan.tree-occupied-bounds.min-x
  let translate-y = (viewport-height - evaluated-plan.tree-occupied-bounds.height) / 2 - evaluated-plan.tree-occupied-bounds.min-y
  let translated-primitives = ()
  for primitive in evaluated-plan.tree-primitives {
    if primitive.kind == "line" {
      let translated-line = primitive
      translated-line.insert(
        "start",
        _translate-point(primitive.start, translate-x, translate-y),
      )
      translated-line.insert(
        "end",
        _translate-point(primitive.end, translate-x, translate-y),
      )
      translated-primitives.push(translated-line)
    } else {
      let translated-label = primitive
      translated-label.insert(
        "origin",
        _translate-point(primitive.origin, translate-x, translate-y),
      )
      translated-primitives.push(translated-label)
    }
  }

  let translated-node-positions = (:)
  for id in range(measured-plan.node-count) {
    translated-node-positions.insert(
      _tree-node-key(id),
      _translate-point(
        evaluated-plan.node-positions.at(_tree-node-key(id)),
        translate-x,
        translate-y,
      ),
    )
  }

  (
    tree-primitives: translated-primitives,
    node-positions: translated-node-positions,
    nodes: measured-plan.nodes,
    root-id: measured-plan.root-id,
    node-count: measured-plan.node-count,
    tree-depth: measured-plan.tree-depth,
    tree-height: measured-plan.tree-height,
    x-scale: x-scale,
    y-scale: y-scale,
    orientation: orientation,
    tree-viewport-width: viewport-width,
    tree-viewport-height: viewport-height,
    tree-viewport-bounds: (
      min-x: 0pt,
      min-y: 0pt,
      max-x: viewport-width,
      max-y: viewport-height,
      width: viewport-width,
      height: viewport-height,
    ),
    tree-occupied-bounds: (
      min-x: evaluated-plan.tree-occupied-bounds.min-x + translate-x,
      min-y: evaluated-plan.tree-occupied-bounds.min-y + translate-y,
      max-x: evaluated-plan.tree-occupied-bounds.max-x + translate-x,
      max-y: evaluated-plan.tree-occupied-bounds.max-y + translate-y,
      width: evaluated-plan.tree-occupied-bounds.width,
      height: evaluated-plan.tree-occupied-bounds.height,
    ),
    tree-depth-span: x-scale * measured-plan.tree-depth,
  )
}

/// Builds the optional scale-bar row plan.
///
/// - fitted-plan (dictionary): Output from `_fit-tree-plan`.
/// - branch-color (color): Scale bar color.
/// - branch-weight (length): Scale bar stroke thickness.
/// - scale-length (auto, int, float): Requested scale length.
/// - scale-unit (str, none): Optional scale-bar unit.
/// - scale-tick-height (length): Tick height.
/// - scale-label-size (length): Label size.
/// - scale-label-gap (length): Gap between bar and label.
/// -> dictionary
#let _build-scale-plan(
  fitted-plan,
  branch-color,
  branch-weight,
  scale-length,
  scale-unit,
  scale-tick-height,
  scale-label-size,
  scale-label-gap,
) = {
  let row-width = fitted-plan.tree-viewport-width
  let root-position = fitted-plan.node-positions.at(_tree-node-key(fitted-plan.root-id))
  let bar-left = if fitted-plan.orientation == "vertical" { 0pt } else { root-position.x }
  let max-bar-width = if fitted-plan.orientation == "vertical" {
    calc.min(fitted-plan.tree-depth-span, row-width)
  } else {
    calc.max(0pt, fitted-plan.tree-depth-span)
  }
  let resolved-scale = _resolve-scale-bar-length(
    scale-length,
    fitted-plan.tree-depth,
    fitted-plan.x-scale,
    max-bar-width,
    zero-length-message: "Cannot render scale bar for zero-depth tree.",
  )
  let scale-label = _format-scale-label(resolved-scale.length, scale-unit)
  let scale-content = _draw-scale-bar-row(
    row-width,
    0pt,
    bar-left,
    resolved-scale.width,
    scale-tick-height,
    scale-label-gap,
    scale-label-size,
    branch-color,
    scale-label,
    branch-color,
    branch-weight,
  )
  let scale-size = measure(scale-content)

  (
    row-width: row-width,
    content: scale-content,
    scale-row-bounds: (
      min-x: 0pt,
      min-y: 0pt,
      max-x: row-width,
      max-y: scale-size.height,
      width: row-width,
      height: scale-size.height,
    ),
  )
}

/// Renders a fitted tree plan and optional scale-bar row.
///
/// - fitted-plan (dictionary): Output from `_fit-tree-plan`.
/// - scale-plan (dictionary, none): Optional scale row.
/// - scale-bar-gap (length): Gap between tree and scale bar.
/// -> content
#let _render-tree-plan(fitted-plan, scale-plan, scale-bar-gap) = {
  let tree-box = box(
    width: fitted-plan.tree-viewport-width,
    height: fitted-plan.tree-viewport-height,
    {
      for primitive in fitted-plan.tree-primitives {
        if primitive.kind == "line" {
          let dx = calc.abs(primitive.end.x - primitive.start.x)
          let dy = calc.abs(primitive.end.y - primitive.start.y)
          if dy <= _fit-tolerance {
            _draw-horizontal-segment(
              calc.min(primitive.start.x, primitive.end.x),
              primitive.start.y,
              dx,
              primitive.stroke,
            )
          } else if dx <= _fit-tolerance {
            _draw-vertical-segment(
              primitive.start.x,
              calc.min(primitive.start.y, primitive.end.y),
              dy,
              primitive.stroke,
            )
          } else {
            place(top + left, line(
              start: (primitive.start.x, primitive.start.y),
              end: (primitive.end.x, primitive.end.y),
              stroke: primitive.stroke,
            ))
          }
        }
      }
      for primitive in fitted-plan.tree-primitives {
        if primitive.kind == "label" {
          let label-content = _build-tree-label-content(primitive)
          place(
            top + left,
            dx: primitive.origin.x,
            dy: primitive.origin.y,
            if primitive.rotation == 0deg {
              label-content
            } else {
              rotate(primitive.rotation, origin: top + left, label-content)
            },
          )
        }
      }
    },
  )

  if scale-plan == none {
    tree-box
  } else {
    block(breakable: false, stack(
      spacing: scale-bar-gap,
      tree-box,
      scale-plan.content,
    ))
  }
}
