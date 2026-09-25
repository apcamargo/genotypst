// Draws RNA secondary structures with Typst.

#import "../common/colors.typ": _blue, _light-gray, _light-medium-gray, _red
#import "../common/fixed_grid.typ": _measure-monospace-width
#import "../common/layout_math.typ": (
  _assert-length, _render-width-is-valid, _resolve-length, _width-mode,
)
#import "../common/strokes.typ": _resolve-stroke
#import "../sequence/sequence_processing.typ": (
  _assert-palette-coverage, _derive-residue-colors, _lookup-palette-entry,
  _prepare-palette,
)

#let _rna-structure-plugin = plugin("rna_structure.wasm")
#let _rna-render-layout = layout
#let _accepted-layouts = (
  "radial",
  "naview",
  "rna_turtle",
  "rna_puzzler",
  "circular",
)

/// Gets drawing geometry from the bundled RNA layout plugin.
///
/// - sequence (str): Nucleotide sequence.
/// - structure (str): Dot-bracket structure matching `sequence`.
/// - layout (str): RNA layout algorithm.
/// -> dictionary
#let _layout-data(sequence, structure, layout) = {
  let config = bytes(json.encode((algorithm: layout), pretty: false))
  json(_rna-structure-plugin.layout(
    bytes(sequence),
    bytes(structure),
    config,
  ))
}

/// Converts a fitted point in pt units to a Typst coordinate array.
///
/// - point (dictionary): Point with numeric `x` and `y` fields in pt units.
/// -> array
#let _page-point(point) = (point.x * 1pt, point.y * 1pt)

/// Places a fitted line when both its geometry and stroke are available.
///
/// - line-data (dictionary, none): Fitted line with pt-valued endpoints.
/// - line-stroke (stroke, none): Stroke to apply.
/// -> content
#let _place-rna-line(line-data, line-stroke) = {
  if line-data != none and line-stroke != none {
    place(top + left, line(
      start: _page-point(line-data.start_pt),
      end: _page-point(line-data.end_pt),
      stroke: line-stroke,
    ))
  }
}

#let _default-rna-backbone-stroke = stroke(
  thickness: 1pt,
  paint: black,
)

#let _default-rna-base-pair-stroke = stroke(
  thickness: 1pt,
  paint: _light-medium-gray,
  dash: "densely-dotted",
)

#let _default-rna-tick-stroke = stroke(
  thickness: 1pt,
  paint: _light-medium-gray,
)

/// Returns the geometry scale below which a connector between two nucleotides
/// is hidden.
///
/// The connector is visible once the scaled span clears both nucleotide radii
/// plus the minimum visible length.
///
/// - span (float): Connector span in layout geometry units.
/// - nucleotide-radius (float): Nucleotide radius in points.
/// - minimum-visible-length (float): Shortest visible connector length in
///   points.
/// -> float
#let _connector-min-scale(span, nucleotide-radius, minimum-visible-length) = {
  if span > 0 {
    (2 * nucleotide-radius + minimum-visible-length) / span
  } else {
    0.0
  }
}

