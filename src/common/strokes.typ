#import "./colors.typ": _dark-gray, _medium-gray, _yellow
#import "./layout_math.typ": _resolve-length

/// Default stroke for coordinate axes, scale bars, and label leader lines
#let _default-axis-stroke = stroke(
  thickness: 0.75pt,
  paint: black,
  cap: "butt",
)

/// Default stroke for filled-shape outlines, such as gene arrows
#let _default-outline-stroke = stroke(
  thickness: 0.75pt,
  paint: black,
  join: "miter",
)

/// Default stroke for matrix cell borders
#let _default-cell-stroke = stroke(thickness: 0.75pt, paint: _medium-gray)

/// Default stroke for tree branches
#let _default-branch-stroke = stroke(
  thickness: 0.75pt,
  paint: black,
  cap: "square",
)

/// Default stroke for leader lines connecting aligned tip labels to branches
#let _default-tip-leader-stroke = stroke(
  thickness: 0.75pt,
  paint: _medium-gray,
  cap: "square",
  dash: (1pt, 2.3pt),
)

/// Default stroke for dynamic-programming traceback arrows
#let _default-arrow-stroke = stroke(
  thickness: 0.75pt,
  paint: _medium-gray,
  cap: "round",
)

/// Default stroke for traceback arrows lying on the highlighted path
#let _default-path-arrow-stroke = stroke(
  thickness: 0.75pt,
  paint: _dark-gray,
  cap: "round",
)

/// Default stroke for the dynamic-programming traceback highlight
#let _default-path-stroke = stroke(
  thickness: 18pt,
  paint: _yellow.transparentize(50%),
  cap: "round",
  join: "round",
)

/// Resolves a user stroke and its absolute thickness for drawing and fitting.
///
/// Accepts anything `stroke()` accepts. An `auto` thickness falls back to the
/// ambient line style, then Typst's built-in default, and em thicknesses are
/// resolved so callers can reserve bleed in points. Must be called in context.
///
/// - value (stroke, length, color, gradient, tiling, dictionary, none): Stroke to
///   resolve.
/// - name (str): Public parameter name used in validation errors.
/// - cap (auto, str): Cap applied when the stroke leaves its cap as `auto`.
/// -> dictionary
#let _resolve-stroke(value, name, cap: auto) = {
  if value == none {
    return (style: none, thickness: 0pt)
  }
  let base = stroke(value)
  let thickness = _resolve-length(if base.thickness != auto {
    base.thickness
  } else if line.stroke.thickness != auto {
    line.stroke.thickness
  } else {
    1pt
  })
  assert(thickness > 0pt, message: name + " thickness must be positive.")
  let override-cap = cap != auto and base.cap == auto
  let style = if base.thickness == auto or override-cap {
    stroke((
      paint: base.paint,
      thickness: thickness,
      cap: if override-cap { cap } else { base.cap },
      join: base.join,
      dash: base.dash,
      miter-limit: base.miter-limit,
    ))
  } else {
    base
  }
  (style: style, thickness: thickness)
}
