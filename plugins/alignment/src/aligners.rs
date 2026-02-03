//! Pairwise alignment algorithms.

use crate::alignment::{
    Aligner, AlignmentResult, Arrows, Cell, DPMatrix, fill_matrix_linear, traceback_all_paths,
};
use crate::scoring::{AlignmentError, ScoringConfig};

/// Global alignment algorithm.
#[derive(Debug, Clone)]
pub struct GlobalAligner {
    scoring: ScoringConfig,
}

impl GlobalAligner {
    pub fn new(scoring: ScoringConfig) -> Self {
        Self { scoring }
    }

    #[cfg(test)]
    pub fn with_defaults() -> Self {
        Self::new(ScoringConfig::default())
    }

    fn initialize_matrix(&self, n: usize, m: usize) -> DPMatrix {
        let mut matrix = DPMatrix::new(n + 1, m + 1);
        matrix.set(0, 0, Cell::new(0));

        for i in 1..=n {
            let score = self.scoring.gap_penalty(i);
            let mut arrows = Arrows::new();
            arrows.set_up();
            matrix.set(i, 0, Cell::with_arrows(score, arrows));
        }

        for j in 1..=m {
            let score = self.scoring.gap_penalty(j);
            let mut arrows = Arrows::new();
            arrows.set_left();
            matrix.set(0, j, Cell::with_arrows(score, arrows));
        }

        matrix
    }
}

impl Aligner for GlobalAligner {
    fn align(&self, seq1: &[u8], seq2: &[u8]) -> Result<AlignmentResult, AlignmentError> {
        let n = seq1.len();
        let m = seq2.len();

        self.scoring.ensure_linear()?;

        // Validate sequences first
        self.scoring.scorer.validate(seq1)?;
        self.scoring.scorer.validate(seq2)?;

        let mut matrix = self.initialize_matrix(n, m);
        fill_matrix_linear(&mut matrix, seq1, seq2, &self.scoring, false)?;

        let final_score = matrix.get(n, m).score;
        let start_positions = [(n, m)];
        let (traceback_paths, alignments) = traceback_all_paths(
            &matrix,
            seq1,
            seq2,
            &start_positions,
            |i, j, _| i == 0 && j == 0,
            false,
        );

        Ok(AlignmentResult {
            seq1: String::from_utf8_lossy(seq1).into_owned(),
            seq2: String::from_utf8_lossy(seq2).into_owned(),
            scoring: self.scoring.clone(),
            matrix,
            traceback_paths,
            alignments,
            final_score,
        })
    }
}

#[cfg(test)]
mod global_tests {
    use super::*;
    use crate::matrices::BuiltinMatrix;

    #[test]
    fn test_identical_sequences() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"ACGT", b"ACGT").unwrap();

        // All matches: 4 * 3 = 12
        assert_eq!(result.final_score, 12);
        assert_eq!(result.alignments.len(), 1);
        assert_eq!(result.alignments[0].seq1_aligned, "ACGT");
        assert_eq!(result.alignments[0].seq2_aligned, "ACGT");
    }

    #[test]
    fn test_simple_alignment() {
        let scoring = ScoringConfig::linear(1, -1, -1, -1);
        let aligner = GlobalAligner::new(scoring);
        let result = aligner.align(b"GAC", b"ACG").unwrap();

        assert_eq!(result.final_score, 0);
    }

    #[test]
    fn test_with_gaps() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"ACGT", b"AGT").unwrap();

        assert!(
            result
                .alignments
                .iter()
                .any(|a| a.seq1_aligned.contains('-') || a.seq2_aligned.contains('-'))
        );
    }

    #[test]
    fn test_empty_sequence() {
        let aligner = GlobalAligner::with_defaults();
        let result = aligner.align(b"ACGT", b"").unwrap();

        // All gaps: 4 * -2 = -8
        assert_eq!(result.final_score, -8);
    }

    #[test]
    fn test_with_matrix_alignment() {
        let scoring = ScoringConfig::with_matrix(BuiltinMatrix::Blosum62, -2, -2);
        let aligner = GlobalAligner::new(scoring);
        // HEAGAWGHEE vs PAWHEAE
        let result = aligner.align(b"HEAGAWGHEE", b"PAWHEAE").unwrap();
        assert!(result.final_score > 0);
    }

    #[test]
    fn test_pam1_forbidden_alignment() {
        let scoring =
            ScoringConfig::with_matrix(BuiltinMatrix::from_str("PAM1").unwrap(), -10, -10);
        let aligner = GlobalAligner::new(scoring);
        // A vs W is forbidden (-inf) in PAM1.
        // An alignment should prefer gaps over matching A with W.
        let result = aligner.align(b"AA", b"AW").unwrap();

        // Aligned pair should NOT be (A, A) and (A, W)
        // Instead it should be something like:
        // A A -
        // A - W
        // (A-A, A--, -W)
        for aln in result.alignments {
            assert_ne!(aln.seq1_aligned, "AA");
            assert_ne!(aln.seq2_aligned, "AW");
        }
    }

    #[test]
    fn test_invalid_character_alignment() {
        let scoring = ScoringConfig::with_matrix(BuiltinMatrix::Ednafull, -2, -2);
        let aligner = GlobalAligner::new(scoring);
        let result = aligner.align(b"ATGCX", b"ATGC");
        assert!(matches!(
            result,
            Err(AlignmentError::InvalidCharacter(b'X'))
        ));
    }
}

