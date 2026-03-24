#let _alignment-backend = plugin("alignment.wasm")

// Cache available matrices at module load time (loaded once)
#let _available-matrices = json(_alignment-backend.list_matrices()).matrices

/// Resolves a scoring matrix name to its canonical form.
///
/// Performs case-insensitive lookup against available matrices from the WASM plugin.
/// Returns the canonical matrix name (e.g., "BLOSUM62") if found, or none if not found.
///
/// - name (str): Matrix name to look up (case-insensitive).
/// -> str, none
#let resolve-matrix-name(name) = {
  let upper-name = upper(name)
  if upper-name in _available-matrices {
    upper-name
  } else {
    none
  }
}

/// Private: Converts a flat row-major array to a 2D array.
///
/// Takes a flat array and reshapes it into a 2D nested array using
/// row-major indexing: element at (i, j) = flat[i * cols + j].
///
/// - cell-values (array): Flat array of cell values.
/// - rows (int): Number of rows in the output.
/// - cols (int): Number of columns in the output.
/// -> array
#let _flat-to-2d(cell-values, rows, cols) = {
  range(rows).map(i => range(cols).map(j => cell-values.at(i * cols + j)))
}

/// Private: Converts WASM i32 infinity representations to Typst floats.
///
/// The WASM plugin uses i32::MIN (-2147483648) for negative infinity
/// and i32::MAX (2147483647) for positive infinity. This function
/// converts these sentinel values to Typst's float.inf representation.
///
/// - value (int): The value to convert.
/// -> int, float
#let _convert-infinity(value) = {
  if value == -2147483648 { -float.inf } else if value == 2147483647 {
    float.inf
  } else { value }
}
