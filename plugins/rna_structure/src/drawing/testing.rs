//! Shared fixtures and helpers for the drawing test suite.
//!
//! Everything the tests need is inlined here, so no test reads from disk.

use crate::drawing::validation::{PairTable, validate};

/// Bacterial `RNase` P RNA, type B. Kept verbatim because two regressions are
/// pinned to its nucleotide indices: the `RNAturtle` zero-length pair at 305-306
/// and the `RNApuzzler` bulge at 384-386. Its crowded multiloops also give the
/// `RNApuzzler` intersection tests real overlaps to resolve.
pub(crate) const RNASE_P2_SEQUENCE: &str = concat!(
    "GTTCTTAACGTTCGGGTAATCGCTGCAGATCTTGAATCTGTAGAGGAAAGTCCATGCTCGCACGGTGCTGAG",
    "ATGCCCGTAGTGTTCGTGCCTAGCGAAGTCATAAGCTAGGGCAGTCTTTAGAGGCTGACGGCAGGAAAAAAG",
    "CCTACGTCTTCGGATATGGCTGAGTATCCTTGAAAGTGCCACAGTGACGAAGTCTCACTAGAAATGGTGAGA",
    "GTGGAACGCGGTAAACCCCTCGAGCGAGAAACCCAAATTTTGGTAGGGGAACCTTCTTAACGGAATTCAACG",
    "GAGAGAAGGACAGAATGCTTTCTGTAGATAGATGATTGCCGCCTGAGTACGAGGTGATGAGCCGTTTGCAGT",
    "ACGATGGAACAAAACATGGCTTACAGAACGTTAGACCACTT",
);

pub(crate) const RNASE_P2_STRUCTURE: &str = concat!(
    "((..(((((((((((((((((.((((((((.....)))))))).............(((((((((.((....",
    "..)))))).....(((.(((((((..........))))))((((((......))))).)((.(((.....((",
    "(((..((((..))))).))))......))).....(((((............((((((((....))))))))",
    ".........)))..))))).))))))))...(((.(.......).)))...(((((((..((........))",
    "..)))))))((((((.().))))))........))))))).((...((((..(((.....))).......))",
    "))...))................).)))))))))...))..",
);

/// Validates `structure` against an all-adenine sequence of the same length.
pub(crate) fn pair_table(structure: &str) -> PairTable {
    validate(&"A".repeat(structure.len()), structure)
        .expect("test structure must validate")
        .pair_table
}

/// Every well-formed dot-bracket string over `.` and one bracket type with
/// length 2 to `max_length` (the Motzkin words): 537 structures in total.
pub(crate) fn balanced_structures_up_to_length(max_length: usize) -> Vec<String> {
    fn walk(prefix: &mut Vec<char>, open: usize, remaining: usize, out: &mut Vec<String>) {
        if remaining == 0 {
            if open == 0 {
                out.push(prefix.iter().collect());
            }
            return;
        }

        prefix.push('.');
        walk(prefix, open, remaining - 1, out);
        prefix.pop();

        prefix.push('(');
        walk(prefix, open + 1, remaining - 1, out);
        prefix.pop();

        if open > 0 {
            prefix.push(')');
            walk(prefix, open - 1, remaining - 1, out);
            prefix.pop();
        }
    }

    let mut structures = Vec::new();
    for length in 2..=max_length {
        walk(&mut Vec::new(), 0, length, &mut structures);
    }
    structures
}

/// A representative spread of structural motifs: hairpins, bulges, internal
/// loops, stacked helices, multiloops, and zero-length hairpins.
pub(crate) const MOTIF_STRUCTURES: [&str; 12] = [
    "(((...)))",
    "((((.....))))",
    "(((.(...).)))",
    "((.((...)).))",
    "((...((...))...))",
    "(.(...).(...).)",
    "(((...)(...)))",
    "((((...)).((...))))",
    "()",
    "(((...()...)).)",
    ".((...))..((...)).",
    "((((((...))))..((..((...))..))..((...))...))",
];

pub(crate) fn all_numbers_are_finite(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(number) => number.as_f64().is_none_or(f64::is_finite),
        serde_json::Value::Array(values) => values.iter().all(all_numbers_are_finite),
        serde_json::Value::Object(object) => object.values().all(all_numbers_are_finite),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{RNASE_P2_SEQUENCE, RNASE_P2_STRUCTURE};
    use crate::drawing::validation::validate;

    #[test]
    fn rnase_p2_fixture_is_a_valid_401_nucleotide_structure() {
        // These constants are hand-wrapped. A missing or duplicated chunk would
        // shift the indices used by the regression tests.
        assert_eq!(RNASE_P2_SEQUENCE.len(), 401);
        assert_eq!(RNASE_P2_STRUCTURE.len(), 401);

        let input = validate(RNASE_P2_SEQUENCE, RNASE_P2_STRUCTURE)
            .expect("the RNase P2 fixture must validate");
        assert_eq!(input.base_pairs.len(), 110);
        // The zero-length hairpin at 305-306 triggers the RNAturtle and
        // RNApuzzler regressions.
        assert_eq!(input.pair_table.raw_partner(305), 306);
    }
}
