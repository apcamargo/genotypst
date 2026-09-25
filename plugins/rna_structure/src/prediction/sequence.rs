use crate::alphabet::is_iupac_nucleotide;
use crate::prediction::FoldError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Base {
    A = 0,
    C = 1,
    G = 2,
    U = 3,
    Unknown = 4,
}

impl Base {
    pub(crate) const fn can_pair(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::A | Self::G, Self::U)
                | (Self::U, Self::A | Self::G)
                | (Self::C, Self::G)
                | (Self::G, Self::C)
        )
    }
}

pub(crate) fn normalize(sequence: &[u8]) -> Result<Vec<Base>, FoldError> {
    if sequence.is_empty() {
        return Err(FoldError::EmptySequence);
    }

    sequence
        .iter()
        .copied()
        .enumerate()
        .map(|(index, byte)| {
            if !is_iupac_nucleotide(byte) {
                return Err(FoldError::InvalidSequenceByte { index, byte });
            }
            Ok(match byte.to_ascii_uppercase() {
                b'A' => Base::A,
                b'C' => Base::C,
                b'G' => Base::G,
                b'U' | b'T' => Base::U,
                _ => Base::Unknown,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_dna_and_unknown_bases() {
        assert_eq!(
            normalize(b"ACGUacgu").unwrap(),
            vec![
                Base::A,
                Base::C,
                Base::G,
                Base::U,
                Base::A,
                Base::C,
                Base::G,
                Base::U
            ]
        );
        // T is DNA for U, and every other IUPAC letter maps to Unknown.
        assert_eq!(
            normalize(b"ACGTRYSWKMBDHVN").unwrap(),
            vec![
                Base::A,
                Base::C,
                Base::G,
                Base::U,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown,
                Base::Unknown
            ]
        );
    }

    #[test]
    fn rejects_empty_and_non_iupac_sequences() {
        assert_eq!(normalize(b""), Err(FoldError::EmptySequence));
        assert_eq!(
            normalize(b"AC GU"),
            Err(FoldError::InvalidSequenceByte {
                index: 2,
                byte: b' '
            })
        );
        assert_eq!(
            normalize(b">ACGU"),
            Err(FoldError::InvalidSequenceByte {
                index: 0,
                byte: b'>'
            })
        );
        assert_eq!(
            normalize(b"GGGGXAAAA"),
            Err(FoldError::InvalidSequenceByte {
                index: 4,
                byte: b'X'
            })
        );
    }
}
