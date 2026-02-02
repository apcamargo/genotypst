//! Scoring systems for sequence alignment.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::matrices::BuiltinMatrix;

/// Error type for alignment and scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlignmentError {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubstitutionScorer {
    /// Simple match/mismatch scoring
    Simple {
        match_score: i32,
        mismatch_score: i32,
    },
    /// Built-in substitution matrix
    Matrix(BuiltinMatrix),
}

impl SubstitutionScorer {
    /// Returns the score for aligning character `a` with character `b`.
    /// 
    /// # Errors
    /// Returns `AlignmentError::InvalidCharacter` if a character is not found in the matrix.
    pub fn score(&self, a: u8, b: u8) -> Result<i32, AlignmentError> {
        match self {
            SubstitutionScorer::Simple {
                match_score,
                mismatch_score,
            } => {
                if a.to_ascii_uppercase() == b.to_ascii_uppercase() {
                    Ok(*match_score)
                } else {
                    Ok(*mismatch_score)
                }
            }
            SubstitutionScorer::Matrix(bm) => {
                bm.score(a, b)
            }
        }
    }

    /// Validates that all characters in a sequence are valid for this scorer.
    pub fn validate(&self, seq: &[u8]) -> Result<(), AlignmentError> {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    pub scorer: SubstitutionScorer,
    pub gap_open: i32,
    pub gap_extend: i32,
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
    pub fn linear(match_score: i32, mismatch_score: i32, gap_open: i32, gap_extend: i32) -> Self {
        Self {
            scorer: SubstitutionScorer::Simple {
                match_score,
                mismatch_score,
            },
            gap_open,
            gap_extend,
        }
    }

    pub fn with_matrix(matrix: BuiltinMatrix, gap_open: i32, gap_extend: i32) -> Self {
        Self {
            scorer: SubstitutionScorer::Matrix(matrix),
            gap_open,
            gap_extend,
        }
    }

    pub fn is_affine(&self) -> bool {
        self.gap_open != self.gap_extend
    }

    pub fn substitution_score(&self, a: u8, b: u8) -> Result<i32, AlignmentError> {
        self.scorer.score(a, b)
    }

    pub fn gap_penalty(&self, length: usize) -> i32 {
        if length == 0 {
            0
        } else if self.is_affine() {
            self.gap_open.saturating_add(self.gap_extend.saturating_mul(length as i32 - 1))
        } else {
            self.gap_open.saturating_mul(length as i32)
        }
    }

    pub fn ensure_linear(&self) -> Result<(), AlignmentError> {
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

    #[test]
    fn test_simple_scoring() {
        let scorer = SubstitutionScorer::Simple {
            match_score: 5,
            mismatch_score: -3,
        };
        assert_eq!(scorer.score(b'A', b'A').unwrap(), 5);
        assert_eq!(scorer.score(b'A', b'T').unwrap(), -3);
        assert_eq!(scorer.score(b'a', b'A').unwrap(), 5);
    }

    #[test]
    fn test_blosum62_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::Blosum62);
        // A-A: 4, A-R: -1
        assert_eq!(scorer.score(b'A', b'A').unwrap(), 4);
        assert_eq!(scorer.score(b'a', b'A').unwrap(), 4);
        assert_eq!(scorer.score(b'A', b'R').unwrap(), -1);
        assert_eq!(scorer.score(b'W', b'W').unwrap(), 11);
    }

    #[test]
    fn test_ednafull_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::Ednafull);
        // A-A: 5, A-T: -4
        assert_eq!(scorer.score(b'A', b'A').unwrap(), 5);
        assert_eq!(scorer.score(b'A', b'T').unwrap(), -4);
        // N-N: -1
        assert_eq!(scorer.score(b'N', b'N').unwrap(), -1);
    }

    #[test]
    fn test_pam250_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::from_str("PAM250").unwrap());
        // A-A: 2, A-R: -2, W-W: 17
        assert_eq!(scorer.score(b'A', b'A').unwrap(), 2);
        assert_eq!(scorer.score(b'A', b'R').unwrap(), -2);
        assert_eq!(scorer.score(b'W', b'W').unwrap(), 17);
    }

    #[test]
    fn test_pam1_scoring() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::from_str("PAM1").unwrap());
        // A-A: 7
        assert_eq!(scorer.score(b'A', b'A').unwrap(), 7);
        // A-W: -inf (i32::MIN)
        assert_eq!(scorer.score(b'A', b'W').unwrap(), i32::MIN);
    }

    #[test]
    fn test_invalid_character_error() {
        let scorer = SubstitutionScorer::Matrix(BuiltinMatrix::Ednafull);
        // 'X' is not in EDNAFULL
        let res = scorer.score(b'X', b'A');
        assert!(matches!(res, Err(AlignmentError::InvalidCharacter(b'X'))));
        
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
