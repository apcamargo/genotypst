#[derive(Debug, thiserror::Error)]
pub(crate) enum LayoutError {
    #[error("sequence length {sequence} does not match structure length {structure}")]
    LengthMismatch { sequence: usize, structure: usize },

    #[error("input sequence and structure must not be empty")]
    EmptyInput,

    #[error("invalid sequence character {found:?} at index {index}")]
    InvalidSequenceChar { index: usize, found: char },

    #[error("invalid structure character {found:?} at index {index}")]
    InvalidStructureChar { index: usize, found: char },

    #[error("unmatched opening bracket {bracket:?} at index {index}")]
    UnmatchedOpeningBracket { index: usize, bracket: char },

    #[error("unmatched closing bracket {bracket:?} at index {index}")]
    UnmatchedClosingBracket { index: usize, bracket: char },

    #[error(
        "pseudoknotted structures are unsupported: pair {first_i}-{first_j} crosses {second_i}-{second_j}"
    )]
    PseudoknotUnsupported {
        first_i: usize,
        first_j: usize,
        second_i: usize,
        second_j: usize,
    },

    #[error("internal layout invariant failed: {message}")]
    InternalInvariant { message: String },
}
