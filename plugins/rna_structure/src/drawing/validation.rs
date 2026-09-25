use crate::alphabet::is_iupac_nucleotide;
use crate::drawing::error::LayoutError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) struct BasePair {
    pub(crate) i: usize,
    pub(crate) j: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LoopElement {
    Unpaired(usize),
    Stem { start: usize, end: usize },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopCounts {
    pub(crate) unpaired: usize,
    pub(crate) stems: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PairTable {
    partners: Vec<usize>,
}

impl PairTable {
    fn from_partners(partners: Vec<usize>) -> Self {
        Self { partners }
    }

    pub(crate) fn len(&self) -> usize {
        self.partners[0]
    }

    pub(crate) fn raw_partner(&self, index: usize) -> usize {
        self.partners.get(index).copied().unwrap_or_default()
    }

    pub(crate) fn is_paired(&self, index: usize) -> bool {
        index != 0 && self.raw_partner(index) != 0
    }

    pub(crate) fn has_pairs(&self) -> bool {
        self.partners[1..].iter().any(|&partner| partner != 0)
    }

    /// Yields every base pair once, as 1-based `(opener, closer)`, ordered by
    /// opener.
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (1..=self.len()).filter_map(|opener| {
            let closer = self.raw_partner(opener);
            (opener < closer).then_some((opener, closer))
        })
    }

    pub(crate) fn loop_elements(&self, start: usize) -> impl Iterator<Item = LoopElement> + '_ {
        let end = self.raw_partner(start);
        let mut index = if end == 0 || start >= end {
            end
        } else {
            start + 1
        };

        std::iter::from_fn(move || {
            while index < end {
                let partner = self.raw_partner(index);
                if partner == 0 {
                    let element = LoopElement::Unpaired(index);
                    index += 1;
                    return Some(element);
                }
                if partner > index {
                    let element = LoopElement::Stem {
                        start: index,
                        end: partner,
                    };
                    index = partner + 1;
                    return Some(element);
                }
                index += 1;
            }
            None
        })
    }

    pub(crate) fn top_level_stems(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let mut index = 1;

        std::iter::from_fn(move || {
            while index < self.len() {
                let partner = self.raw_partner(index);
                if partner > index {
                    let stem = (index, partner);
                    index = partner;
                    return Some(stem);
                }
                index += 1;
            }
            None
        })
    }

    #[must_use]
    pub(crate) fn helix_loop_start(&self, mut base: usize) -> usize {
        while base < self.len()
            && self.raw_partner(base) > base + 1
            && self.raw_partner(base + 1) == self.raw_partner(base) - 1
        {
            base += 1;
        }
        base
    }

    #[must_use]
    pub(crate) fn loop_counts(&self, start: usize) -> LoopCounts {
        let mut counts = LoopCounts {
            unpaired: 0,
            stems: 0,
        };
        for element in self.loop_elements(start) {
            match element {
                LoopElement::Unpaired(_) => counts.unpaired += 1,
                LoopElement::Stem { .. } => counts.stems += 1,
            }
        }
        counts
    }

    fn pairs_0_based(&self) -> Vec<BasePair> {
        self.pairs()
            .map(|(opener, closer)| BasePair {
                i: opener - 1,
                j: closer - 1,
            })
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct InputModel {
    pub(crate) pair_table: PairTable,
    pub(crate) base_pairs: Vec<BasePair>,
}

pub(crate) fn validate(sequence: &str, structure: &str) -> Result<InputModel, LayoutError> {
    validate_lengths(sequence, structure)?;
    validate_sequence(sequence)?;

    let pair_table = parse_structure(structure, sequence.len())?;
    reject_pseudoknots(&pair_table)?;
    let base_pairs = pair_table.pairs_0_based();

    Ok(InputModel {
        pair_table,
        base_pairs,
    })
}

const fn validate_lengths(sequence: &str, structure: &str) -> Result<(), LayoutError> {
    let sequence_len = sequence.len();
    let structure_len = structure.len();

    if sequence_len == 0 || structure_len == 0 {
        return Err(LayoutError::EmptyInput);
    }

    if sequence_len != structure_len {
        return Err(LayoutError::LengthMismatch {
            sequence: sequence_len,
            structure: structure_len,
        });
    }

    Ok(())
}

fn validate_sequence(sequence: &str) -> Result<(), LayoutError> {
    if let Some((index, base)) = sequence
        .chars()
        .enumerate()
        .find(|&(_, base)| !is_valid_iupac_base(base))
    {
        return Err(LayoutError::InvalidSequenceChar { index, found: base });
    }
    Ok(())
}

const fn is_valid_iupac_base(base: char) -> bool {
    base.is_ascii() && is_iupac_nucleotide(base as u8)
}

#[derive(Debug, Clone, Copy)]
enum BracketKind {
    Round = 0,
    Square = 1,
    Curly = 2,
    Angle = 3,
}

impl BracketKind {
    const ALL: [Self; 4] = [Self::Round, Self::Square, Self::Curly, Self::Angle];

    const fn from_opening(bracket: char) -> Option<Self> {
        match bracket {
            '(' => Some(Self::Round),
            '[' => Some(Self::Square),
            '{' => Some(Self::Curly),
            '<' => Some(Self::Angle),
            _ => None,
        }
    }

    const fn from_closing(bracket: char) -> Option<Self> {
        match bracket {
            ')' => Some(Self::Round),
            ']' => Some(Self::Square),
            '}' => Some(Self::Curly),
            '>' => Some(Self::Angle),
            _ => None,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy)]
struct OpeningBracket {
    index: usize,
    bracket: char,
}

#[derive(Default)]
struct BracketStacks([Vec<OpeningBracket>; 4]);

impl BracketStacks {
    const fn stack_mut(&mut self, kind: BracketKind) -> &mut Vec<OpeningBracket> {
        &mut self.0[kind.index()]
    }

    fn first_unmatched(&self) -> Option<OpeningBracket> {
        BracketKind::ALL
            .into_iter()
            .filter_map(|kind| self.0[kind.index()].first().copied())
            .min_by_key(|opening| opening.index)
    }
}

fn parse_structure(structure: &str, sequence_len: usize) -> Result<PairTable, LayoutError> {
    let mut partners = vec![0; sequence_len + 1];
    partners[0] = sequence_len;
    let mut stacks = BracketStacks::default();

    for (index, bracket) in structure.chars().enumerate() {
        if bracket == '.' {
            continue;
        }

        if let Some(kind) = BracketKind::from_opening(bracket) {
            stacks
                .stack_mut(kind)
                .push(OpeningBracket { index, bracket });
            continue;
        }

        if let Some(kind) = BracketKind::from_closing(bracket) {
            let Some(opening) = stacks.stack_mut(kind).pop() else {
                return Err(LayoutError::UnmatchedClosingBracket { index, bracket });
            };
            let open_position = opening.index + 1;
            let close_position = index + 1;
            partners[open_position] = close_position;
            partners[close_position] = open_position;
            continue;
        }

        return Err(LayoutError::InvalidStructureChar {
            index,
            found: bracket,
        });
    }

    if let Some(opening) = stacks.first_unmatched() {
        return Err(LayoutError::UnmatchedOpeningBracket {
            index: opening.index,
            bracket: opening.bracket,
        });
    }

    Ok(PairTable::from_partners(partners))
}

fn reject_pseudoknots(pair_table: &PairTable) -> Result<(), LayoutError> {
    let mut open_pairs = Vec::new();

    for position in 1..=pair_table.len() {
        let partner = pair_table.raw_partner(position);
        if partner > position {
            open_pairs.push(position);
        } else if partner != 0 {
            let open_position = open_pairs
                .pop()
                .ok_or_else(|| LayoutError::InternalInvariant {
                    message: "pair table closes a pair that was never opened".to_owned(),
                })?;
            if open_position != partner {
                return Err(LayoutError::PseudoknotUnsupported {
                    first_i: partner - 1,
                    first_j: position - 1,
                    second_i: open_position - 1,
                    second_j: pair_table.raw_partner(open_position) - 1,
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod structure_tests {
    use super::{LoopElement, validate};
    use crate::drawing::error::LayoutError;

    fn error(sequence: &str, structure: &str) -> LayoutError {
        validate(sequence, structure).expect_err("input must be rejected")
    }

    #[test]
    fn lengths_must_agree_and_be_non_empty() {
        assert!(matches!(error("", ""), LayoutError::EmptyInput));
        assert!(matches!(error("A", ""), LayoutError::EmptyInput));
        assert!(matches!(error("", "."), LayoutError::EmptyInput));
        assert!(matches!(
            error("A", ".."),
            LayoutError::LengthMismatch {
                sequence: 1,
                structure: 2
            }
        ));
        assert!(matches!(
            error("AA", "."),
            LayoutError::LengthMismatch {
                sequence: 2,
                structure: 1
            }
        ));
    }

    #[test]
    fn sequences_accept_iupac_in_either_case_and_report_the_first_offender() {
        assert!(validate("AcGuUrYsWkMbDhVn", "................").is_ok());
        assert!(validate("ACGT", "....").is_ok(), "T must be accepted");

        // The index and character are what tell a user which residue to fix.
        assert!(matches!(
            error("ACXG", "...."),
            LayoutError::InvalidSequenceChar {
                index: 2,
                found: 'X'
            }
        ));
        // Lengths use bytes, so a multi-byte character fails the length check
        // first. Padding the structure to the byte length reaches the character
        // check and reports the non-ASCII base by character index.
        assert!(matches!(
            error("AÄG", "..."),
            LayoutError::LengthMismatch {
                sequence: 4,
                structure: 3
            }
        ));
        assert!(matches!(
            error("AÄG", "...."),
            LayoutError::InvalidSequenceChar {
                index: 1,
                found: 'Ä'
            }
        ));
    }

    #[test]
    fn every_bracket_type_pairs_only_with_its_own_kind() {
        for structure in ["()", "[]", "{}", "<>"] {
            assert!(
                validate("AA", structure).is_ok(),
                "{structure} must be accepted"
            );
        }
        assert!(validate("AAAAAAAA", "([{<>}])").is_ok());

        // A cross-type closer must not consume an opener of another kind.
        assert!(matches!(
            error("AA", "(]"),
            LayoutError::UnmatchedClosingBracket {
                index: 1,
                bracket: ']'
            }
        ));
    }

    #[test]
    fn unbalanced_and_invalid_structure_characters_are_rejected() {
        assert!(matches!(
            error("AA", ".)"),
            LayoutError::UnmatchedClosingBracket {
                index: 1,
                bracket: ')'
            }
        ));
        assert!(matches!(
            error("AA", "(."),
            LayoutError::UnmatchedOpeningBracket {
                index: 0,
                bracket: '('
            }
        ));
        // The earliest unmatched opener wins, across bracket kinds.
        assert!(matches!(
            error("AAAA", "[(]."),
            LayoutError::UnmatchedOpeningBracket {
                index: 1,
                bracket: '('
            }
        ));
        assert!(matches!(
            error("AA", ".x"),
            LayoutError::InvalidStructureChar {
                index: 1,
                found: 'x'
            }
        ));
    }

    #[test]
    fn crossing_pairs_are_reported_with_both_offending_pairs() {
        let LayoutError::PseudoknotUnsupported {
            first_i,
            first_j,
            second_i,
            second_j,
        } = error("AAAA", "([)]")
        else {
            panic!("crossing pairs must be reported as a pseudoknot");
        };
        // 0-based, so pair 1-3 crosses pair 2-4.
        assert_eq!((first_i, first_j), (0, 2));
        assert_eq!((second_i, second_j), (1, 3));

        // Nesting the same two bracket kinds is not a pseudoknot.
        assert!(validate("AAAA", "([])").is_ok());
    }

    #[test]
    fn noncanonical_pairs_are_accepted_because_they_are_drawing_data() {
        // The drawing layer accepts any pairing. The prediction layer checks
        // whether the pairing makes chemical sense.
        for sequence in ["AC", "AA", "CU"] {
            assert!(
                validate(sequence, "()").is_ok(),
                "{sequence} must be drawable"
            );
        }
    }

    #[test]
    fn pair_table_accessors_handle_degenerate_loops() {
        let flat = validate("AAAA", "....").unwrap().pair_table;
        assert_eq!(flat.len(), 4);
        assert!(!flat.has_pairs());
        assert_eq!(flat.pairs().count(), 0);
        assert_eq!(flat.top_level_stems().count(), 0);

        // A zero-length hairpin has no loop bases at all, which is what
        // defeated the helix walks described in KNOWN_ISSUES.
        let degenerate = validate("AA", "()").unwrap().pair_table;
        assert!(degenerate.has_pairs());
        assert_eq!(degenerate.loop_elements(1).count(), 0);
        let counts = degenerate.loop_counts(1);
        assert_eq!((counts.unpaired, counts.stems), (0, 0));

        // A one-base bulge has one stem and one unpaired base. `arcs.rs` uses
        // this shape to skip it as a loop start.
        let bulge = validate("AAAAAAA", "(.(.).)").unwrap().pair_table;
        let counts = bulge.loop_counts(1);
        assert_eq!((counts.unpaired, counts.stems), (2, 1));

        let multiloop = validate("AAAAAAAAAAAAAAA", "(.(.).(.).(.).)")
            .unwrap()
            .pair_table;
        let counts = multiloop.loop_counts(1);
        assert_eq!((counts.unpaired, counts.stems), (4, 3));
    }

    #[test]
    fn helix_loop_start_respects_zero_length_hairpins() {
        let cases = [
            ("()", 1),
            ("().", 1),
            ("(())", 2),
            ("((..))", 2),
            ("(((...)))", 3),
            ("((((.....))))", 4),
        ];
        for (structure, expected) in cases {
            let input = validate(&"A".repeat(structure.len()), structure).unwrap();
            assert_eq!(
                input.pair_table.helix_loop_start(1),
                expected,
                "unexpected loop start for {structure}"
            );
        }
    }

    #[test]
    fn iterators_preserve_structure_order() {
        let input = validate("AAAAAAAAAAAAAAA", "((..)(.))..(..)").unwrap();
        let pair_table = &input.pair_table;

        assert_eq!(
            pair_table.pairs().collect::<Vec<_>>(),
            [(1, 9), (2, 5), (6, 8), (12, 15)]
        );
        assert_eq!(
            pair_table.top_level_stems().collect::<Vec<_>>(),
            [(1, 9), (12, 15)]
        );
        assert!(matches!(
            pair_table.loop_elements(1).collect::<Vec<_>>().as_slice(),
            [
                LoopElement::Stem { start: 2, end: 5 },
                LoopElement::Stem { start: 6, end: 8 }
            ]
        ));
        assert!(matches!(
            pair_table.loop_elements(2).collect::<Vec<_>>().as_slice(),
            [LoopElement::Unpaired(3), LoopElement::Unpaired(4)]
        ));
    }
}
