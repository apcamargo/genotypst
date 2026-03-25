#import "../common/fixed_grid.typ": _fixed-width-grid
#import "./alignment_backend.typ": _alignment-backend, resolve-matrix-name

/// Private: Validates and cleans a sequence string.
///
/// Removes all whitespace characters (spaces, tabs, newlines) and converts to uppercase.
/// This allows users to input sequences with whitespace for readability.
///
/// - seq (str): The sequence to validate.
/// - name (str): Name for error messages (e.g., "seq-1").
/// -> str
#let _validate-sequence(seq, name) = {
  assert(type(seq) == str, message: name + " must be a string.")
  let cleaned = upper(seq.replace(regex("\\s"), ""))
  assert(cleaned.len() > 0, message: name + " must not be empty.")
  cleaned
}

/// Private: Validates scoring parameters and returns canonical matrix name if applicable.
///
/// - matrix (str, none): Scoring matrix name.
/// - match-score (int, none): Match score.
/// - mismatch-score (int, none): Mismatch score.
/// -> str, none (canonical matrix name if using matrix)
#let _validate-scoring-params(matrix, match-score, mismatch-score) = {
  // Mutual exclusivity
  assert(
    not (matrix != none and (match-score != none or mismatch-score != none)),
    message: "Cannot use both 'matrix' and 'match-score'/'mismatch-score' - they are mutually exclusive.",
  )

  // At least one scoring method
  assert(
    matrix != none or (match-score != none and mismatch-score != none),
    message: "Provide either 'matrix' or both 'match-score' and 'mismatch-score'.",
  )

  // Matrix name resolution (case-insensitive)
  if matrix != none {
    let canonical = resolve-matrix-name(matrix)
    assert(
      canonical != none,
      message: "Unknown scoring matrix: '" + matrix + "'.",
    )
    canonical
  }
}

/// Private: Builds the JSON configuration dictionary for WASM.
///
/// - canonical-matrix (str, none): Canonical matrix name.
/// - match-score (int, none): Match score.
/// - mismatch-score (int, none): Mismatch score.
/// - gap-penalty (int): Gap penalty.
/// - mode (str): Alignment mode.
/// -> dictionary
#let _build-config(
  canonical-matrix,
  match-score,
  mismatch-score,
  gap-penalty,
  mode,
) = {
  let config = (
    gap_open: gap-penalty,
    gap_extend: gap-penalty,
    mode: mode,
  )

  if canonical-matrix != none {
    config.insert("matrix", canonical-matrix)
  } else {
    config.insert("match_score", match-score)
    config.insert("mismatch_score", mismatch-score)
  }

  config
}

/// Private: Calls the alignment WASM plugin and parses the response.
///
/// - seq-1 (str): First sequence.
/// - seq-2 (str): Second sequence.
/// - config (dictionary): Configuration dictionary.
/// -> dictionary
#let _call-align-wasm(seq-1, seq-2, config) = {
  let config-json = json.encode(config)
  let result = _alignment-backend.align(
    bytes(seq-1),
    bytes(seq-2),
    bytes(config-json),
  )
  json(result)
}


/// Private: Transforms the WASM response to the final output format.
///
/// - wasm-result (dictionary): Raw result from WASM plugin.
/// - original-seq-1 (str): Original (cleaned) first sequence.
/// - original-seq-2 (str): Original (cleaned) second sequence.
/// - mode (str): Alignment mode.
/// - canonical-matrix (str, none): Canonical matrix name.
/// - match-score (int, none): Match score.
/// - mismatch-score (int, none): Mismatch score.
/// - gap-penalty (int): Gap penalty.
/// -> dictionary
#let _transform-result(
  wasm-result,
  original-seq-1,
  original-seq-2,
  mode,
  canonical-matrix,
  match-score,
  mismatch-score,
  gap-penalty,
) = {
  let dp = wasm-result.dp_matrix
  let rows = dp.rows
  let cols = dp.cols

  // Convert traceback paths
  let traceback-paths = wasm-result.traceback_paths.map(path => path.map(
    coord => (coord.at(0), coord.at(1)),
  ))

  // Determine if there's a valid alignment
  let has-alignment = wasm-result.alignments.len() > 0

  (
    seq-1: original-seq-1,
    seq-2: original-seq-2,
    score: wasm-result.alignment_score,
    mode: mode,
    scoring: (
      matrix: canonical-matrix,
      match-score: match-score,
      mismatch-score: mismatch-score,
      gap-penalty: gap-penalty,
    ),
    alignments: wasm-result.alignments,
    traceback-paths: traceback-paths,
    dp-matrix: (
      rows: rows,
      cols: cols,
      cell-values: dp.cell_values,
      arrows: dp.arrows,
    ),
    has-alignment: has-alignment,
  )
}