/// Local alignment algorithm.
#[derive(Debug, Clone)]
pub struct LocalAligner {
    scoring: ScoringConfig,
}

impl LocalAligner {
    pub fn new(scoring: ScoringConfig) -> Self {
        Self { scoring }
    }

    #[cfg(test)]
    pub fn with_defaults() -> Self {
        Self::new(ScoringConfig::default())
    }

    fn initialize_matrix(&self, n: usize, m: usize) -> DPMatrix {
        // In local alignment, first row and column are initialized to 0
        // (no arrows needed - they represent the option to start fresh)
        let mut matrix = DPMatrix::new(n + 1, m + 1);

        for i in 0..=n {
            matrix.set(i, 0, Cell::new(0));
        }
        for j in 0..=m {
            matrix.set(0, j, Cell::new(0));
        }

        matrix
    }
}

impl Aligner for LocalAligner {
    fn align(&self, seq1: &[u8], seq2: &[u8]) -> Result<AlignmentResult, AlignmentError> {
        let n = seq1.len();
        let m = seq2.len();

        self.scoring.ensure_linear()?;

        // Validate sequences first
        self.scoring.scorer.validate(seq1)?;
        self.scoring.scorer.validate(seq2)?;

        let mut matrix = self.initialize_matrix(n, m);
        let fill_result = fill_matrix_linear(&mut matrix, seq1, seq2, &self.scoring, true)?;
        let final_score = fill_result.max_score;
        let max_positions = fill_result.max_positions;

        let (traceback_paths, alignments) = if final_score > 0 {
            traceback_all_paths(
                &matrix,
                seq1,
                seq2,
                &max_positions,
                |i, j, cell| cell.score == 0 || (i == 0 && j == 0),
                true,
            )
        } else {
            // No alignment found (all scores <= 0)
            (Vec::new(), Vec::new())
        };

        Ok(AlignmentResult {
            seq1: String::from_utf8_lossy(seq1).into_owned(),
            seq2: String::from_utf8_lossy(seq2).into_owned(),
            scoring: self.scoring.clone(),
            matrix,
            traceback_paths,
            alignments,
            final_score,
        })
    }
}

#[cfg(test)]
mod local_tests {
    use super::*;

    #[test]
    fn test_identical_sequences() {
        let aligner = LocalAligner::with_defaults();
        let result = aligner.align(b"ACGT", b"ACGT").unwrap();

        // All matches: 4 * 3 = 12
        assert_eq!(result.final_score, 12);
        assert_eq!(result.alignments.len(), 1);
        assert_eq!(result.alignments[0].seq1_aligned, "ACGT");
        assert_eq!(result.alignments[0].seq2_aligned, "ACGT");
    }

    #[test]
    fn test_local_alignment_finds_best_region() {
        // Test that local alignment finds the best matching region
        let scoring = ScoringConfig::linear(2, -1, -2, -2);
        let aligner = LocalAligner::new(scoring);

        // The best local alignment should be "GCT" matching "GCT"
        let result = aligner.align(b"AAAGCTAAA", b"CGCT").unwrap();

        // Score should be 3 matches * 2 = 6
        assert_eq!(result.final_score, 6);
        assert!(
            result
                .alignments
                .iter()
                .any(|a| a.seq1_aligned.contains("GCT") && a.seq2_aligned.contains("GCT"))
        );
    }

    #[test]
    fn test_no_good_alignment() {
        let scoring = ScoringConfig::linear(1, -3, -3, -3);
        let aligner = LocalAligner::new(scoring);

        // Very different sequences with harsh penalties
        let result = aligner.align(b"AAAA", b"TTTT").unwrap();

        // Should have score 0 or empty alignments
        assert!(result.final_score <= 0 || result.alignments.is_empty());
    }

    #[test]
    fn test_first_row_col_are_zero() {
        let aligner = LocalAligner::with_defaults();
        let result = aligner.align(b"ACG", b"ACG").unwrap();

        // First row should all be 0
        for j in 0..=3 {
            assert_eq!(result.matrix.get(0, j).score, 0);
        }
        // First column should all be 0
        for i in 0..=3 {
            assert_eq!(result.matrix.get(i, 0).score, 0);
        }
    }

    #[test]
    fn test_no_negative_scores() {
        let aligner = LocalAligner::with_defaults();
        let result = aligner.align(b"ACGT", b"TGCA").unwrap();

        // All cells should have score >= 0
        for cell in &result.matrix.cells {
            assert!(cell.score >= 0, "Found negative score: {}", cell.score);
        }
    }
}
