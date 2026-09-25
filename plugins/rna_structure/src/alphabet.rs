//! Nucleotide alphabet shared by prediction and drawing.

/// Returns whether `byte` is an IUPAC nucleotide code, in either case.
pub(crate) const fn is_iupac_nucleotide(byte: u8) -> bool {
    matches!(
        byte.to_ascii_uppercase(),
        b'A' | b'C'
            | b'G'
            | b'U'
            | b'T'
            | b'R'
            | b'Y'
            | b'S'
            | b'W'
            | b'K'
            | b'M'
            | b'B'
            | b'D'
            | b'H'
            | b'V'
            | b'N'
    )
}