/// Builds measured RNA drawing primitives without placing content.
///
/// Keeps serializable geometry separate from Typst content for the fitting
/// plugin.
///
/// - sequence (str): Nucleotide sequence.
/// - layout-data (dictionary): Layout data from the RNA plugin.
/// - config (dictionary): Validated render options with keys:
///   - backbone-bond-length (length): Resolved target backbone bond length.
///   - minimum-connector-length (length): Resolved shortest visible backbone
///     connector.
///   - backbone-stroke (stroke, none): Backbone stroke.
///   - base-pair-stroke (stroke, none): Base-pair stroke.
///   - show-nucleotide-circles (bool): Whether to draw nucleotide circles.
///   - palette (dictionary, none): Nucleotide color palette.
///   - nucleotide-padding (length): Resolved bare-letter connector clearance.
///   - circle-padding (length): Resolved circle interior clearance.
///   - position-interval (int, none): Numbered-tick interval.
///   - position-tick-length (length, auto): Resolved position-tick length.
///   - position-label-gap (length): Resolved tick-label clearance.
///   - tick-stroke (stroke, none): Tick and terminal-leader stroke.
///   - show-terminal-labels (bool): Whether to add 5' and 3' labels.
///   - five-prime-label-color (color): 5' terminal-label color.
///   - three-prime-label-color (color): 3' terminal-label color.
/// -> dictionary
#let _prepare-rna-plan(sequence, layout-data, config) = {
  let (
    backbone-bond-length,
    minimum-connector-length,
    backbone-stroke,
    base-pair-stroke,
    show-nucleotide-circles,
    palette,
    nucleotide-padding,
    circle-padding,
    position-interval,
    position-tick-length,
    position-label-gap,
    tick-stroke,
    show-terminal-labels,
    five-prime-label-color,
    three-prime-label-color,
  ) = config
  let coordinates = layout-data.coordinates
  let nucleotides = sequence.clusters()
  let nucleotide-count = coordinates.len()
  // Add fixed padding around each nucleotide. Bare letters use
  // nucleotide-padding, and circles use circle-padding inside their edges.
  let nucleotide-padding-pt = nucleotide-padding / 1pt
  let bare-nucleotide-radius = if show-nucleotide-circles {
    none
  } else {
    _measure-monospace-width() / 2 / 1pt + nucleotide-padding-pt
  }
  let tick-length = if position-tick-length == auto {
    0.0
  } else {
    position-tick-length / 1pt
  }
  let label-gap = position-label-gap / 1pt
  let resolved-backbone-stroke = _resolve-stroke(
    backbone-stroke,
    "backbone-stroke",
  )
  let resolved-base-pair-stroke = _resolve-stroke(
    base-pair-stroke,
    "base-pair-stroke",
  )
  let resolved-tick-stroke = _resolve-stroke(tick-stroke, "tick-stroke")
  let backbone-half-stroke = resolved-backbone-stroke.thickness / 2 / 1pt
  let pair-half-stroke = resolved-base-pair-stroke.thickness / 2 / 1pt
  let minimum-connector-length-pt = minimum-connector-length / 1pt
  let backbone-bond-length-pt = backbone-bond-length / 1pt
  let tick-half-stroke = resolved-tick-stroke.thickness / 2 / 1pt
  let pair-style = resolved-base-pair-stroke.style
  let tick-style = resolved-tick-stroke.style

  let lines = ()
  let arcs = ()
  let boxes = ()
  let box-bodies = ()
  let ticks = ()
  let tick-bodies = ()
  let curves = ()
  // Draw order for backbone and pair primitives, as (kind, index) entries
  // into `lines`, `arcs`, or `curves`.
  let backbone-primitives = ()
  let pair-primitives = ()

  // A palette colors both the letter and its circle. Without a palette, the
  // letter uses the document's text color and the circle stays gray.
  let nucleotide-colors = if palette == none {
    (:)
  } else {
    let prepared = _prepare-palette(palette)
    // RNA drawing treats every character, gaps included, as a visible
    // nucleotide, so every character needs a color.
    _assert-palette-coverage(
      prepared,
      (sequence,),
      observed: sequence.clusters().map(c => (upper(c), true)).to-dict(),
    )
    _derive-residue-colors(prepared, darken-body: show-nucleotide-circles)
  }

  // Cache nucleotide labels and measurements before building the geometry.
  let glyph-cache = (:)
  let max-glyph-extent = 0pt
  for glyph in nucleotides {
    if glyph not in glyph-cache {
      let colors = _lookup-palette-entry(nucleotide-colors, glyph)
      let body = if colors == none {
        text(glyph)
      } else {
        text(fill: colors.body-fill, glyph)
      }
      let size = measure(body)
      glyph-cache.insert(
        glyph,
        (
          body: body,
          size: size,
          circle-fill: if colors == none { _light-gray } else {
            colors.cell-fill
          },
        ),
      )
      max-glyph-extent = calc.max(max-glyph-extent, size.width, size.height)
    }
  }
  let circle-diameter = max-glyph-extent + 2 * circle-padding
  let circle-bodies = (:)
  if show-nucleotide-circles {
    for (glyph, glyph-data) in glyph-cache.pairs() {
      circle-bodies.insert(glyph, box(
        width: circle-diameter,
        height: circle-diameter,
        {
          place(top + left, circle(
            radius: circle-diameter / 2,
            fill: glyph-data.circle-fill,
          ))
          align(center + horizon, glyph-data.body)
        },
      ))
    }
  }
  let nucleotide-radius = if show-nucleotide-circles {
    circle-diameter / 2 / 1pt
  } else {
    bare-nucleotide-radius
  }
  // Connector endpoints sit on the geometry point and are pushed along `unit`
  // by the nucleotide radius, so they stop at the nucleotide edge.
  let inset-anchor(geometry, unit, sign) = (
    geometry: geometry,
    page_pt: (
      x: sign * unit.x * nucleotide-radius,
      y: sign * unit.y * nucleotide-radius,
    ),
  )
  let make-line(first, second, half-stroke, min-scale, minimum-visible) = {
    let dx = second.x - first.x
    let dy = second.y - first.y
    let distance = calc.sqrt(dx * dx + dy * dy)
    if distance <= 0 { return none }
    let unit = (x: dx / distance, y: dy / distance)
    (
      start: inset-anchor(first, unit, 1),
      end: inset-anchor(second, unit, -1),
      half_stroke_pt: half-stroke,
      min_scale: min-scale,
      minimum_visible_length_pt: minimum-visible,
    )
  }
  // Backbone connectors always keep the minimum visible length. Pair and
  // nominal connectors keep it only between circles; bare letters show a pair
  // as soon as the letters stop overlapping.
  let pair-minimum-visible-length = if show-nucleotide-circles {
    minimum-connector-length-pt
  } else {
    0.0
  }
  // After fitting resolves the geometry scale, auto ticks and terminal leaders
  // use this nominal connector instead of inheriting a local edge.
  let archetypical-connector = (
    span: layout-data.nominal_spacing,
    endpoint_gap_pt: nucleotide-radius,
    minimum_scale: _connector-min-scale(
      layout-data.nominal_spacing,
      nucleotide-radius,
      pair-minimum-visible-length,
    ),
    minimum_visible_length_pt: none,
  )
  // This scale gives one nominal backbone step the requested pitch when a
  // layout sizes itself, regardless of which layout produced the geometry.
  let target-geometry-scale = (
    (2 * nucleotide-radius + backbone-bond-length-pt)
      / layout-data.nominal_spacing
  )
  let make-tick(
    anchor,
    previous,
    next,
    owner-box-index,
    tick-length-pt,
    resolved-connector,
    label-size,
  ) = (
    anchor: anchor,
    previous: previous,
    next: next,
    owner_box: owner-box-index,
    offset_pt: nucleotide-radius,
    tick_len_pt: tick-length-pt,
    resolved_connector: resolved-connector,
    gap_pt: label-gap,
    half_stroke_pt: tick-half-stroke,
    label_width_pt: label-size.width / 1pt,
    label_height_pt: label-size.height / 1pt,
  )

  // Build backbone segments.
  for from in range(layout-data.backbone.len()) {
    let arc = layout-data.backbone.at(from)
    let to = from + 1
    let first = coordinates.at(from)
    let second = coordinates.at(to)
    if arc == none {
      let line = make-line(
        first,
        second,
        backbone-half-stroke,
        none,
        minimum-connector-length-pt,
      )
      if line != none {
        backbone-primitives.push((kind: "line", index: lines.len()))
        lines.push(line)
      }
    } else {
      let center = arc.center
      let theta1 = calc.atan2(first.x - center.x, first.y - center.y)
      let theta2 = calc.atan2(second.x - center.x, second.y - center.y)
      let start-rad = theta1.rad()
      let end-rad = theta2.rad()
      let difference = calc.rem(end-rad - start-rad, calc.tau)
      if difference < 0 { difference += calc.tau }
      let span-rad = if arc.clockwise { difference - calc.tau } else {
        difference
      }
      backbone-primitives.push((kind: "arc", index: arcs.len()))
      arcs.push((
        center: center,
        radius: arc.radius,
        end_radius: arc.at("end_radius", default: none),
        start_angle_rad: start-rad,
        span_rad: span-rad,
        endpoint_gap_pt: nucleotide-radius,
        half_stroke_pt: backbone-half-stroke,
        minimum_visible_length_pt: minimum-connector-length-pt,
      ))
    }
  }

  // Build base pairs. Circular layouts may provide a cubic bow per pair. The
  // plugin omits `pair_curves` when every pair is a straight chord, and a
  // `none` entry marks one straight pair; both draw as lines.
  let pair-curves = layout-data.at("pair_curves", default: ())
  for pair-index in range(layout-data.base_pairs.len()) {
    let pair = layout-data.base_pairs.at(pair-index)
    let first = coordinates.at(pair.i)
    let second = coordinates.at(pair.j)
    let dx = second.x - first.x
    let dy = second.y - first.y
    let distance = calc.sqrt(dx * dx + dy * dy)
    let pair-min-scale = _connector-min-scale(
      distance,
      nucleotide-radius,
      pair-minimum-visible-length,
    )
    let pair-curve = pair-curves.at(pair-index, default: none)
    if pair-curve != none {
      let control-i = pair-curve.control_i
      let control-j = pair-curve.control_j
      let control-i-dx = control-i.x - first.x
      let control-i-dy = control-i.y - first.y
      let control-j-dx = second.x - control-j.x
      let control-j-dy = second.y - control-j.y
      let start-distance = calc.sqrt(
        control-i-dx * control-i-dx + control-i-dy * control-i-dy,
      )
      let end-distance = calc.sqrt(
        control-j-dx * control-j-dx + control-j-dy * control-j-dy,
      )
      if start-distance > 0 and end-distance > 0 {
        let start-unit = (
          x: control-i-dx / start-distance,
          y: control-i-dy / start-distance,
        )
        let end-unit = (
          x: control-j-dx / end-distance,
          y: control-j-dy / end-distance,
        )
        pair-primitives.push((kind: "curve", index: curves.len()))
        curves.push((
          start: inset-anchor(first, start-unit, 1),
          control_1: control-i,
          control_2: control-j,
          end: inset-anchor(second, end-unit, -1),
          half_stroke_pt: pair-half-stroke,
          min_scale: pair-min-scale,
        ))
        continue
      }
    }
    let line = make-line(first, second, pair-half-stroke, pair-min-scale, none)
    if line != none {
      pair-primitives.push((kind: "line", index: lines.len()))
      lines.push(line)
    }
  }

  // Build nucleotide labels and optional circle backgrounds.
  for index in range(nucleotide-count) {
    let coordinate = coordinates.at(index)
    let glyph = nucleotides.at(index)
    let glyph-data = glyph-cache.at(glyph)
    let size = if show-nucleotide-circles {
      (width: circle-diameter, height: circle-diameter)
    } else {
      glyph-data.size
    }
    let body = if show-nucleotide-circles {
      circle-bodies.at(glyph)
    } else {
      glyph-data.body
    }
    boxes.push((
      anchor: coordinate,
      width_pt: size.width / 1pt,
      height_pt: size.height / 1pt,
    ))
    box-bodies.push(body)
  }

  // Measure position ticks here. The fit stage chooses their direction after
  // it knows the final scale and can account for crowding.
  if position-interval != none {
    for index in range(nucleotide-count) {
      let position = index + 1
      let is-internal-tick = (
        index > 0
          and (index < nucleotide-count - 1 or not show-terminal-labels)
          and calc.rem(position, position-interval) == 0
      )
      let is-leading-tick = index == 0 and not show-terminal-labels
      if is-internal-tick or is-leading-tick {
        let body = text(size: 0.85em, fill: _light-medium-gray, str(position))
        let size = measure(body)
        ticks.push(make-tick(
          coordinates.at(index),
          if index > 0 { coordinates.at(index - 1) } else { none },
          if index < nucleotide-count - 1 {
            coordinates.at(index + 1)
          } else {
            none
          },
          index,
          tick-length,
          if position-tick-length == auto { archetypical-connector } else {
            none
          },
          size,
        ))
        tick-bodies.push(body)
      }
    }
  }

  // Terminal labels use the same collision-aware placement as position ticks.
  // They normally continue the backbone away from the molecule, but the
  // placement search can turn them toward open space around a loop.
  if show-terminal-labels and nucleotide-count > 0 {
    let terminal-data = (
      (index: 0, label: "5'", color: five-prime-label-color),
      (
        index: nucleotide-count - 1,
        label: "3'",
        color: three-prime-label-color,
      ),
    )
    for terminal in terminal-data {
      let coordinate = coordinates.at(terminal.index)
      let body = text(
        fill: terminal.color,
        terminal.label,
      )
      let size = measure(body)
      let previous = if terminal.index > 0 {
        coordinates.at(terminal.index - 1)
      } else if nucleotide-count == 1 and terminal.label == "5'" {
        (x: coordinate.x, y: coordinate.y + 1.0)
      } else {
        none
      }
      let next = if terminal.index < nucleotide-count - 1 {
        coordinates.at(terminal.index + 1)
      } else if nucleotide-count == 1 and terminal.label == "3'" {
        (x: coordinate.x, y: coordinate.y - 1.0)
      } else {
        none
      }
      // Terminal leaders use the tick style and the fitted length of the
      // nominal backbone connector.
      ticks.push(make-tick(
        coordinate,
        previous,
        next,
        terminal.index,
        0.0,
        archetypical-connector,
        size,
      ))
      tick-bodies.push(body)
    }
  }

  (
    lines: lines,
    arcs: arcs,
    boxes: boxes,
    box-bodies: box-bodies,
    ticks: ticks,
    tick-bodies: tick-bodies,
    backbone-stroke: resolved-backbone-stroke.style,
    pair-stroke: pair-style,
    tick-stroke: tick-style,
    backbone-primitives: backbone-primitives,
    pair-primitives: pair-primitives,
    curves: curves,
    target-geometry-scale: target-geometry-scale,
  )
}

