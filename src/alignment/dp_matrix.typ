#import "../common/colors.typ": _dark-gray, _medium-gray, _yellow
#import "./pair_alignment.typ": (
  _parse-and-validate-coord, _parse-coord, _validate-path,
)
#import "@preview/tiptoe:0.4.0": (
  line as _tiptoe-line, straight as _tiptoe-straight,
)

/// Private: Validate highlight entry shape, coordinates, and optional color.
#let _validate-highlights(highlights, max-row, max-col) = {
  assert(type(highlights) == array, message: "highlights must be an array.")

  for (idx, highlight) in highlights.enumerate() {
    assert(
      type(highlight) == array
        and (highlight.len() == 2 or highlight.len() == 3),
      message: "Highlight at index "
        + str(idx)
        + " must be (row, col) or (row, col, color).",
    )

    let _ = _parse-and-validate-coord(
      (highlight.at(0), highlight.at(1)),
      max-row,
      max-col,
      "Highlight at index " + str(idx),
    )

    if highlight.len() == 3 {
      assert(
        type(highlight.at(2)) == color,
        message: "Highlight at index "
          + str(idx)
          + " color must be a color value.",
      )
    }
  }

  none
}

/// Private: Validate flat arrow list structure and coordinate bounds.
#let _validate-arrows(arrows, max-row, max-col) = {
  assert(type(arrows) == array, message: "arrows must be an array.")
  if arrows.len() == 0 { return none }

  for (idx, arrow) in arrows.enumerate() {
    assert(
      type(arrow) == array and arrow.len() == 2,
      message: "Arrow at index " + str(idx) + " must have (from, to).",
    )

    let from = _parse-and-validate-coord(
      arrow.at(0),
      max-row,
      max-col,
      "Arrow at index " + str(idx) + " from",
    )
    let to = _parse-and-validate-coord(
      arrow.at(1),
      max-row,
      max-col,
      "Arrow at index " + str(idx) + " to",
    )

    let row-delta = calc.abs(from.row - to.row)
    let col-delta = calc.abs(from.col - to.col)

    assert(
      row-delta + col-delta > 0,
      message: "Arrow at index "
        + str(idx)
        + " cannot have identical from/to coordinates.",
    )
    assert(
      calc.max(row-delta, col-delta) == 1,
      message: "Arrow at index " + str(idx) + " must connect adjacent cells.",
    )
  }

  none
}

/// Private: Validate sparse DP matrix entries and bounds.
#let _validate-dp-cell-values(cell-values, expected-rows, expected-cols) = {
  assert(type(cell-values) == array, message: "cell-values must be an array.")
  let max-row = expected-rows - 1
  let max-col = expected-cols - 1
  let seen = (:)

  for (idx, entry) in cell-values.enumerate() {
    assert(
      type(entry) == array and entry.len() == 2,
      message: "Cell value at index " + str(idx) + " must have (coord, value).",
    )

    let coord = _parse-and-validate-coord(
      entry.at(0),
      max-row,
      max-col,
      "Cell value at index " + str(idx) + " coordinate",
    )
    let value = entry.at(1)

    assert(
      type(value) == int or type(value) == float,
      message: "Cell value at index " + str(idx) + " must be numeric.",
    )

    let key = str(coord.row) + "," + str(coord.col)
    assert(
      not (key in seen),
      message: "Duplicate cell value entry for coordinate ("
        + str(coord.row)
        + ", "
        + str(coord.col)
        + ").",
    )
    seen.insert(key, true)
  }
}

/// Private: Convert sparse cell value entries to a coordinate map.
#let _cell-values-to-map(cell-values) = {
  let cell-map = (:)

  for entry in cell-values {
    let coord = _parse-coord(entry.at(0))
    let key = str(coord.row) + "," + str(coord.col)
    cell-map.insert(key, entry.at(1))
  }

  cell-map
}

/// Private: Calculate cell center coordinates.
#let _cell-center(row, col, label-col-width, label-row-height, cell-size) = {
  let x = label-col-width + col * cell-size + cell-size * 0.5
  let y = label-row-height + row * cell-size + cell-size * 0.5
  (x: x, y: y)
}

/// Private: Create a label cell (for header row and left column).
#let _label-cell(content) = grid.cell(stroke: none, inset: 0pt)[
  #if content != none { align(center + horizon)[#content] }
]

