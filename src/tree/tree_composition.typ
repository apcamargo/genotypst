#import "./tree_layout.typ": _tree-node-key
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
  if label-primitive.placement-role == "internal-label" {
    if label-primitive.text-fill == none {
      text(
        size: label-primitive.text-size,
        style: label-primitive.text-style,
        bottom-edge: "baseline",
      )[#label-primitive.text]
    } else {
      text(
        size: label-primitive.text-size,
        fill: label-primitive.text-fill,
        style: label-primitive.text-style,
        bottom-edge: "baseline",
      )[#label-primitive.text]
    }
  } else {
    if label-primitive.text-fill == none {
      text(
        size: label-primitive.text-size,
        style: label-primitive.text-style,
        bottom-edge: "descender",
      )[#label-primitive.text]
    } else {
      text(
        size: label-primitive.text-size,
        fill: label-primitive.text-fill,
        style: label-primitive.text-style,
        bottom-edge: "descender",
      )[#label-primitive.text]
    }
  }
}

/// Builds a line primitive record.
///
/// - role (str): Primitive role.
/// - node-id (int): Related node identifier.
/// - stroke (stroke): Stroke styling.
/// - stroke-thickness (length): Stroke thickness used for bounds.
/// - tree-start (dictionary): Start point in tree space.
/// - tree-end (dictionary): End point in tree space.
/// - page-start (dictionary): Start offset in page space.
/// - page-end (dictionary): End offset in page space.
/// -> dictionary
#let _line-primitive(
  role,
  node-id,
  stroke,
  stroke-thickness,
  tree-start,
  tree-end,
  page-start: _zero-point,
  page-end: _zero-point,
) = {
  (
    kind: "line",
    role: role,
    node-id: node-id,
    stroke: stroke,
    stroke-thickness: stroke-thickness,
    tree-start: tree-start,
    tree-end: tree-end,
    page-start: page-start,
    page-end: page-end,
  )
}

/// Builds a label primitive record.
///
/// - role (str): Primitive role.
/// - node-id (int): Related node identifier.
/// - anchor-tree (dictionary): Anchor point in tree space.
/// - text (str): Label text.
/// - text-size (length): Font size.
/// - text-fill (color, none): Label color.
/// - text-style (str): Text style.
/// - rotation-mode (str): Rotation behavior during orientation transforms.
/// -> dictionary
#let _label-primitive(
  role,
  node-id,
  anchor-tree,
  text,
  text-size,
  text-fill,
  text-style,
  rotation-mode,
) = {
  (
    kind: "label",
    role: role,
    placement-role: role,
    node-id: node-id,
    anchor-tree: anchor-tree,
    anchor-page: _zero-point,
    text: text,
    text-size: text-size,
    text-fill: text-fill,
    text-style: text-style,
    rotation-mode: rotation-mode,
    rotation-origin: "top-left",
    measure-width: 0pt,
    measure-height: 0pt,
  )
}

/// Builds explicit tree primitives from a laid-out normalized tree.
///
/// - layout-tree (dictionary): Output from `_layout-tree`.
/// - style (dictionary): Tree styling configuration.
/// -> dictionary
#let _build-tree-plan(layout-tree, style) = {
  let primitives = ()
  let nodes = layout-tree.nodes
  let root = nodes.at(_tree-node-key(layout-tree.root-id))

  if root.input-rooted {
    primitives.push(_line-primitive(
      "root-edge",
      root.id,
      style.branch-stroke,
      style.branch-weight,
      (x: root.x-unit, y: root.y-unit),
      (x: root.x-unit, y: root.y-unit),
      page-start: (x: -style.root-length, y: 0pt),
      page-end: _zero-point,
    ))
  }

  for id in range(layout-tree.node-count) {
    let node = nodes.at(_tree-node-key(id))
    let node-point = (x: node.x-unit, y: node.y-unit)

    if not node.is-root {
      let parent = nodes.at(_tree-node-key(node.parent-id))
      primitives.push(_line-primitive(
        "branch-horizontal",
        node.id,
        style.branch-stroke,
        style.branch-weight,
        (x: parent.x-unit, y: node.y-unit),
        node-point,
      ))
    }

    if node.is-leaf {
      if node.label-text != none {
        primitives.push(_label-primitive(
          "tip-label",
          node.id,
          node-point,
          node.label-text,
          style.tip-label-size,
          style.tip-label-color,
          if style.tip-label-italics { "italic" } else { "normal" },
          "rotate-with-tree",
        ))
      }
    } else {
      let first-child = nodes.at(_tree-node-key(node.children-ids.first()))
      let last-child = nodes.at(_tree-node-key(node.children-ids.last()))
      primitives.push(_line-primitive(
        "branch-vertical",
        node.id,
        style.branch-stroke,
        style.branch-weight,
        (x: node.x-unit, y: first-child.y-unit),
        (x: node.x-unit, y: last-child.y-unit),
      ))

      if node.label-text != none {
        primitives.push(_label-primitive(
          "internal-label",
          node.id,
          node-point,
          node.label-text,
          style.internal-label-size,
          style.internal-label-color,
          "normal",
          "stay-horizontal",
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

/// Number of samples used while locating the feasible fit band.
#let _fit-band-samples = 24

/// Maximum number of exponentially growing fit bands to explore.
#let _fit-max-bands = 24

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

/// Resolves fit-time style lengths and normalizes primitive page-space offsets.
///
/// This keeps later fit evaluation in absolute lengths and lets downstream code
/// treat rooted and non-rooted primitives uniformly.
///
/// The current tree primitive model only uses a non-zero page-space offset for
/// the rooted edge, so normalization resolves that offset once here and clears
/// the remaining page-space offsets.
///
/// - tree-plan (dictionary): Measured tree primitive plan.
/// - style (dictionary): Tree style configuration.
/// -> dictionary: `(fit-offsets: ..., fit-plan: ...)`
#let _prepare-fit-inputs(tree-plan, style) = {
  let resolved-root-length = _resolve-length(style.root-length)
  let fit-offsets = (
    label-x-offset: _resolve-length(style.label-x-offset),
    internal-label-gap: _resolve-length(style.internal-label-gap),
    label-y-offset: _resolve-length(style.label-y-offset),
  )
  let fit-primitives = ()
  for primitive in tree-plan.tree-primitives {
    let fit-primitive = primitive
    if primitive.kind == "line" {
      fit-primitive.insert(
        "page-start",
        if primitive.role == "root-edge" {
          (x: -resolved-root-length, y: 0pt)
        } else {
          _zero-point
        },
      )
      fit-primitive.insert("page-end", _zero-point)
      fit-primitive.insert(
        "half-stroke",
        _resolve-length(primitive.stroke-thickness) / 2,
      )
    } else {
      fit-primitive.insert("anchor-page", _zero-point)
    }
    fit-primitives.push(fit-primitive)
  }
  let fit-plan = tree-plan
  fit-plan.insert("tree-primitives", fit-primitives)
  (fit-offsets: fit-offsets, fit-plan: fit-plan)
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

/// Resolves a normalized line primitive into screen coordinates.
///
/// - primitive (dictionary): Line primitive with absolute page offsets.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _materialize-line(primitive, x-scale, y-scale, orientation) = {
  let start = _transform-point(
    primitive.tree-start.x * x-scale + primitive.page-start.x,
    primitive.tree-start.y * y-scale + primitive.page-start.y,
    orientation,
  )
  let end = _transform-point(
    primitive.tree-end.x * x-scale + primitive.page-end.x,
    primitive.tree-end.y * y-scale + primitive.page-end.y,
    orientation,
  )
  (start: start, end: end)
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

/// Returns whether a normalized line primitive has non-zero geometry.
///
/// - primitive (dictionary): Line primitive with normalized page offsets.
/// -> bool
#let _line-has-extent(primitive) = {
  (
    primitive.tree-start.x != primitive.tree-end.x
      or primitive.tree-start.y != primitive.tree-end.y
      or primitive.page-start.x != primitive.page-end.x
      or primitive.page-start.y != primitive.page-end.y
  )
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

/// Projects bounds onto one screen axis.
///
/// - bounds (dictionary): Bounds record.
/// - axis (str): "x" or "y".
/// -> dictionary
#let _bounds-axis-interval(bounds, axis) = {
  if axis == "x" {
    (min: bounds.min-x, max: bounds.max-x)
  } else {
    (min: bounds.min-y, max: bounds.max-y)
  }
}

/// Evaluates bounds and fitted primitives for a pair of axis scales.
///
/// - tree-plan (dictionary): Prepared tree primitive plan for fitting.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - x-scale (length): Depth-axis scale.
/// - y-scale (length): Spread-axis scale.
/// - orientation (str): Tree orientation.
/// -> dictionary
#let _evaluate-tree-bounds(
  tree-plan,
  fit-offsets,
  x-scale,
  y-scale,
  orientation,
) = {
  let fitted-primitives = ()
  let bounds = _empty-bounds()
  let node-positions = (:)

  for id in range(tree-plan.node-count) {
    let node = tree-plan.nodes.at(_tree-node-key(id))
    node-positions.insert(_tree-node-key(id), _transform-point(
      node.x-unit * x-scale,
      node.y-unit * y-scale,
      orientation,
    ))
  }

  for primitive in tree-plan.tree-primitives {
    if primitive.kind == "line" {
      let resolved-line = _materialize-line(
        primitive,
        x-scale,
        y-scale,
        orientation,
      )
      if not _line-is-degenerate(resolved-line) {
        let line-bounds = _line-bounds(
          resolved-line.start,
          resolved-line.end,
          primitive.half-stroke,
        )
        bounds = _expand-bounds(
          bounds,
          line-bounds.min-x,
          line-bounds.min-y,
          line-bounds.max-x,
          line-bounds.max-y,
        )
        let fitted-line = primitive
        fitted-line.insert("start", resolved-line.start)
        fitted-line.insert("end", resolved-line.end)
        fitted-primitives.push(fitted-line)
      }
    } else {
      let resolved-label = _materialize-label-origin(
        primitive,
        fit-offsets,
        x-scale,
        y-scale,
        orientation,
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
/// - tree-plan (dictionary): Prepared tree primitive plan for fitting.
/// - fit-offsets (dictionary): Absolute fit offsets from `_prepare-fit-inputs`.
/// - orientation (str): Tree orientation.
/// - viewport-limit (length): Available size on the constrained screen axis.
/// - axis-kind (str): "depth" or "spread".
/// -> length
#let _solve-axis-scale(
  tree-plan,
  fit-offsets,
  orientation,
  viewport-limit,
  axis-kind,
) = {
  let tree-axis = if axis-kind == "depth" { "x" } else { "y" }
  let tree-extent = if tree-axis == "x" {
    tree-plan.tree-depth
  } else {
    tree-plan.tree-height
  }
  if tree-extent <= 0 { return 0pt }

  let screen-axis = if orientation == "vertical" {
    if tree-axis == "x" { "y" } else { "x" }
  } else {
    tree-axis
  }
  let evaluate-span = scale => {
    let x-scale = if tree-axis == "x" { scale } else { 0pt }
    let y-scale = if tree-axis == "y" { scale } else { 0pt }
    let min-edge = none
    let max-edge = none

    for primitive in tree-plan.tree-primitives {
      let interval = if primitive.kind == "line" {
        let resolved-line = _materialize-line(
          primitive,
          x-scale,
          y-scale,
          orientation,
        )
        if (
          _line-is-degenerate(resolved-line) and not _line-has-extent(primitive)
        ) {
          none
        } else {
          _bounds-axis-interval(
            _line-bounds(
              resolved-line.start,
              resolved-line.end,
              primitive.half-stroke,
            ),
            screen-axis,
          )
        }
      } else {
        let resolved-label = _materialize-label-origin(
          primitive,
          fit-offsets,
          x-scale,
          y-scale,
          orientation,
        )
        let label-bounds = _label-bounds(
          resolved-label.origin,
          primitive.measure-width,
          primitive.measure-height,
          resolved-label.rotation,
        )
        _bounds-axis-interval(label-bounds, screen-axis)
      }

      if interval != none {
        if min-edge == none {
          min-edge = interval.min
          max-edge = interval.max
        } else {
          min-edge = calc.min(min-edge, interval.min)
          max-edge = calc.max(max-edge, interval.max)
        }
      }
    }

    if min-edge == none { 0pt } else { max-edge - min-edge }
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
/// - width (length, fraction): Target rendered width.
/// - height (length, auto): Target rendered tree height.
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
  let fit-inputs = _prepare-fit-inputs(measured-plan, style)
  let fit-plan = fit-inputs.fit-plan
  let fit-offsets = fit-inputs.fit-offsets
  let label-only-plan = _evaluate-tree-bounds(
    fit-plan,
    fit-offsets,
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

  let x-scale = _solve-axis-scale(
    fit-plan,
    fit-offsets,
    orientation,
    if orientation == "vertical" { viewport-height } else { viewport-width },
    "depth",
  )
  let y-scale = _solve-axis-scale(
    fit-plan,
    fit-offsets,
    orientation,
    if orientation == "vertical" { viewport-width } else { viewport-height },
    "spread",
  )
  let evaluated-plan = _evaluate-tree-bounds(
    fit-plan,
    fit-offsets,
    x-scale,
    y-scale,
    orientation,
  )

  let issues = ()
  if not _span-acceptable(
    evaluated-plan.tree-occupied-bounds.width,
    viewport-width,
  ) {
    issues.push(
      "width is too small for the tree labels and fixed margins (current: "
        + repr(viewport-width)
        + ", required: >= "
        + repr(evaluated-plan.tree-occupied-bounds.width)
        + ")",
    )
  }
  if not _span-acceptable(
    evaluated-plan.tree-occupied-bounds.height,
    viewport-height,
  ) {
    issues.push(
      "height is too small for the tree labels and fixed margins (current: "
        + repr(viewport-height)
        + ", required: >= "
        + repr(evaluated-plan.tree-occupied-bounds.height)
        + ")",
    )
  }
  assert(
    issues.len() == 0,
    message: "Tree cannot be rendered: "
      + issues.join("; ")
      + ". Increase width or height, reduce labels, reduce label size, or reduce root-length.",
  )

  let translate-x = (
    (viewport-width - evaluated-plan.tree-occupied-bounds.width) / 2
      - evaluated-plan.tree-occupied-bounds.min-x
  )
  let translate-y = (
    (viewport-height - evaluated-plan.tree-occupied-bounds.height) / 2
      - evaluated-plan.tree-occupied-bounds.min-y
  )
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
/// - scale-length (auto, int, float): Requested scale length in branch-length units.
/// - unit (str, none): Optional scale-bar unit.
/// - min-auto-bar-width (length): Minimum rendered width used in auto mode.
/// - scale-tick-height (length): Tick height.
/// - scale-label-size (length): Label size.
/// -> dictionary
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
  let root-position = fitted-plan.node-positions.at(_tree-node-key(
    fitted-plan.root-id,
  ))
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
    fitted-plan.tree-depth,
    fitted-plan.x-scale,
    max-bar-width,
    min-auto-bar-width: min-auto-bar-width,
    zero-length-message: "Cannot render scale bar for zero-depth tree.",
  )
  let scale-label = _format-scale-label(resolved-scale.length, unit)
  let scale-label-gap = 1.5pt
  let scale-content = _draw-scale-bar-row(
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