/// Fits a prepared RNA plan into a layout viewport.
///
/// - plan (dictionary): Measured RNA drawing plan.
/// - width (length, auto, ratio, relative): Requested render width.
/// - height (length, auto): Resolved render height.
/// - layout-size (dictionary): Size supplied by Typst's layout callback.
/// -> dictionary
#let _fit-rna-plan(plan, width, height, layout-size) = {
  let width-mode = _width-mode(width, layout-size.width)
  let height-mode = if height == auto { "auto" } else { "resolved" }
  let payload = (
    width_mode: width-mode,
    viewport_width_pt: if width-mode == "resolved" {
      _resolve-length(layout-size.width) / 1pt
    } else {
      none
    },
    height_mode: height-mode,
    viewport_height_pt: if height-mode == "resolved" {
      height / 1pt
    } else {
      none
    },
    lines: plan.lines,
    arcs: plan.arcs,
    curves: plan.curves,
    boxes: plan.boxes,
    ticks: plan.ticks,
    target_geometry_scale: plan.target-geometry-scale,
  )
  json(_rna-structure-plugin.fit(bytes(json.encode(payload, pretty: false))))
}

/// Renders fitted RNA primitives in their established layer order.
///
/// - plan (dictionary): Measured RNA drawing plan.
/// - fitted-plan (dictionary): Fitted geometry from the RNA plugin.
/// -> content
#let _render-rna-plan(plan, fitted-plan) = box(
  width: fitted-plan.viewport_width_pt * 1pt,
  height: fitted-plan.viewport_height_pt * 1pt,
  {
    for entry in plan.backbone-primitives {
      let line-style = plan.backbone-stroke
      if entry.kind == "line" {
        _place-rna-line(fitted-plan.lines.at(entry.index), line-style)
      } else {
        let materialized = fitted-plan.arcs.at(entry.index)
        if materialized != none and line-style != none {
          let points = materialized.points_pt
          let first = points.first()
          let components = (curve.move(_page-point(first)),)
          for chunk in points.slice(1).chunks(3) {
            let first = chunk.at(0)
            let second = chunk.at(1)
            let third = chunk.at(2)
            components.push(curve.cubic(
              _page-point(first),
              _page-point(second),
              _page-point(third),
              relative: false,
            ))
          }
          place(top + left, curve(stroke: line-style, ..components))
        }
      }
    }
    for entry in plan.pair-primitives {
      let line-style = plan.pair-stroke
      if entry.kind == "line" {
        _place-rna-line(fitted-plan.lines.at(entry.index), line-style)
      } else {
        let materialized = fitted-plan.curves.at(entry.index)
        if materialized != none and line-style != none {
          let components = (
            curve.move(_page-point(materialized.start_pt)),
            curve.cubic(
              _page-point(materialized.control_1_pt),
              _page-point(materialized.control_2_pt),
              _page-point(materialized.end_pt),
              relative: false,
            ),
          )
          place(top + left, curve(stroke: line-style, ..components))
        }
      }
    }
    // Draw tick lines under the nucleotides so they tuck beneath circle edges.
    // Draw labels over the lines so numbers remain readable in tight spaces.
    for tick in fitted-plan.ticks {
      _place-rna-line(tick.line, plan.tick-stroke)
    }
    for index in range(plan.box-bodies.len()) {
      let body = plan.box-bodies.at(index)
      let box-data = fitted-plan.boxes.at(index)
      place(
        top + left,
        dx: box-data.top_left_pt.x * 1pt,
        dy: box-data.top_left_pt.y * 1pt,
        body,
      )
    }
    for index in range(fitted-plan.ticks.len()) {
      let tick = fitted-plan.ticks.at(index)
      place(
        top + left,
        dx: tick.top_left_pt.x * 1pt,
        dy: tick.top_left_pt.y * 1pt,
        plan.tick-bodies.at(index),
      )
    }
  },
)

