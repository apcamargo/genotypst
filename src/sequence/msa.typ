#import "../common/colors.typ": _light-gray
#import "../common/fixed_grid.typ": _fixed-width-grid, _measure-monospace-width
#import "../common/interval.typ": _resolve-1indexed-window
#import "./sequence_alphabet.typ": _resolve-alphabet-config
#import "./sequence_processing.typ": (
  _collect-window-column-stats, _compute-consensus-sequence,
  _lookup-palette-color, _resolve-palette, _validate-alignment,
)

/// Renders a single character in an MSA with optional coloring.
///
/// - char (str): The character to render.
/// - colors (bool): Whether to apply coloring.
/// - palette (dictionary): Color palette for residues.
/// - use-palette (bool): Whether to use residue palette colors.
/// -> dictionary with keys:
///   - body (content): Rendered character content.
///   - fill (color, none): Optional background fill color.
#let _render-msa-character(char, colors, palette, use-palette: true) = {
  let base-color = if colors and use-palette {
    _lookup-palette-color(palette, char)
  } else {
    none
  }
  if base-color != none {
    let bg-color = base-color.lighten(73.5%)
    let fg-color = base-color.darken(22.5%)
    (body: text(fill: fg-color, char), fill: bg-color)
  } else {
    let content = if colors { text(fill: _light-gray, char) } else { char }
    (body: content, fill: none)
  }
}

/// Renders a conservation row for an MSA block.
///
/// Creates a horizontal row of bars where each bar represents information
/// content (conservation) of a single column in alignment.
///
/// - column-stats (array): Prepared per-column statistics for the current block.
/// - max-bits (float): Maximum possible information content (log2 of alphabet size).
/// - cell-width (length): Width of each character cell.
/// -> content
#let _render-msa-conservation-row(column-stats, max-bits, cell-width) = {
  let bar-height = 1.5em
  let bars = ()

  for stats in column-stats {
    let h = (stats.conservation / max-bits) * bar-height
    bars.push((
      body: box(
        height: bar-height,
        align(bottom, rect(width: cell-width, height: h, fill: _light-gray)),
      ),
    ))
  }

  if bars.len() == 0 { [] } else {
    _fixed-width-grid((bars,), cell-width: cell-width)
  }
}

/// Renders a single sequence row for an MSA block.
///
/// Creates a row with the sequence identifier and a segment of sequence
/// optionally colored by chemical properties.
///
/// - acc (str): Sequence identifier/accession.
/// - seq (str): The full sequence string.
/// - block-start (int): Starting position of the block (0-indexed).
/// - block-end (int): Ending position of the block (0-indexed, exclusive).
/// - max-acc-width (int): Maximum width for accession display.
/// - colors (bool): Whether to color residues.
/// - palette (dictionary): Color palette for residues.
/// - consensus-chars (array, none): Consensus residue characters for this block.
/// -> array with:
///   - The accession text (content)
///   - The rendered sequence segment (array)
#let _render-msa-sequence-row(
  acc,
  seq,
  block-start,
  block-end,
  max-acc-width,
  colors,
  palette,
  consensus-chars: none,
) = {
  let display-acc = if acc.len() > max-acc-width {
    acc.slice(0, max-acc-width - 1) + "…"
  } else {
    acc
  }

  let segment = if block-start < seq.len() {
    seq.slice(block-start, calc.min(block-end, seq.len()))
  } else {
    ""
  }

  let rendered-seq = segment
    .clusters()
    .enumerate()
    .map(item => {
      let (index, char) = item
      let use-palette = if consensus-chars != none {
        (
          index < consensus-chars.len()
            and upper(char) == upper(consensus-chars.at(index))
        )
      } else {
        true
      }
      _render-msa-character(char, colors, palette, use-palette: use-palette)
    })

  (display-acc, rendered-seq)
}

/// Renders prepared MSA cells as a fixed-width grid row.
///
/// - acc (content, str): Row label.
/// - seq-cells (array): Rendered sequence cells.
/// - cell-width (length): Width of each character cell.
/// - cell-outset-y (length): Vertical cell outset.
/// -> array with:
///   - The row label
///   - The rendered sequence grid
#let _render-msa-row(acc, seq-cells, cell-width, cell-outset-y) = {
  let seq-content = if seq-cells.len() == 0 { [] } else {
    _fixed-width-grid(
      (seq-cells,),
      cell-width: cell-width,
      cell-outset: (y: cell-outset-y),
    )
  }
  (acc, seq-content)
}

