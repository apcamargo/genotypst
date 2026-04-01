//! Scoring systems for sequence alignment.

use std::fmt;

use crate::matrices::BuiltinMatrix;

/// Error type for alignment and scoring.
#[derive(Debug, Clone)]
pub(crate) enum AlignmentError {
    /// Character not found in substitution matrix
    InvalidCharacter(u8),
    /// Other error
    Other(String),
}

impl fmt::Display for AlignmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlignmentError::InvalidCharacter(c) => {
                write!(f, "Invalid character in sequence: '{}'", *c as char)
            }
            AlignmentError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for AlignmentError {}

/// Substitution scoring source: either simple match/mismatch or a matrix.
#[derive(Debug, Clone)]
pub(crate) enum SubstitutionScorer {
    /// Simple match/mismatch scoring
    Simple {
        match_score: i32,
        mismatch_score: i32,
    },
    /// Built-in substitution matrix
    Matrix(BuiltinMatrix),
}

impl SubstitutionScorer {
    /// Validates that all characters in a sequence are valid for this scorer.
    pub(crate) fn validate(&self, seq: &[u8]) -> Result<(), AlignmentError> {
        match self {
            SubstitutionScorer::Simple { .. } => Ok(()), // All bytes are theoretically valid for simple
            SubstitutionScorer::Matrix(bm) => {
                let map = bm.lookup_map();
                for &c in seq {
                    if map[c as usize].is_none() {
                        return Err(AlignmentError::InvalidCharacter(c));
                    }
                }
                Ok(())
            }
        }
    }
}

/// Combined scoring configuration for alignment algorithms.
#[derive(Debug, Clone)]
pub(crate) struct ScoringConfig {
    pub(crate) scorer: SubstitutionScorer,
    pub(crate) gap_open: i32,
    pub(crate) gap_extend: i32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            scorer: SubstitutionScorer::Simple {
                match_score: 3,
                mismatch_score: -1,
            },
            gap_open: -2,
            gap_extend: -2,
        }
    }
}

impl ScoringConfig {
    pub(crate) fn linear(
        match_score: i32,
        mismatch_score: i32,
        gap_open: i32,
        gap_extend: i32,
    ) -> Self {
        Self {
            scorer: SubstitutionScorer::Simple {
                match_score,
                mismatch_score,
            },
            gap_open,
            gap_extend,
        }
    }

    pub(crate) fn with_matrix(matrix: BuiltinMatrix, gap_open: i32, gap_extend: i32) -> Self {
        Self {
            scorer: SubstitutionScorer::Matrix(matrix),
            gap_open,
            gap_extend,
        }
    }

    pub(crate) fn is_affine(&self) -> bool {
        self.gap_open != self.gap_extend
    }

    pub(crate) fn gap_penalty(&self, length: usize) -> i32 {
        if length == 0 {
            0
        } else if self.is_affine() {
            self.gap_open
                .saturating_add(self.gap_extend.saturating_mul(length as i32 - 1))
        } else {
            self.gap_open.saturating_mul(length as i32)
        }
    }

    pub(crate) fn ensure_linear(&self) -> Result<(), AlignmentError> {
        if self.is_affine() {
            Err(AlignmentError::Other(format!(
                "Affine gap penalties are not supported yet (gap_open={}, gap_extend={})",
                self.gap_open, self.gap_extend
            )))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix_score(matrix: BuiltinMatrix, a: u8, b: u8) -> Result<i32, AlignmentError> {
        let map = matrix.lookup_map();
        let i = map[a as usize].ok_or(AlignmentError::InvalidCharacter(a))?;
        let j = map[b as usize].ok_or(AlignmentError::InvalidCharacter(b))?;
        let n = matrix.score_dimension();
        Ok(matrix.scores()[i as usize * n + j as usize])
    }

    #[test]
    fn test_simple_scoring() {
        let scorer = SubstitutionScorer::Simple {
            match_score: 5,
            mismatch_score: -3,
        };
        match scorer {
            SubstitutionScorer::Simple {
                match_score,
                mismatch_score,
            } => {
                assert_eq!(match_score, 5);
                assert_eq!(mismatch_score, -3);
            }
            SubstitutionScorer::Matrix(_) => panic!("expected simple scorer"),
        }
    }

    #[test]
    fn test_blosum62_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::Blosum62);
        if let SubstitutionScorer::Matrix(matrix) = scorer {
            // A-A: 4, A-R: -1
            assert_eq!(matrix_score(matrix, b'A', b'A').unwrap(), 4);
            assert_eq!(matrix_score(matrix, b'a', b'A').unwrap(), 4);
            assert_eq!(matrix_score(matrix, b'A', b'R').unwrap(), -1);
            assert_eq!(matrix_score(matrix, b'W', b'W').unwrap(), 11);
        } else {
            panic!("expected matrix scorer");
        }
    }

    #[test]
    fn test_ednafull_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::Ednafull);
        if let SubstitutionScorer::Matrix(matrix) = scorer {
            // A-A: 5, A-T: -4
            assert_eq!(matrix_score(matrix, b'A', b'A').unwrap(), 5);
            assert_eq!(matrix_score(matrix, b'A', b'T').unwrap(), -4);
            // N-N: -1
            assert_eq!(matrix_score(matrix, b'N', b'N').unwrap(), -1);
        } else {
            panic!("expected matrix scorer");
        }
    }

    #[test]
    fn test_pam250_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::from_str("PAM250").unwrap());
        if let SubstitutionScorer::Matrix(matrix) = scorer {
            // A-A: 2, A-R: -2, W-W: 17
            assert_eq!(matrix_score(matrix, b'A', b'A').unwrap(), 2);
            assert_eq!(matrix_score(matrix, b'A', b'R').unwrap(), -2);
            assert_eq!(matrix_score(matrix, b'W', b'W').unwrap(), 17);
        } else {
            panic!("expected matrix scorer");
        }
    }

    #[test]
    fn test_pam1_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::from_str("PAM1").unwrap());
        if let SubstitutionScorer::Matrix(matrix) = scorer {
            // A-A: 7
            assert_eq!(matrix_score(matrix, b'A', b'A').unwrap(), 7);
            // A-W: -inf (i32::MIN)
            assert_eq!(matrix_score(matrix, b'A', b'W').unwrap(), i32::MIN);
        } else {
            panic!("expected matrix scorer");
        }
    }

    #[test]
    fn test_invalid_character_error() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::Ednafull);
        if let SubstitutionScorer::Matrix(matrix) = scorer {
            // 'X' is not in EDNAFULL
            let res = matrix_score(matrix, b'X', b'A');
            assert!(matches!(res, Err(AlignmentError::InvalidCharacter(b'X'))));
        } else {
            panic!("expected matrix scorer");
        }

        let res_v = scorer.validate(b"ATGCX");
        assert!(matches!(res_v, Err(AlignmentError::InvalidCharacter(b'X'))));
    }

    #[test]
    fn test_scoring_config_gap_penalty() {
        let config = ScoringConfig::linear(3, -1, -2, -2);
        assert_eq!(config.gap_penalty(0), 0);
        assert_eq!(config.gap_penalty(1), -2);
        assert_eq!(config.gap_penalty(3), -6);
    }

    #[test]
    fn test_affine_gap_not_supported() {
        let config = ScoringConfig {
            scorer: SubstitutionScorer::Simple {
                match_score: 1,
                mismatch_score: -1,
            },
            gap_open: -2,
            gap_extend: -1,
        };
        let err = config.ensure_linear().unwrap_err();
        assert!(matches!(err, AlignmentError::Other(_)));
    }
}