/// Private: Determine radius for a cell based on its position.
#let _get-cell-radius(row-idx, col-idx, last-row, last-col, corner-radius) = {
  let is-top = row-idx == 0
  let is-bottom = row-idx == last-row
  let is-left = col-idx == 0
  let is-right = col-idx == last-col

  if is-top and is-left {
    (top-left: corner-radius, rest: 0pt)
  } else if is-top and is-right {
    (top-right: corner-radius, rest: 0pt)
  } else if is-bottom and is-left {
    (bottom-left: corner-radius, rest: 0pt)
  } else if is-bottom and is-right {
    (bottom-right: corner-radius, rest: 0pt)
  } else {
    0pt
  }
}

/// Private: Build grid content arrays for background and text layers.
#let _build-grid-content(
  top-clusters,
  left-clusters,
  cell-value-map,
  highlights,
  highlight-color,
  path,
  path-cell-bold,
  stroke-width,
  stroke-color,
  cell-inset,
  corner-radius,
) = {
  let bg-grid-content = ()
  let text-grid-content = ()

  let key-of = (row, col) => str(row) + "," + str(col)

  let highlight-map = (:)
  for h in highlights {
    let h-coord = _parse-coord(h)
    let key = key-of(h-coord.row, h-coord.col)

    // Preserve existing behavior: first matching highlight wins.
    if not (key in highlight-map) {
      let color = if h.len() > 2 { h.at(2) } else { highlight-color }
      highlight-map.insert(key, color)
    }
  }

  let path-set = (:)
  if path-cell-bold and path != none {
    for p in path {
      let p-coord = _parse-coord(p)
      path-set.insert(key-of(p-coord.row, p-coord.col), true)
    }
  }

  // Header row: empty top-left corner, then top sequence characters
  bg-grid-content.push(_label-cell(none))
  text-grid-content.push(_label-cell(none))

  for char in top-clusters {
    bg-grid-content.push(_label-cell(none))
    text-grid-content.push(_label-cell(char))
  }

  // Calculate last row and column indices
  let last-row = left-clusters.len() - 1
  let last-col = top-clusters.len() - 1

  // Data rows: left label, then cell values
  for (row-idx, row-label) in left-clusters.enumerate() {
    bg-grid-content.push(_label-cell(none))
    text-grid-content.push(_label-cell(row-label))

    for col-idx in range(top-clusters.len()) {
      let key = key-of(row-idx, col-idx)
      let value = cell-value-map.at(key, default: none)
      let cell-content = if value == none {
        []
      } else {
        let content = if path-cell-bold and (key in path-set) {
          strong[#value]
        } else {
          value
        }
        align(center + horizon)[#content]
      }

      let fill-color = highlight-map.at(key, default: none)
      let cell-radius = _get-cell-radius(
        row-idx,
        col-idx,
        last-row,
        last-col,
        corner-radius,
      )

      // Background layer: boxes with rounded corners and fills
      bg-grid-content.push(box(
        width: 100%,
        height: 100%,
        fill: fill-color,
        stroke: stroke-width + stroke-color,
        radius: cell-radius,
        inset: cell-inset,
      )[])

      // Text layer: only text, no fills
      text-grid-content.push(box(
        width: 100%,
        height: 100%,
        inset: cell-inset,
      )[#cell-content])
    }
  }

  (bg: bg-grid-content, text: text-grid-content)
}

/// Private: Render path overlay.
#let _render-path(
  path,
  path-color,
  path-width,
  label-col-width,
  label-row-height,
  cell-size,
) = {
  if path == none or path.len() <= 1 {
    return
  }

  // Calculate path coordinates
  let path-coords = path.map(pt => {
    let coord = _parse-coord(pt)
    let center = _cell-center(
      coord.row,
      coord.col,
      label-col-width,
      label-row-height,
      cell-size,
    )
    (center.x, center.y)
  })

  // Draw continuous path through all points
  place(top + left, dx: 0pt, dy: 0pt, {
    let curve-components = (curve.move(path-coords.at(0)),)
    for i in range(1, path-coords.len()) {
      curve-components.push(curve.line(path-coords.at(i)))
    }

    curve(
      stroke: (
        paint: path-color,
        thickness: path-width,
        cap: "round",
        join: "round",
      ),
      ..curve-components,
    )
  })
}

/// Private: Check if an arrow is part of the traceback path.
#let _is-arrow-on-path(arrow-from, arrow-to, path) = {
  if path == none or path.len() < 2 {
    return false
  }
  let parsed-arrow-from = _parse-coord(arrow-from)
  let parsed-arrow-to = _parse-coord(arrow-to)

  // Path is ordered from end to start, so consecutive pairs are (from, to)
  for i in range(path.len() - 1) {
    let path-from = _parse-coord(path.at(i))
    let path-to = _parse-coord(path.at(i + 1))
    if (
      parsed-arrow-from.row == path-from.row
        and parsed-arrow-from.col == path-from.col
        and parsed-arrow-to.row == path-to.row
        and parsed-arrow-to.col == path-to.col
    ) {
      return true
    }
  }
  false
}

/// Private: Calculate arrow start and end positions based on direction.
#let _calculate-arrow-positions(
  from-coord,
  to-coord,
  center-x,
  center-y,
  arrow-half-length,
) = {
  if from-coord.row == to-coord.row {
    (
      center-x + arrow-half-length,
      center-y,
      center-x - arrow-half-length,
      center-y,
    )
  } else if from-coord.col == to-coord.col {
    (
      center-x,
      center-y + arrow-half-length,
      center-x,
      center-y - arrow-half-length,
    )
  } else {
    let dx-sign = if to-coord.col < from-coord.col { -1 } else { 1 }
    let dy-sign = if to-coord.row < from-coord.row { -1 } else { 1 }
    let diag-offset = arrow-half-length * 0.85
    (
      center-x - dx-sign * diag-offset,
      center-y - dy-sign * diag-offset,
      center-x + dx-sign * diag-offset,
      center-y + dy-sign * diag-offset,
    )
  }
}

/// Private: Render all arrows.
#let _render-arrows(
  arrows,
  arrow-color,
  cell-size,
  label-col-width,
  label-row-height,
  path,
  highlight-path-arrows,
  path-arrow-color,
  arrow-width,
  arrow-length-scale,
) = {
  for arrow in arrows {
    let from-coord = _parse-coord(arrow.at(0))
    let to-coord = _parse-coord(arrow.at(1))

    let arr-color = arrow-color
    if (
      highlight-path-arrows
        and _is-arrow-on-path(arrow.at(0), arrow.at(1), path)
    ) {
      arr-color = path-arrow-color
    }

    let from-center = _cell-center(
      from-coord.row,
      from-coord.col,
      label-col-width,
      label-row-height,
      cell-size,
    )
    let to-center = _cell-center(
      to-coord.row,
      to-coord.col,
      label-col-width,
      label-row-height,
      cell-size,
    )

    let center-x = (from-center.x + to-center.x) / 2.0
    let center-y = (from-center.y + to-center.y) / 2.0
    let arrow-half-length = cell-size * 0.215 * arrow-length-scale

    let (start-x, start-y, end-x, end-y) = _calculate-arrow-positions(
      from-coord,
      to-coord,
      center-x,
      center-y,
      arrow-half-length,
    )

    place(top + left, dx: 0pt, dy: 0pt, {
      _tiptoe-line(
        start: (start-x, start-y),
        end: (end-x, end-y),
        stroke: (
          paint: arr-color,
          thickness: arrow-width,
          cap: "round",
        ),
        tip: _tiptoe-straight.with(width: 550%, length: 375%),
      )
    })
  }
}

