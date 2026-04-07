//! JSON output serialization for alignment results.

use serde::Serialize;
use serde::ser::{SerializeSeq, Serializer};

use crate::alignment::{AlignedPair, AlignmentResult, DPMatrix, TracebackPath};

/// JSON-serializable DP matrix output.
/// Scores and arrows are emitted as dense row-major arrays.
#[derive(Debug, Serialize)]
struct DPMatrixOutput {
    rows: usize,
    cols: usize,
    scores: Vec<i32>,
    arrow_bits: Vec<u8>,
}

impl From<&DPMatrix> for DPMatrixOutput {
    fn from(matrix: &DPMatrix) -> Self {
        let mut scores = Vec::with_capacity(matrix.cells.len());
        let mut arrow_bits = Vec::with_capacity(matrix.cells.len());

        for cell in &matrix.cells {
            scores.push(cell.score);
            arrow_bits.push(cell.arrows.bits());
        }

        Self {
            rows: matrix.rows,
            cols: matrix.cols,
            scores,
            arrow_bits,
        }
    }
}

/// Serialize an alignment result into the JSON payload expected by Typst.
pub(crate) fn serialize_alignment_result(
    result: &AlignmentResult,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&AlignmentResultOutputRef {
        alignment_score: result.final_score,
        alignments: AlignmentsRef(&result.alignments),
        traceback_paths: TracebackPathsRef(&result.traceback_paths),
        dp_matrix: DPMatrixOutput::from(&result.matrix),
    })
}

#[derive(Serialize)]
struct AlignmentResultOutputRef<'a> {
    alignment_score: i32,
    alignments: AlignmentsRef<'a>,
    traceback_paths: TracebackPathsRef<'a>,
    dp_matrix: DPMatrixOutput,
}

struct AlignmentsRef<'a>(&'a [AlignedPair]);

impl Serialize for AlignmentsRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for pair in self.0 {
            seq.serialize_element(&AlignmentOutputRef {
                seq1: &pair.seq1_aligned,
                seq2: &pair.seq2_aligned,
            })?;
        }
        seq.end()
    }
}

#[derive(Serialize)]
struct AlignmentOutputRef<'a> {
    seq1: &'a str,
    seq2: &'a str,
}

struct TracebackPathsRef<'a>(&'a [TracebackPath]);

impl Serialize for TracebackPathsRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for path in self.0 {
            seq.serialize_element(&TracebackPathRef(path))?;
        }
        seq.end()
    }
}

struct TracebackPathRef<'a>(&'a TracebackPath);

impl Serialize for TracebackPathRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.steps.len()))?;
        for step in &self.0.steps {
            seq.serialize_element(&[step.i, step.j])?;
        }
        seq.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aligners::GlobalAligner;
    use crate::alignment::{Arrows, Cell, DPMatrix};
    use crate::scoring::ScoringConfig;
    use serde_json::Value;

    #[test]
    fn test_dp_matrix_output_format() {
        let aligner = GlobalAligner::new(ScoringConfig::default());
        let result = aligner.align(b"AC", b"AC").unwrap();

        let output = DPMatrixOutput::from(&result.matrix);

        // Check dp_matrix has dense row-major scores and arrow bitmasks
        assert_eq!(output.rows, 3); // len("AC") + 1
        assert_eq!(output.cols, 3); // len("AC") + 1
        assert_eq!(output.scores.len(), 9); // 3 * 3
        assert_eq!(output.arrow_bits.len(), 9); // 3 * 3
        assert_eq!(output.scores[0], 0);
        assert_eq!(output.scores[1], -2);
        assert_eq!(output.scores[3], -2);
        assert!(output.arrow_bits.iter().any(|bits| *bits != 0));
    }

    #[test]
    fn test_traceback_paths_format() {
        let aligner = GlobalAligner::new(ScoringConfig::default());
        let result = aligner.align(b"AC", b"AC").unwrap();

        let json = serialize_alignment_result(&result).unwrap();
        let value: Value = serde_json::from_slice(&json).unwrap();
        let traceback_paths = value["traceback_paths"].as_array().unwrap();

        // Check traceback_paths is Vec<Vec<[usize; 2]>>
        assert!(!traceback_paths.is_empty());
        for path in traceback_paths {
            let path = path.as_array().unwrap();
            assert!(!path.is_empty());
            for coord in path {
                assert_eq!(coord.as_array().unwrap().len(), 2);
            }
        }
    }

    #[test]
    fn test_collect_arrow_bits_preserves_multiple_directions() {
        let mut matrix = DPMatrix::new(2, 2);
        let mut arrows = Arrows::new();
        arrows.set_diagonal();
        arrows.set_up();
        arrows.set_left();
        matrix.set(1, 1, Cell::with_arrows(5, arrows));

        let output = DPMatrixOutput::from(&matrix);

        assert_eq!(output.arrow_bits, vec![0, 0, 0, 7]);
    }

    #[test]
    fn test_collect_scores_uses_row_major_order() {
        let mut matrix = DPMatrix::new(2, 2);
        matrix.set(0, 0, Cell::new(10));
        matrix.set(0, 1, Cell::new(11));
        matrix.set(1, 0, Cell::new(12));
        matrix.set(1, 1, Cell::new(13));

        let output = DPMatrixOutput::from(&matrix);

        assert_eq!(output.scores, vec![10, 11, 12, 13]);
    }
}