/// Performs pairwise sequence alignment using dynamic programming.
///
/// Aligns two sequences using either a scoring matrix (e.g., BLOSUM62) or
/// custom match/mismatch scores. Returns alignment results including the
/// DP matrix, traceback paths, and aligned sequences.
///
/// Sequences are automatically cleaned: whitespace is removed and characters
/// are converted to uppercase. This allows input like "ACG TGC\nAAA".
///
/// Available scoring matrices: BLOSUM30, BLOSUM40, BLOSUM45, BLOSUM50,
/// BLOSUM62, BLOSUM70, BLOSUM80, BLOSUM90, BLOSUM100, PAM1, PAM10, PAM40,
/// PAM80, PAM120, PAM160, PAM250, EDNAFULL. Matrix names are case-insensitive.
///
/// - seq-1 (str): First sequence to align.
/// - seq-2 (str): Second sequence to align.
/// - matrix (str, none): Scoring matrix name (e.g., "BLOSUM62"). Mutually exclusive with match/mismatch scores (default: none).
/// - match-score (int, none): Score for matching characters. Required if matrix is none (default: none).
/// - mismatch-score (int, none): Score for mismatching characters. Required if matrix is none (default: none).
/// - gap-penalty (int): Gap penalty.
/// - mode (str): Alignment mode: "global" or "local" (default: "global").
/// -> dictionary with keys:
///   - seq-1 (str): Cleaned first input sequence.
///   - seq-2 (str): Cleaned second input sequence.
///   - score (int): Alignment score.
///   - mode (str): Alignment mode.
///   - scoring (dictionary): Scoring settings used for the alignment.
///   - alignments (array): Aligned sequence result(s).
///   - traceback-paths (array): Traceback path coordinates.
///   - dp-matrix (dictionary): DP matrix data with dimensions, scores, and arrows.
///   - has-alignment (bool): Whether at least one alignment was found.
#let align-seq-pair(
  seq-1,
  seq-2,
  matrix: none,
  match-score: none,
  mismatch-score: none,
  gap-penalty: none,
  mode: "global",
) = {
  // Validate and clean inputs
  let cleaned-seq-1 = _validate-sequence(seq-1, "seq-1")
  let cleaned-seq-2 = _validate-sequence(seq-2, "seq-2")
  let canonical-matrix = _validate-scoring-params(
    matrix,
    match-score,
    mismatch-score,
  )
  assert(gap-penalty != none, message: "gap-penalty is required.")
  assert(type(gap-penalty) == int, message: "gap-penalty must be an integer.")
  assert(
    mode in ("global", "local"),
    message: "mode must be 'global' or 'local'.",
  )

  // Build config and call WASM
  let config = _build-config(
    canonical-matrix,
    match-score,
    mismatch-score,
    gap-penalty,
    mode,
  )
  let wasm-result = _call-align-wasm(cleaned-seq-1, cleaned-seq-2, config)

  // Transform and return result
  _transform-result(
    wasm-result,
    cleaned-seq-1,
    cleaned-seq-2,
    mode,
    canonical-matrix,
    match-score,
    mismatch-score,
    gap-penalty,
  )
}

/// Private: Parse coordinates from array format.
#let _parse-coord(coord) = {
  (row: coord.at(0), col: coord.at(1))
}

/// Parse and validate coordinate format, type, and bounds.
#let _parse-and-validate-coord(
  coord,
  max-row,
  max-col,
  coord-context,
  allow-extra-array-items: false,
) = {
  assert(
    type(coord) == array,
    message: coord-context + " must be a coordinate array.",
  )

  if allow-extra-array-items {
    assert(
      coord.len() >= 2,
      message: coord-context + " array must contain at least row and col.",
    )
  } else {
    assert(
      coord.len() == 2,
      message: coord-context + " array must contain exactly row and col.",
    )
  }

  let parsed = _parse-coord(coord)
  assert(
    type(parsed.row) == int,
    message: coord-context + " row must be an integer.",
  )
  assert(
    type(parsed.col) == int,
    message: coord-context + " col must be an integer.",
  )
  assert(
    parsed.row >= 0 and parsed.row <= max-row,
    message: coord-context
      + " row "
      + str(parsed.row)
      + " out of bounds [0, "
      + str(max-row)
      + "].",
  )
  assert(
    parsed.col >= 0 and parsed.col <= max-col,
    message: coord-context
      + " col "
      + str(parsed.col)
      + " out of bounds [0, "
      + str(max-col)
      + "].",
  )

  parsed
}