/// Predicts an RNA secondary structure from a sequence.
///
/// Returns the predicted dot-bracket structure and associated prediction
/// score metadata.
///
/// - sequence (str): RNA or DNA sequence to predict structure for, using IUPAC
///   nucleotide codes. DNA `T` bases are normalized to RNA `U`, and ambiguity
///   codes are treated as unpairable.
/// - model (str): Folding energy model: "contrafold" (statistical parameters,
///   log-linear score) or "viennarnafold" (Turner thermodynamic parameters,
///   free energy in kcal/mol) (default: "contrafold").
/// - beam-size (int, none): Beam size for pruning during search. Set to `none`
///   to disable pruning (default: 100).
/// - allow-sharp-turns (bool): Whether to allow hairpin loops with fewer than
///   3 nucleotides (default: false).
/// - dangles (str): Treatment of dangling end energies for bases adjacent to
///   helices: "both" (include dangling energies on both sides) or "none"
///   (ignore dangling ends). This affects only the ViennaRNA model
///   (default: "both").
/// - topology (str): Molecule topology: "linear" or "circular"
///   (default: "linear"). Circular folding requires `model: "viennarnafold"`.
/// - constraints (str, none): Hard structure constraints matching the sequence
///   length, using `.` for unpaired, `(` and `)` for paired, and `?` for
///   unconstrained bases. Paired constraints must be balanced and canonical for
///   the supplied sequence. With `model: "viennarnafold"`, each forced pair
///   must enclose at least 3 bases (default: none).
/// -> dictionary with keys:
///   - structure (str): Predicted secondary structure in dot-bracket notation.
///   - score (dictionary): Prediction scoring metadata with:
///     - model (str): Name of the scoring model used.
///     - value (float): Prediction score or free energy value.
#let predict-rna-structure(
  sequence,
  model: "contrafold",
  beam-size: 100,
  allow-sharp-turns: false,
  dangles: "both",
  topology: "linear",
  constraints: none,
) = {
  assert(type(sequence) == str, message: "sequence must be a string.")
  assert(sequence.len() > 0, message: "sequence cannot be empty.")
  assert(
    model in ("contrafold", "viennarnafold"),
    message: "model must be 'contrafold' or 'viennarnafold'.",
  )
  assert(
    beam-size == none or (type(beam-size) == int and beam-size >= 1),
    message: "beam-size must be none or an integer >= 1.",
  )
  assert(
    type(allow-sharp-turns) == bool,
    message: "allow-sharp-turns must be a boolean.",
  )
  assert(
    dangles in ("none", "both"),
    message: "dangles must be 'none' or 'both'.",
  )
  assert(
    topology in ("linear", "circular"),
    message: "topology must be 'linear' or 'circular'.",
  )
  assert(
    constraints == none or type(constraints) == str,
    message: "constraints must be none or a string.",
  )
  if constraints != none {
    assert(
      constraints.len() == sequence.len(),
      message: "constraints length must match sequence length.",
    )
  }

  let canonical-model = if model == "viennarnafold" { "ViennaRnafold" } else {
    "ContraFold"
  }
  let canonical-dangles = if dangles == "none" { "None" } else { "Both" }
  let canonical-topology = if topology == "circular" { "Circular" } else {
    "Linear"
  }

  let config = bytes(json.encode(
    (
      model: canonical-model,
      beam_size: if beam-size == none { 0 } else { beam-size },
      allow_sharp_turns: allow-sharp-turns,
      dangles: canonical-dangles,
      topology: canonical-topology,
      constraints: constraints,
    ),
    pretty: false,
  ))
  json(_rna-structure-plugin.predict(bytes(sequence), config))
}

