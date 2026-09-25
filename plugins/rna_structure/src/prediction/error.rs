/// Failure returned by RNA secondary-structure prediction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FoldError {
    #[error("the sequence is empty")]
    EmptySequence,
    #[error("invalid sequence byte {byte:#04x} at index {index}")]
    InvalidSequenceByte { index: usize, byte: u8 },
    #[error("constraint length {constraint_len} does not match sequence length {sequence_len}")]
    ConstraintLengthMismatch {
        sequence_len: usize,
        constraint_len: usize,
    },
    #[error("invalid constraint byte {byte:#04x} at index {index}")]
    InvalidConstraintByte { index: usize, byte: u8 },
    #[error("closing parenthesis at index {index} has no opener")]
    UnmatchedClosingParenthesis { index: usize },
    #[error("opening parenthesis at index {index} has no closer")]
    UnmatchedOpeningParenthesis { index: usize },
    #[error("forced pair {left}..{right} is not AU, CG, or GU")]
    NoncanonicalConstrainedPair { left: usize, right: usize },
    #[error(
        "forced pair {left}..{right} encloses fewer than three bases, which the Vienna RNAfold model cannot score"
    )]
    SharpConstrainedPair { left: usize, right: usize },
    #[error("no structure satisfies the supplied constraints")]
    NoValidStructure,
    #[error("circular folding is supported only by the Vienna RNAfold model")]
    CircularFoldingRequiresViennaRnafold,
    #[error("internal folding invariant failed: {0}")]
    InternalInvariant(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PredictionError {
    #[error("invalid prediction config JSON: {0}")]
    Config(#[source] serde_json::Error),
    #[error("prediction failed: {0}")]
    Folding(#[source] FoldError),
    #[error("prediction serialization failed: {0}")]
    Serialization(#[source] serde_json::Error),
}