/// Private: Validate that the path is valid for the given grid bounds.
///
/// Checks that coordinates are within bounds and that the path is monotonic
/// (only moves down, right, or diagonally down-right with unit steps).
///
/// - path (array): Path coordinates as `(row, col)` arrays.
/// - max-row (int): Maximum allowed row index.
/// - max-col (int): Maximum allowed column index.
/// -> none
#let _validate-path(path, max-row, max-col) = {
  assert(type(path) == array, message: "path must be an array.")
  assert(path.len() >= 1, message: "Path must contain at least one coordinate.")

  let prev-coord = none
  for (idx, coord) in path.enumerate() {
    let parsed = _parse-and-validate-coord(
      coord,
      max-row,
      max-col,
      "Path coordinate at index " + str(idx),
    )

    // Validate monotonicity (path can only move down, right, or diagonal down-right)
    if prev-coord != none {
      let row-delta = parsed.row - prev-coord.row
      let col-delta = parsed.col - prev-coord.col

      assert(
        row-delta >= 0 and col-delta >= 0,
        message: "Path must be monotonic: step from ("
          + str(prev-coord.row)
          + ", "
          + str(prev-coord.col)
          + ") to ("
          + str(parsed.row)
          + ", "
          + str(parsed.col)
          + ") is invalid. Renderer inputs expect traceback paths in end-to-start order (as returned by align-seq-pair).",
      )
      assert(
        row-delta <= 1 and col-delta <= 1,
        message: "Path steps must be unit steps: step from ("
          + str(prev-coord.row)
          + ", "
          + str(prev-coord.col)
          + ") to ("
          + str(parsed.row)
          + ", "
          + str(parsed.col)
          + ") is too large.",
      )
      assert(
        row-delta + col-delta > 0,
        message: "Path cannot have duplicate consecutive coordinates at ("
          + str(parsed.row)
          + ", "
          + str(parsed.col)
          + ").",
      )
    }

    prev-coord = parsed
  }
}

/// Private: Convert path coordinates to alignment operations.
///
/// - path (array): Path coordinates as `(row, col)` arrays.
/// -> array
#let _path-to-operations(path) = {
  let operations = ()

  for i in range(1, path.len()) {
    let prev = _parse-coord(path.at(i - 1))
    let curr = _parse-coord(path.at(i))

    let row-delta = curr.row - prev.row
    let col-delta = curr.col - prev.col

    if row-delta == 1 and col-delta == 1 {
      // Diagonal: match or mismatch
      operations.push("match-or-mismatch")
    } else if row-delta == 1 and col-delta == 0 {
      // Down: gap in seq2 (seq1 advances, seq2 doesn't)
      operations.push("gap-in-seq2")
    } else if row-delta == 0 and col-delta == 1 {
      // Right: gap in seq1 (seq2 advances, seq1 doesn't)
      operations.push("gap-in-seq1")
    }
  }

  operations
}

/// Private: Build the three alignment strings from sequences and operations.
///
/// - seq-1 (str): First sequence.
/// - seq-2 (str): Second sequence.
/// - path (array): Path coordinates.
/// - operations (array): Array of operation strings.
/// - gap-char (str): Character for gaps.
/// - match-char (str): Character for matches.
/// - mismatch-char (str): Character for mismatches.
/// - hide-unaligned (bool): Whether to hide unaligned characters entirely.
/// -> dictionary
#let _build-alignment-strings(
  seq-1,
  seq-2,
  path,
  operations,
  gap-char,
  match-char,
  mismatch-char,
  hide-unaligned,
) = {
  let seq-1-chars = seq-1.clusters()
  let seq-2-chars = seq-2.clusters()

  let first-coord = _parse-coord(path.at(0))

  // Initialize result strings and unaligned mask
  let aligned1 = ()
  let match-line = ()
  let aligned2 = ()
  let unaligned-mask = ()

  // Handle leading unaligned region (local alignment starting after position 0)
  // Show unaligned characters if hide-unaligned is false
  if not hide-unaligned {
    // Add seq-1 unaligned chars (rows before path starts)
    for i in range(first-coord.row) {
      aligned1.push(seq-1-chars.at(i))
      match-line.push(" ")
      aligned2.push(" ")
      unaligned-mask.push(true)
    }

    // Add seq-2 unaligned chars (cols before path starts)
    for j in range(first-coord.col) {
      aligned1.push(" ")
      match-line.push(" ")
      aligned2.push(seq-2-chars.at(j))
      unaligned-mask.push(true)
    }
  }

  // Track current position in each sequence
  let seq-1-pos = first-coord.row
  let seq-2-pos = first-coord.col

  // Process each operation
  for op in operations {
    if op == "match-or-mismatch" {
      let char1 = seq-1-chars.at(seq-1-pos)
      let char2 = seq-2-chars.at(seq-2-pos)

      aligned1.push(char1)
      aligned2.push(char2)
      match-line.push(if char1 == char2 { match-char } else { mismatch-char })
      unaligned-mask.push(false)

      seq-1-pos += 1
      seq-2-pos += 1
    } else if op == "gap-in-seq1" {
      aligned1.push(gap-char)
      aligned2.push(seq-2-chars.at(seq-2-pos))
      match-line.push(mismatch-char)
      unaligned-mask.push(false)

      seq-2-pos += 1
    } else if op == "gap-in-seq2" {
      aligned1.push(seq-1-chars.at(seq-1-pos))
      aligned2.push(gap-char)
      match-line.push(mismatch-char)
      unaligned-mask.push(false)

      seq-1-pos += 1
    }
  }

  // Handle trailing unaligned region
  // Show unaligned characters if hide-unaligned is false
  if not hide-unaligned {
    // Add remaining seq-1 characters
    while seq-1-pos < seq-1-chars.len() {
      aligned1.push(seq-1-chars.at(seq-1-pos))
      match-line.push(" ")
      aligned2.push(" ")
      unaligned-mask.push(true)
      seq-1-pos += 1
    }

    // Add remaining seq-2 characters
    while seq-2-pos < seq-2-chars.len() {
      aligned1.push(" ")
      match-line.push(" ")
      aligned2.push(seq-2-chars.at(seq-2-pos))
      unaligned-mask.push(true)
      seq-2-pos += 1
    }
  }

  (
    aligned1: aligned1.join(),
    match-line: match-line.join(),
    aligned2: aligned2.join(),
    unaligned-mask: unaligned-mask,
  )
}