/// Renders an RNA secondary structure visualization.
///
/// Coordinates scale uniformly to fit the allocated area while text, strokes,
/// ticks, and padding preserve their sizes. Circular layouts use a fixed
/// circle with curved chords connecting base pairs.
///
/// - sequence (str): Nucleotide sequence string using IUPAC nucleotide codes.
/// - structure (str): Secondary structure in dot-bracket notation matching `sequence`.
/// - width (length, auto, ratio, relative): Width of the visualization
///   (default: 100%).
/// - height (length, auto): Height of the visualization (default: auto).
/// - layout (str): Layout algorithm: "radial", "naview", "rna_turtle",
///   "rna_puzzler", or "circular" (default: "radial").
/// - backbone-bond-length (length): Target backbone bond length between
///   consecutive nucleotides before scaling (default: 0.7em).
/// - minimum-connector-length (length): Shortest visible length before a
///   backbone connector is hidden (default: 1.2pt).
/// - backbone-stroke (stroke, none): Stroke for sequence backbone lines. `none`
///   hides them (default: 1pt black solid).
/// - base-pair-stroke (stroke, none): Stroke for base-pair connector lines.
///   `none` hides them (default: 1pt gray densely dotted).
/// - show-nucleotide-circles (bool): Whether to draw background circles behind
///   nucleotides (default: false).
/// - palette (dictionary, none): Color map from nucleotide characters to base
///   colors, such as an entry from `residue-palette.rna` (default: none).
/// - nucleotide-padding (length): Clearance between bare nucleotide letters
///   and connector lines (default: 1.1pt).
/// - circle-padding (length): Clearance between nucleotide letters and
///   surrounding background circles (default: 3.0pt).
/// - position-interval (int, none): Interval between numbered position ticks.
///   `none` disables ticks (default: 50).
/// - position-tick-length (length, auto): Length of position tick marks. `auto`
///   uses the fitted connector length (default: auto).
/// - position-label-gap (length): Clearance between tick mark ends and label
///   numbers (default: 1.2pt).
/// - tick-stroke (stroke, none): Stroke for position ticks and terminal-label
///   leaders. `none` hides them (default: 1pt gray solid).
/// - show-terminal-labels (bool): Whether to draw 5' and 3' terminal labels
///   (default: false).
/// - five-prime-label-color (color): Text color for 5' terminal label
///   (default: blue).
/// - three-prime-label-color (color): Text color for 3' terminal label
///   (default: red).
/// -> content
#let render-rna-structure(
  sequence,
  structure,
  width: 100%,
  height: auto,
  layout: "radial",
  backbone-bond-length: 0.7em,
  minimum-connector-length: 1.2pt,
  backbone-stroke: _default-rna-backbone-stroke,
  base-pair-stroke: _default-rna-base-pair-stroke,
  show-nucleotide-circles: false,
  palette: none,
  nucleotide-padding: 1.1pt,
  circle-padding: 3.0pt,
  position-interval: 50,
  position-tick-length: auto,
  position-label-gap: 1.2pt,
  tick-stroke: _default-rna-tick-stroke,
  show-terminal-labels: false,
  five-prime-label-color: _blue,
  three-prime-label-color: _red,
) = {
  assert(type(sequence) == str, message: "sequence must be a string.")
  assert(sequence.len() > 0, message: "sequence cannot be empty.")
  assert(type(structure) == str, message: "structure must be a string.")
  assert(
    structure.len() == sequence.len(),
    message: "structure length must match sequence length.",
  )
  assert(
    layout in _accepted-layouts,
    message: "layout must be "
      + _accepted-layouts
        .map(name => "'" + name + "'")
        .join(", ", last: ", or ")
      + ".",
  )
  assert(
    position-interval == none
      or (type(position-interval) == int and position-interval > 0),
    message: "position-interval must be none or an integer >= 1.",
  )
  assert(
    type(show-terminal-labels) == bool,
    message: "show-terminal-labels must be a boolean.",
  )
  assert(
    type(show-nucleotide-circles) == bool,
    message: "show-nucleotide-circles must be a boolean.",
  )
  if show-terminal-labels {
    assert(
      type(five-prime-label-color) == color,
      message: "five-prime-label-color must be a color.",
    )
    assert(
      type(three-prime-label-color) == color,
      message: "three-prime-label-color must be a color.",
    )
  }
  assert(
    palette == none or type(palette) == dictionary,
    message: "palette must be a dictionary that maps nucleotides to colors.",
  )

  // Lengths may mix em and absolute parts, so they are validated in context.
  context {
    assert(
      _render-width-is-valid(width),
      message: "width must be auto or a positive length, ratio, or relative width.",
    )
    let height = if height == auto { auto } else {
      _assert-length(
        height,
        "height must be auto or a positive length.",
        positive: true,
      )
    }
    let backbone-bond-length = _assert-length(
      backbone-bond-length,
      "backbone-bond-length must be positive.",
      positive: true,
    )
    let minimum-connector-length = _assert-length(
      minimum-connector-length,
      "minimum-connector-length must be positive.",
      positive: true,
    )
    let nucleotide-padding = _assert-length(
      nucleotide-padding,
      "nucleotide-padding must be non-negative.",
    )
    let circle-padding = _assert-length(
      circle-padding,
      "circle-padding must be non-negative.",
    )
    let position-tick-length = if position-tick-length == auto { auto } else {
      _assert-length(
        position-tick-length,
        "position-tick-length must be auto or a non-negative length.",
      )
    }
    let position-label-gap = _assert-length(
      position-label-gap,
      "position-label-gap must be non-negative.",
    )

    block(width: width, {
      let layout-data = _layout-data(sequence, structure, layout)
      let plan = _prepare-rna-plan(sequence, layout-data, (
        backbone-bond-length: backbone-bond-length,
        minimum-connector-length: minimum-connector-length,
        backbone-stroke: backbone-stroke,
        base-pair-stroke: base-pair-stroke,
        show-nucleotide-circles: show-nucleotide-circles,
        palette: palette,
        nucleotide-padding: nucleotide-padding,
        circle-padding: circle-padding,
        position-interval: position-interval,
        position-tick-length: position-tick-length,
        position-label-gap: position-label-gap,
        tick-stroke: tick-stroke,
        show-terminal-labels: show-terminal-labels,
        five-prime-label-color: five-prime-label-color,
        three-prime-label-color: three-prime-label-color,
      ))
      // Auto width does not use the available layout size.
      if width == auto {
        let fitted-plan = _fit-rna-plan(
          plan,
          width,
          height,
          (width: 0pt, height: 0pt),
        )
        _render-rna-plan(plan, fitted-plan)
      } else {
        _rna-render-layout(size => {
          let fitted-plan = _fit-rna-plan(plan, width, height, size)
          _render-rna-plan(plan, fitted-plan)
        })
      }
    })
  }
}