/// Renders a multiple sequence alignment.
///
/// Sequences are displayed in blocks of up to `max-seq-width` characters to fit
/// within the document. Can also show residue colors, a consensus sequence, and
/// conservation scores. Empty alignments render nothing and return `none`.
///
/// - alignment (dictionary): Dictionary mapping sequence identifiers to aligned sequences.
/// - max-acc-width (int): Maximum width for accession display (default: 20).
/// - max-seq-width (int): Maximum characters per line in a block (default: 50).
/// - start (int, none): Starting position (1-indexed, inclusive) (default: none).
/// - end (int, none): Ending position (1-indexed, inclusive) (default: none).
/// - colors (bool): Whether to color residues by chemical properties (default: false).
/// - show-consensus-sequence (bool): Whether to show a consensus sequence (default: false).
/// - color-consensus-only (bool): Whether to color only consensus residues (default: false).
/// - show-conservation (bool): Whether to show conservation bars (default: false).
/// - sampling-correction (bool): Whether to apply small sample correction (default: true).
/// - alphabet (auto, str): Sequence alphabet: auto, "aa", "dna", or "rna" (default: auto).
/// - breakable (bool): Whether to allow blocks to break across pages (default: true).
/// - palette (dictionary, auto): Residue color palette to use (default: auto).
/// -> content, none
#let render-msa(
  alignment,
  max-acc-width: 20,
  max-seq-width: 50,
  start: none,
  end: none,
  colors: false,
  show-consensus-sequence: false,
  color-consensus-only: false,
  show-conservation: false,
  sampling-correction: true,
  alphabet: auto,
  breakable: true,
  palette: auto,
) = {
  let pairs = alignment.pairs()
  if pairs.len() == 0 { return }

  _validate-alignment(alignment)
  let sequences = alignment.values()
  let total-max-len = sequences.first().len()

  let config = _resolve-alphabet-config(alphabet, sequences)
  let palette-to-use = _resolve-palette(
    palette,
    config,
    sequences,
    enabled: colors,
  )

  let window = _resolve-1indexed-window(
    start,
    end,
    total-max-len,
    window-name: "MSA",
  )
  let actual-start = window.actual-start
  let actual-end = window.actual-end

  let max-bits = config.max-bits
  let consensus-coloring-enabled = colors and color-consensus-only
  let needs-consensus = show-consensus-sequence or consensus-coloring-enabled
  let needs-column-stats = show-conservation or needs-consensus
  let column-stats = if needs-column-stats {
    _collect-window-column-stats(
      sequences,
      actual-start,
      actual-end,
      config,
      sampling-correction,
      compute-conservation: show-conservation,
    )
  } else {
    ()
  }
  let consensus-sequence = if needs-consensus {
    _compute-consensus-sequence(column-stats)
  } else {
    ""
  }

  context {
    let leading = par.leading
    let char-width = _measure-monospace-width()
    let outset-y = leading / 2
    let box-width = char-width + 0.03em

    let blocks = range(actual-start, actual-end, step: max-seq-width).map(
      block-start => {
        let block-end = calc.min(block-start + max-seq-width, actual-end)
        let relative-start = block-start - actual-start
        let relative-end = block-end - actual-start
        let consensus-chars = if consensus-coloring-enabled {
          consensus-sequence.slice(relative-start, relative-end).clusters()
        } else {
          none
        }

        let conservation-row = if show-conservation {
          let block-stats = column-stats.slice(
            relative-start,
            relative-end,
          )
          let bars = _render-msa-conservation-row(
            block-stats,
            max-bits,
            box-width,
          )
          ([], bars)
        } else {
          ()
        }

        let consensus-row = if show-consensus-sequence {
          let row = _render-msa-sequence-row(
            "Consensus",
            consensus-sequence,
            relative-start,
            relative-end,
            "Consensus".len(),
            colors,
            palette-to-use,
          )
          _render-msa-row(row.at(0), row.at(1), box-width, outset-y)
        } else {
          ()
        }

        let sequence-rows = pairs
          .map(p => {
            let (acc, seq) = p
            let row = _render-msa-sequence-row(
              acc,
              seq,
              block-start,
              block-end,
              max-acc-width,
              colors,
              palette-to-use,
              consensus-chars: consensus-chars,
            )
            _render-msa-row(row.at(0), row.at(1), box-width, outset-y)
          })
          .flatten()

        block(
          breakable: breakable,
          grid(
            columns: (auto, auto),
            column-gutter: 7pt,
            row-gutter: leading,
            align: left,
            ..conservation-row,
            ..consensus-row,
            ..sequence-rows,
          ),
        )
      },
    )

    block(
      inset: (y: outset-y),
      stack(spacing: 2em, ..blocks),
    )
  }
}
