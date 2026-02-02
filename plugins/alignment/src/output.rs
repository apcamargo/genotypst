//! JSON output serialization for alignment results.

use serde::Serialize;

use crate::alignment::{AlignedPair, AlignmentResult, DPMatrix};
use crate::scoring::ScoringConfig;

/// JSON-serializable representation of the DP matrix with separate scores and arrows arrays.
#[derive(Debug, Serialize)]
pub struct DPMatrixOutput {
    pub rows: usize,
    pub cols: usize,
    pub scores: Vec<i32>,
    pub arrows: Vec<u8>,
}

impl From<&DPMatrix> for DPMatrixOutput {
    fn from(matrix: &DPMatrix) -> Self {
        let scores: Vec<i32> = matrix.cells.iter().map(|c| c.score).collect();
        let arrows: Vec<u8> = matrix.cells.iter().map(|c| c.arrows.bits()).collect();
        Self {
            rows: matrix.rows,
            cols: matrix.cols,
            scores,
            arrows,
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
            .map(|path| {
                path.steps
                    .iter()
                    .map(|step| [step.i, step.j])
                    .collect()
            })
            .collect();

        Self {
            seq1: result.seq1.clone(),
            seq2: result.seq2.clone(),
            alignment_score: result.final_score,
            scoring: result.scoring.clone(),
            alignments: result.alignments.iter().map(AlignmentOutput::from).collect(),
            traceback_paths,
            dp_matrix: DPMatrixOutput::from(&result.matrix),
        }
    }
}

impl AlignmentResultOutput {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

pub fn result_to_json(result: &AlignmentResult) -> Result<String, serde_json::Error> {
    let output = AlignmentResultOutput::from(result);
    output.to_json()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alignment::Aligner;
    use crate::aligners::GlobalAligner;

    #[test]
    fn test_json_serialization() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"AC", b"AC").unwrap();

        let json = result_to_json(&result).unwrap();
        assert!(json.contains("\"alignment_score\":6"));
        assert!(json.contains("\"seq1\":\"AC\""));
    }

    #[test]
    fn test_dp_matrix_output_format() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"AC", b"AC").unwrap();

        let output = AlignmentResultOutput::from(&result);

        // Check dp_matrix has separate scores and arrows arrays
        assert_eq!(output.dp_matrix.rows, 3); // len("AC") + 1
        assert_eq!(output.dp_matrix.cols, 3); // len("AC") + 1
        assert_eq!(output.dp_matrix.scores.len(), 9); // 3 * 3
        assert_eq!(output.dp_matrix.arrows.len(), 9); // 3 * 3
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
}