/// Renders a dynamic programming matrix for sequence alignment visualization.
///
/// Creates a visual representation of a dynamic programming (DP) matrix with
/// optional cell highlighting, traceback path overlay, and arrow indicators for
/// alignment directions.
///
/// - seq-1 (str): Sequence displayed on the left as row labels.
/// - seq-2 (str): Sequence displayed on top as column labels.
/// - cell-values (array, none): Flat array of `((row, col), value)` entries.
///   Omitted coordinates render as blank cells (default: none).
/// - highlights (array): Cell highlights as `(row, col)` or `(row, col, color)` arrays (default: ()).
/// - highlight-color (color): Default color for highlighted cells (default: light gray).
/// - path (array, none): Traceback path as `(row, col)` arrays, in end-to-start order (default: none).
/// - path-color (color): Color for the path line (default: semi-transparent yellow).
/// - path-width (length): Width of the path line (default: 18pt).
/// - path-cell-bold (bool): Whether scores in cells on the path are rendered in bold (default: true).
/// - arrows (array): Flat array of (from, to) coordinate pairs, one per arrow (default: ()).
/// - arrow-color (color): Default color for arrows (default: medium gray).
/// - highlight-path-arrows (bool): Whether arrows on the path use a different color (default: true).
/// - path-arrow-color (color): Color for arrows on the traceback path (default: dark gray).
/// - arrow-width (length): Width of the arrows (default: 1pt).
/// - arrow-length-scale (int, float): Multiplier for arrow length (default: 1).
/// - cell-size (length): Size of each square cell (default: 34pt).
/// - stroke-width (length): Width of cell borders (default: 0.75pt).
/// - stroke-color (color): Color of cell borders (default: medium gray).
/// -> content
#let render-dp-matrix(
  seq-1,
  seq-2,
  cell-values: none,
  highlights: (),
  highlight-color: _medium-gray.lighten(75%),
  path: none,
  path-color: _yellow.transparentize(50%),
  path-width: 18pt,
  path-cell-bold: true,
  arrows: (),
  arrow-color: _medium-gray,
  highlight-path-arrows: true,
  path-arrow-color: _dark-gray,
  arrow-width: 1pt,
  arrow-length-scale: 1,
  cell-size: 34pt,
  stroke-width: 0.75pt,
  stroke-color: _medium-gray,
) = {
  assert(type(seq-1) == str, message: "seq-1 must be a string.")
  assert(type(seq-2) == str, message: "seq-2 must be a string.")
  assert(type(arrow-width) == length, message: "arrow-width must be a length.")
  assert(
    type(arrow-length-scale) == int or type(arrow-length-scale) == float,
    message: "arrow-length-scale must be numeric.",
  )
  assert(
    arrow-length-scale > 0,
    message: "arrow-length-scale must be greater than 0.",
  )

  let seq1-raw-clusters = seq-1.clusters()
  let seq2-raw-clusters = seq-2.clusters()
  let expected-rows = seq1-raw-clusters.len() + 1
  let expected-cols = seq2-raw-clusters.len() + 1
  let resolved-cell-values = if cell-values == none {
    ()
  } else {
    _validate-dp-cell-values(cell-values, expected-rows, expected-cols)
    cell-values
  }
  let cell-value-map = _cell-values-to-map(resolved-cell-values)

  let top-label-seq = "–" + seq-2
  let left-label-seq = "–" + seq-1

  let top-clusters = top-label-seq.clusters()
  let left-clusters = left-label-seq.clusters()

  let max-row = left-clusters.len() - 1
  let max-col = top-clusters.len() - 1

  _validate-highlights(highlights, max-row, max-col)
  _validate-arrows(arrows, max-row, max-col)

  if path != none {
    _validate-path(path.rev(), max-row, max-col)
  }

  let label-scale = 0.65
  let cell-inset = 5pt
  let corner-radius = 3pt

  let label-col-width = cell-size * label-scale
  let label-row-height = cell-size * label-scale

  let grid-content = _build-grid-content(
    top-clusters,
    left-clusters,
    cell-value-map,
    highlights,
    highlight-color,
    path,
    path-cell-bold,
    stroke-width,
    stroke-color,
    cell-inset,
    corner-radius,
  )

  let column-widths = (label-col-width,) + ((cell-size,) * top-clusters.len())
  let row-heights = (label-row-height,) + ((cell-size,) * left-clusters.len())

  let bg-grid = grid(
    columns: column-widths,
    rows: row-heights,
    stroke: none,
    inset: 0pt,
    ..grid-content.bg
  )

  let text-grid = grid(
    columns: column-widths,
    rows: row-heights,
    stroke: none,
    inset: 0pt,
    ..grid-content.text
  )

  if path == none and arrows.len() == 0 {
    return block(breakable: false, {
      bg-grid
      place(top + left, dx: 0pt, dy: 0pt, text-grid)
    })
  }

  block(breakable: false, {
    bg-grid

    _render-path(
      path,
      path-color,
      path-width,
      label-col-width,
      label-row-height,
      cell-size,
    )

    place(top + left, dx: 0pt, dy: 0pt, text-grid)

    _render-arrows(
      arrows,
      arrow-color,
      cell-size,
      label-col-width,
      label-row-height,
      path,
      highlight-path-arrows,
      path-arrow-color,
      arrow-width,
      arrow-length-scale,
    )
  })
}