/// Renders a formatted pairwise sequence alignment from alignment result data.
///
/// Creates a three-line display showing the first aligned sequence (with gaps),
/// match/mismatch indicators, and the second aligned sequence (with gaps).
/// The traceback path from `align-seq-pair` goes from end to start (high indices
/// to low), so it is automatically reversed before processing.
///
/// - seq-1 (str): First sequence (without gaps).
/// - seq-2 (str): Second sequence (without gaps).
/// - path (array): Traceback path as `(row, col)` arrays, in end-to-start order.
/// - gap-char (str): Character to display for gaps (default: "–").
/// - match-char (str): Character to display for matches (default: "│").
/// - mismatch-char (str): Character to display for mismatches (default: " ").
/// - hide-unaligned (bool): Hide unaligned characters entirely (default: false).
/// - unaligned-color (color, none): Color for unaligned characters (default: none, which uses the default text color).
/// -> content
#let render-pair-alignment(
  seq-1,
  seq-2,
  path,
  gap-char: "–",
  match-char: "│",
  mismatch-char: " ",
  hide-unaligned: false,
  unaligned-color: none,
) = {
  // Validate inputs
  assert(type(seq-1) == str, message: "seq-1 must be a string.")
  assert(type(seq-2) == str, message: "seq-2 must be a string.")

  // Parse sequences
  let seq1-chars = seq-1.clusters()
  let seq2-chars = seq-2.clusters()

  // Finish validating inputs
  assert(seq1-chars.len() > 0, message: "seq-1 cannot be empty.")
  assert(seq2-chars.len() > 0, message: "seq-2 cannot be empty.")
  assert(type(path) == array, message: "path must be an array.")
  assert(path.len() > 0, message: "path cannot be empty.")

  // Reverse the path (traceback goes end-to-start, we need start-to-end)
  let reversed-path = path.rev()

  // Validate path
  _validate-path(reversed-path, seq1-chars.len(), seq2-chars.len())

  // Convert path to operations
  let operations = _path-to-operations(reversed-path)

  // Build alignment strings
  let result = _build-alignment-strings(
    seq-1,
    seq-2,
    reversed-path,
    operations,
    gap-char,
    match-char,
    mismatch-char,
    hide-unaligned,
  )

  // Render with regular font in fixed-width grid cells
  context {
    let unaligned-mask = result.unaligned-mask
    let mask-length = unaligned-mask.len()
    let has-unaligned-color = unaligned-color != none

    let make-line-cells = (chars, apply-unaligned) => {
      chars
        .enumerate()
        .map(item => {
          let (i, char) = item
          let should-color = (
            apply-unaligned
              and has-unaligned-color
              and i < mask-length
              and unaligned-mask.at(i)
          )
          if should-color { text(char, fill: unaligned-color) } else { char }
        })
    }

    let line1-cells = make-line-cells(result.aligned1.clusters(), true)
    let line2-cells = make-line-cells(result.match-line.clusters(), false)
    let line3-cells = make-line-cells(result.aligned2.clusters(), true)

    block(breakable: false, _fixed-width-grid(
      (line1-cells, line2-cells, line3-cells),
      row-heights: (text.size * 0.85, text.size * 1.6, text.size * 0.85),
    ))
  }
}
