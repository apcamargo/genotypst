//! JSON output serialization for alignment results.

use serde::Serialize;

use crate::alignment::{AlignedPair, AlignmentResult, DPMatrix};
use crate::scoring::ScoringConfig;

/// JSON-serializable DP matrix output.
/// Scores are emitted as `([row, col], score)` entries and arrows as
/// `[[from_row, from_col], [to_row, to_col]]` pairs.
#[derive(Debug, Serialize)]
pub struct DPMatrixOutput {
    pub rows: usize,
    pub cols: usize,
    pub cell_values: Vec<([usize; 2], i32)>,
    pub arrows: Vec<[[usize; 2]; 2]>,
}

fn collect_cell_values(matrix: &DPMatrix) -> Vec<([usize; 2], i32)> {
    let mut cell_values = Vec::with_capacity(matrix.rows * matrix.cols);

    for i in 0..matrix.rows {
        for j in 0..matrix.cols {
            cell_values.push(([i, j], matrix.get(i, j).score));
        }
    }

    cell_values
}

fn collect_arrow_pairs(matrix: &DPMatrix) -> Vec<[[usize; 2]; 2]> {
    let mut arrows = Vec::new();

    for i in 0..matrix.rows {
        for j in 0..matrix.cols {
            let cell = matrix.get(i, j);

            if cell.arrows.has_diagonal() && i > 0 && j > 0 {
                arrows.push([[i, j], [i - 1, j - 1]]);
            }
            if cell.arrows.has_up() && i > 0 {
                arrows.push([[i, j], [i - 1, j]]);
            }
            if cell.arrows.has_left() && j > 0 {
                arrows.push([[i, j], [i, j - 1]]);
            }
        }
    }

    arrows
}

impl From<&DPMatrix> for DPMatrixOutput {
    fn from(matrix: &DPMatrix) -> Self {
        Self {
            rows: matrix.rows,
            cols: matrix.cols,
            cell_values: collect_cell_values(matrix),
            arrows: collect_arrow_pairs(matrix),
        }
    }
}

/// JSON-serializable representation of an aligned sequence pair.
#[derive(Debug, Serialize)]
pub struct AlignmentOutput {
    pub seq1: String,
    pub seq2: String,
}

impl From<&AlignedPair> for AlignmentOutput {
    fn from(pair: &AlignedPair) -> Self {
        Self {
            seq1: pair.seq1_aligned.clone(),
            seq2: pair.seq2_aligned.clone(),
        }
    }
}

/// Complete JSON output for an alignment result.
#[derive(Debug, Serialize)]
pub struct AlignmentResultOutput {
    pub seq1: String,
    pub seq2: String,
    pub alignment_score: i32,
    pub scoring: ScoringConfig,
    pub alignments: Vec<AlignmentOutput>,
    pub traceback_paths: Vec<Vec<[usize; 2]>>,
    pub dp_matrix: DPMatrixOutput,
}

impl From<&AlignmentResult> for AlignmentResultOutput {
    fn from(result: &AlignmentResult) -> Self {
        // Convert traceback paths to Vec<Vec<[usize; 2]>> format
        let traceback_paths: Vec<Vec<[usize; 2]>> = result
            .traceback_paths
            .iter()
            .map(|path| path.steps.iter().map(|step| [step.i, step.j]).collect())
            .collect();

        Self {
            seq1: result.seq1.clone(),
            seq2: result.seq2.clone(),
            alignment_score: result.final_score,
            scoring: result.scoring.clone(),
            alignments: result
                .alignments
                .iter()
                .map(AlignmentOutput::from)
                .collect(),
            traceback_paths,
            dp_matrix: DPMatrixOutput::from(&result.matrix),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aligners::GlobalAligner;
    use crate::alignment::{Aligner, Arrows, Cell, DPMatrix};

    #[test]
    fn test_json_serialization() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"AC", b"AC").unwrap();

        let json = serde_json::to_string(&AlignmentResultOutput::from(&result)).unwrap();
        assert!(json.contains("\"alignment_score\":6"));
        assert!(json.contains("\"seq1\":\"AC\""));
    }

    #[test]
    fn test_dp_matrix_output_format() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"AC", b"AC").unwrap();

        let output = AlignmentResultOutput::from(&result);

        // Check dp_matrix has coordinate-addressable cell values and arrows
        assert_eq!(output.dp_matrix.rows, 3); // len("AC") + 1
        assert_eq!(output.dp_matrix.cols, 3); // len("AC") + 1
        assert_eq!(output.dp_matrix.cell_values.len(), 9); // 3 * 3
        assert_eq!(output.dp_matrix.cell_values[0], ([0, 0], 0));
        assert_eq!(output.dp_matrix.cell_values[1], ([0, 1], -2));
        assert_eq!(output.dp_matrix.cell_values[3], ([1, 0], -2));
        assert!(!output.dp_matrix.arrows.is_empty());
        for arrow in &output.dp_matrix.arrows {
            assert_eq!(arrow.len(), 2);
            assert_eq!(arrow[0].len(), 2);
            assert_eq!(arrow[1].len(), 2);
        }
    }

    #[test]
    fn test_traceback_paths_format() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"AC", b"AC").unwrap();

        let output = AlignmentResultOutput::from(&result);

        // Check traceback_paths is Vec<Vec<[usize; 2]>>
        assert!(!output.traceback_paths.is_empty());
        for path in &output.traceback_paths {
            assert!(!path.is_empty());
            for coord in path {
                assert_eq!(coord.len(), 2);
            }
        }
    }

    #[test]
    fn test_collect_arrow_pairs_emits_multiple_directions() {
        let mut matrix = DPMatrix::new(2, 2);
        let mut arrows = Arrows::new();
        arrows.set_diagonal();
        arrows.set_up();
        arrows.set_left();
        matrix.set(1, 1, Cell::with_arrows(5, arrows));

        let output = DPMatrixOutput::from(&matrix);

        assert_eq!(
            output.arrows,
            vec![[[1, 1], [0, 0]], [[1, 1], [0, 1]], [[1, 1], [1, 0]]]
        );
    }

    #[test]
    fn test_collect_arrow_pairs_omits_cells_without_arrows() {
        let matrix = DPMatrix::new(2, 2);

        let output = DPMatrixOutput::from(&matrix);

        assert!(output.arrows.is_empty());
    }

    #[test]
    fn test_collect_cell_values_uses_row_major_coordinates() {
        let mut matrix = DPMatrix::new(2, 2);
        matrix.set(0, 0, Cell::new(10));
        matrix.set(0, 1, Cell::new(11));
        matrix.set(1, 0, Cell::new(12));
        matrix.set(1, 1, Cell::new(13));

        let output = DPMatrixOutput::from(&matrix);

        assert_eq!(
            output.cell_values,
            vec![([0, 0], 10), ([0, 1], 11), ([1, 0], 12), ([1, 1], 13)]
        );
    }
}
