mod config;
mod constraints;
mod engine;
mod error;
mod model;
mod output;
mod sequence;

use constraints::Constraints;
use error::{FoldError, PredictionError};
use model::{ContraFoldModel, ViennaModel};

/// Smallest `right - left` of a pair that encloses at least three bases.
const MIN_PAIR_SPAN: usize = 4;

/// Scoring model used by the folding engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// Vienna `RNAfold` thermodynamic parameters.
    ViennaRnafold,
    /// `CONTRAfold` log-linear parameters.
    ContraFold,
}

/// Treatment of dangling bases adjacent to helices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangleModel {
    /// Ignore dangling-end energies.
    None,
    /// Include both adjacent dangling bases when available.
    Both,
}

/// Sequence topology for secondary structure prediction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceTopology {
    /// Standard linear sequence with open free ends.
    Linear,
    /// Covalently closed circular sequence.
    Circular,
}

/// Options for one optimal fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldOptions {
    /// Scoring model. The default is [`Model::ContraFold`].
    pub model: Model,
    /// Maximum states retained per beam. Zero disables pruning.
    pub beam_size: usize,
    /// Permit pairs separated by fewer than three unpaired bases.
    pub allow_sharp_turns: bool,
    /// Vienna dangling-end treatment. `CONTRAfold` ignores this setting.
    pub dangles: DangleModel,
    /// Topology of the RNA sequence.
    pub topology: SequenceTopology,
}

impl Default for FoldOptions {
    fn default() -> Self {
        Self {
            model: Model::ContraFold,
            beam_size: 100,
            allow_sharp_turns: false,
            dangles: DangleModel::Both,
            topology: SequenceTopology::Linear,
        }
    }
}

/// Model-specific score for a predicted structure.
#[derive(Debug, Clone, PartialEq)]
pub enum PredictionScore {
    /// Vienna free energy in kcal/mol.
    ViennaRnafold(f64),
    /// `CONTRAfold` log-linear score.
    ContraFold(f64),
}

/// Optimal secondary-structure prediction.
#[derive(Debug, Clone, PartialEq)]
pub struct Prediction {
    /// Dot-bracket structure with one byte per input base.
    pub structure: String,
    /// Score assigned to `structure` by the selected model. `ViennaRnafold` uses
    /// kcal/mol. `ContraFold` uses a log-linear score.
    pub score: PredictionScore,
}

/// Predicts the optimal secondary structure for one RNA or DNA sequence.
///
/// A beam size of zero disables pruning. Constraints use `?`, `.`, `(`, and `)`.
///
/// # Errors
///
/// Returns [`FoldError`] for invalid input, malformed constraints, or constraints
/// that cannot be satisfied.
pub(crate) fn fold(
    sequence: &[u8],
    constraints: Option<&[u8]>,
    options: &FoldOptions,
) -> Result<Prediction, FoldError> {
    let bases = sequence::normalize(sequence)?;
    let constraints = constraints
        .map(|input| Constraints::parse(input, &bases))
        .transpose()?;

    if options.topology == SequenceTopology::Circular && options.model != Model::ViennaRnafold {
        return Err(FoldError::CircularFoldingRequiresViennaRnafold);
    }

    // Vienna assigns no finite energy to hairpins shorter than three bases, so a
    // forced pair that encloses fewer bases cannot appear in any scored structure.
    if options.model == Model::ViennaRnafold
        && let Some((left, right)) = constraints.as_ref().and_then(Constraints::first_sharp_pair)
    {
        return Err(FoldError::SharpConstrainedPair { left, right });
    }

    match options.model {
        Model::ViennaRnafold => {
            let filled = engine::fill_charts::<ViennaModel>(&bases, constraints.as_ref(), options)?;
            let (structure, score) = match options.topology {
                SequenceTopology::Linear => engine::run_linear_from_filled(&filled, bases.len())?,
                SequenceTopology::Circular => {
                    engine::run_circular_vienna(filled, constraints.as_ref(), bases.len())?
                }
            };
            Ok(Prediction {
                structure,
                score: PredictionScore::ViennaRnafold(-(f64::from(score)) / 100.0),
            })
        }
        Model::ContraFold => {
            let (structure, score) =
                engine::run_linear::<ContraFoldModel>(&bases, constraints.as_ref(), options)?;
            Ok(Prediction {
                structure,
                score: PredictionScore::ContraFold(score),
            })
        }
    }
}

pub(crate) fn predict(sequence: &[u8], config_bytes: &[u8]) -> Result<Vec<u8>, PredictionError> {
    let request = config::parse(config_bytes).map_err(PredictionError::Config)?;
    let prediction = fold(sequence, request.constraints.as_deref(), &request.options)
        .map_err(PredictionError::Folding)?;
    output::serialize(prediction).map_err(PredictionError::Serialization)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_precedes_topology_checks() {
        let options = FoldOptions {
            model: Model::ContraFold,
            topology: SequenceTopology::Circular,
            ..FoldOptions::default()
        };
        assert_eq!(fold(b"", None, &options), Err(FoldError::EmptySequence));
        assert_eq!(
            fold(b"AC GU", None, &options),
            Err(FoldError::InvalidSequenceByte {
                index: 2,
                byte: b' '
            })
        );
        assert_eq!(
            fold(b"ACGU", Some(b"???"), &options),
            Err(FoldError::ConstraintLengthMismatch {
                sequence_len: 4,
                constraint_len: 3
            })
        );
        assert_eq!(
            fold(b"ACGU", Some(b"??x?"), &options),
            Err(FoldError::InvalidConstraintByte {
                index: 2,
                byte: b'x'
            })
        );
        assert_eq!(
            fold(b"ACGU", None, &options),
            Err(FoldError::CircularFoldingRequiresViennaRnafold)
        );
    }

    #[test]
    fn sharp_and_unsatisfiable_constraints_are_reported() {
        // `constraints::tests` owns the parse errors. These need the folding
        // model to decide.
        let options = FoldOptions::default();
        assert_eq!(
            fold(b"AU", Some(b"()"), &options)
                .expect("forced pairs allow sharp turns")
                .structure,
            "()"
        );

        let vienna = FoldOptions {
            model: Model::ViennaRnafold,
            ..FoldOptions::default()
        };
        assert_eq!(
            fold(b"GAAC", Some(b"(..)"), &vienna),
            Err(FoldError::SharpConstrainedPair { left: 0, right: 3 })
        );
        assert_eq!(
            fold(b"GGGGCCCC", Some(b"(((())))"), &vienna),
            Err(FoldError::SharpConstrainedPair { left: 2, right: 5 })
        );
        assert_eq!(
            fold(
                b"GAAAAC",
                Some(b"(....)"),
                &FoldOptions {
                    topology: SequenceTopology::Circular,
                    ..vienna
                }
            ),
            Err(FoldError::NoValidStructure)
        );

        let sequence = format!("A{}A{}UU", "C".repeat(31), "C".repeat(6));
        let constraints = format!("({}({}))", ".".repeat(31), ".".repeat(6));
        assert_eq!(
            fold(sequence.as_bytes(), Some(constraints.as_bytes()), &options),
            Err(FoldError::NoValidStructure)
        );
    }

    #[test]
    fn contrafold_known_examples_remain_stable() {
        let examples = [
            (
                b"UGAGUUCUCGAUCUCUAAAAUCG".as_slice(),
                ".......................",
                -0.22,
            ),
            (
                b"AAAACGGUCCUUAUCAGGACCAAACA".as_slice(),
                ".....((((((....)))))).....",
                4.91,
            ),
            (
                b"UCGGCCACAAACACACAAUCUACUGUUGGUCGA".as_slice(),
                "(((((((...................)))))))",
                0.99,
            ),
        ];

        for (sequence, structure, score) in examples {
            let prediction = fold(sequence, None, &FoldOptions::default()).unwrap();
            assert_eq!(prediction.structure, structure);
            let PredictionScore::ContraFold(actual_score) = prediction.score else {
                panic!("wrong score model");
            };
            assert!((actual_score - score).abs() < 0.005);
        }
    }

    #[test]
    fn vienna_known_examples_remain_stable() {
        let options = FoldOptions {
            model: Model::ViennaRnafold,
            ..FoldOptions::default()
        };
        let examples = [
            (
                b"UGAGUUCUCGAUCUCUAAAAUCG".as_slice(),
                ".(((........)))........",
                -1.80,
            ),
            (
                b"AAAACGGUCCUUAUCAGGACCAAACA".as_slice(),
                ".....((((((....)))))).....",
                -9.30,
            ),
            (
                b"AUUCUUGCUUCAACAGUGUUUGAACGGAAU".as_slice(),
                "(((((...(((((......))))).)))))",
                -6.80,
            ),
            (
                b"UCGGCCACAAACACACAAUCUACUGUUGGUCGA".as_slice(),
                "(((((((((..............))).))))))",
                -7.80,
            ),
            (
                b"GUUUUUAUCUUACACACGCUUGUGUAAGAUAGUUA".as_slice(),
                "....((((((((((((....))))))))))))...",
                -13.00,
            ),
        ];

        for (sequence, structure, score) in examples {
            let prediction = fold(sequence, None, &options).unwrap();
            assert_eq!(prediction.structure, structure);
            assert_eq!(prediction.score, PredictionScore::ViennaRnafold(score));
        }
    }

    #[test]
    fn constrained_predictions_remain_stable() {
        let sequence = b"AACUCCGCCAGGCCUGGAAGGGAGCAACGGUAGUGACACUCUCUGUGUGCGUAGGUUGCCUAGCUACCAUUU";
        let constraints =
            b"??(???(??????)?(????????)???(??????(???????)?)???????????)??.???????????";
        let prediction = fold(sequence, Some(constraints), &FoldOptions::default()).unwrap();
        assert_eq!(
            prediction.structure,
            "..(.(((......)((........))(((......(.......).))).....))..).............."
        );
        let PredictionScore::ContraFold(score) = prediction.score else {
            panic!("wrong score model");
        };
        assert!((score + 27.33).abs() < 0.005);

        let vienna = fold(
            sequence,
            Some(constraints),
            &FoldOptions {
                model: Model::ViennaRnafold,
                ..FoldOptions::default()
            },
        )
        .unwrap();
        assert_eq!(
            vienna.structure,
            "..(.(((......)((........))(((......(.......).))).....))..).............."
        );
        assert_eq!(vienna.score, PredictionScore::ViennaRnafold(13.40));
    }

    const OPTION_SEQUENCE: &[u8] =
        b"GGGCUCGUAGAUCAGCGGUAGAUCGCUUCCUUCGCAAGGAAGCCCUGGGUUCAAAUCCCAGCGAGUCCACCA";

    #[test]
    fn beam_size_changes_the_vienna_optimum() {
        let beam_twenty = FoldOptions {
            model: Model::ViennaRnafold,
            beam_size: 20,
            ..FoldOptions::default()
        };
        let prediction = fold(OPTION_SEQUENCE, None, &beam_twenty).unwrap();
        assert_eq!(prediction.score, PredictionScore::ViennaRnafold(-31.50));

        let beam_one = FoldOptions {
            model: Model::ViennaRnafold,
            beam_size: 1,
            ..FoldOptions::default()
        };
        let beam_one_prediction = fold(OPTION_SEQUENCE, None, &beam_one).unwrap();
        assert_eq!(
            beam_one_prediction.structure,
            ".(((((((......))))......)))..............((((((...........))).).))......"
        );
        assert_eq!(
            beam_one_prediction.score,
            PredictionScore::ViennaRnafold(-3.10)
        );

        let unbounded = FoldOptions {
            model: Model::ViennaRnafold,
            beam_size: 0,
            ..FoldOptions::default()
        };
        assert_eq!(
            fold(OPTION_SEQUENCE, None, &unbounded).unwrap().score,
            PredictionScore::ViennaRnafold(-31.50)
        );
    }

    #[test]
    fn dangle_model_changes_the_vienna_score_but_not_this_structure() {
        let both = FoldOptions {
            model: Model::ViennaRnafold,
            ..FoldOptions::default()
        };
        let none = FoldOptions {
            dangles: DangleModel::None,
            ..both
        };
        let with_dangles = fold(OPTION_SEQUENCE, None, &both).unwrap();
        let without_dangles = fold(OPTION_SEQUENCE, None, &none).unwrap();

        assert_eq!(with_dangles.score, PredictionScore::ViennaRnafold(-31.50));
        assert_eq!(
            without_dangles.score,
            PredictionScore::ViennaRnafold(-25.50)
        );
        assert_eq!(with_dangles.structure, without_dangles.structure);
    }

    #[test]
    fn dangle_model_does_not_change_the_contrafold_prediction() {
        let contra_default = fold(OPTION_SEQUENCE, None, &FoldOptions::default()).unwrap();
        let contra_no_dangles = fold(
            OPTION_SEQUENCE,
            None,
            &FoldOptions {
                dangles: DangleModel::None,
                ..FoldOptions::default()
            },
        )
        .unwrap();
        assert_eq!(contra_no_dangles, contra_default);
    }

    #[test]
    fn long_unpairable_gaps_do_not_overflow_trace_padding() {
        let mut sequence =
            b"AACUCCGCCAGGCCUGGAAGGGAGCAACGGUAGUGACACUCUCUGUGUGCGUAGGUUGCCUAGCUACCAUUU".to_vec();
        sequence.extend(std::iter::repeat_n(b'N', 300));
        sequence.push(b'U');

        let prediction = fold(&sequence, None, &FoldOptions::default()).unwrap();
        assert_eq!(prediction.structure.len(), sequence.len());
        let PredictionScore::ContraFold(score) = prediction.score else {
            panic!("wrong score model");
        };
        assert!((score + 2.47).abs() < 0.005);
    }

    #[test]
    fn circular_vienna_cases_remain_stable() {
        let options = FoldOptions {
            model: Model::ViennaRnafold,
            topology: SequenceTopology::Circular,
            beam_size: 0,
            ..FoldOptions::default()
        };
        let cases = [
            (b"A".as_slice(), ".", 2.70),
            (b"GGGAAACCC".as_slice(), ".........", 4.73),
            (
                b"AUCGAUCGAUCGAUCGAUCG".as_slice(),
                ".((((((....))))))...",
                -1.10,
            ),
            (
                b"GGGAAACCCGGGAAACCC".as_slice(),
                "(((...)))(((...)))",
                -4.80,
            ),
            (
                b"AGGGGGAAAAAAAACCCCCA".as_slice(),
                "..((((........))))..",
                -2.20,
            ),
        ];
        for (sequence, structure, score) in cases {
            let prediction = fold(sequence, None, &options).unwrap();
            assert_eq!(prediction.structure, structure);
            assert_eq!(prediction.score, PredictionScore::ViennaRnafold(score));
        }

        // Forcing the second hairpin unpaired removes it and keeps the first.
        let forced_unpaired =
            fold(b"GGGAAACCCGGGAAACCC", Some(b"?????????........."), &options).unwrap();
        assert_eq!(forced_unpaired.structure, "(((...))).........");
        assert_eq!(forced_unpaired.score, PredictionScore::ViennaRnafold(3.70));

        let short_loop_options = FoldOptions {
            allow_sharp_turns: true,
            ..options
        };
        assert_eq!(
            fold(b"GGGAAACCC", Some(b"(((...)))"), &short_loop_options),
            Err(FoldError::NoValidStructure)
        );

        let degree_two_cases = [
            (
                b"AGGGGGAAAACCCCCAGGGGGAAAACCCCC".as_slice(),
                ".(((((....))))).(((((....)))))",
                -16.50,
            ),
            (
                b"AAGGGGGAAAACCCCCAAGGGGGAAAACCCCC".as_slice(),
                "..(((((....)))))..(((((....)))))",
                -16.20,
            ),
            (
                b"GGGGGAAAACCCCCAAAGGGGGAAAACCCCC".as_slice(),
                "(((((....)))))...(((((....)))))",
                -14.20,
            ),
        ];
        for (sequence, structure, score) in degree_two_cases {
            let prediction = fold(sequence, None, &options).unwrap();
            assert_eq!(prediction.structure, structure);
            assert_eq!(prediction.score, PredictionScore::ViennaRnafold(score));
        }
    }

    #[test]
    fn circular_multiloop_dangles_change_only_the_score() {
        let sequence = b"GCGCAAAAGCGCCGCGAAAACGCGGGCCAAAAGGCC";
        for (dangles, score) in [(DangleModel::Both, -12.30), (DangleModel::None, -8.10)] {
            let options = FoldOptions {
                model: Model::ViennaRnafold,
                topology: SequenceTopology::Circular,
                dangles,
                beam_size: 0,
                ..FoldOptions::default()
            };
            let prediction = fold(sequence, None, &options).unwrap();
            assert_eq!(prediction.structure, "((((....))))((((....))))((((....))))");
            assert_eq!(prediction.score, PredictionScore::ViennaRnafold(score));
        }
    }

    /// Every bracket in `structure` must close, and every resulting pair must be
    /// one of the six canonical Watson-Crick or wobble pairings.
    fn assert_balanced_and_canonical(sequence: &[u8], structure: &str) {
        let mut openings = Vec::new();
        for (index, symbol) in structure.bytes().enumerate() {
            match symbol {
                b'.' => {}
                b'(' => openings.push(index),
                b')' => {
                    let left = openings
                        .pop()
                        .unwrap_or_else(|| panic!("unbalanced structure {structure}"));
                    let left = sequence[left].to_ascii_uppercase();
                    let right = sequence[index].to_ascii_uppercase();
                    assert!(
                        matches!(
                            (left, right),
                            (b'A' | b'G', b'U') | (b'U', b'A' | b'G') | (b'C', b'G') | (b'G', b'C')
                        ),
                        "noncanonical pair {}-{} in {structure}",
                        left as char,
                        right as char
                    );
                }
                symbol => panic!("unexpected dot-bracket symbol {symbol:?}"),
            }
        }
        assert!(openings.is_empty(), "unclosed brackets in {structure}");
    }

    #[test]
    fn predictions_are_balanced_and_canonical_under_every_option() {
        // Pseudo-random sequences over every model, topology, sharp-turn and
        // dangle setting. The fixed LCG keeps the sweep deterministic.
        let base = FoldOptions::default();
        let vienna = FoldOptions {
            model: Model::ViennaRnafold,
            ..base
        };
        let option_sets = [
            base,
            FoldOptions {
                allow_sharp_turns: true,
                ..base
            },
            vienna,
            FoldOptions {
                allow_sharp_turns: true,
                dangles: DangleModel::None,
                ..vienna
            },
            FoldOptions {
                topology: SequenceTopology::Circular,
                ..vienna
            },
        ];

        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from(state >> 33).expect("a 31-bit value fits in usize")
        };
        let mut paired_predictions = 0;
        for _ in 0..50 {
            let length = 20 + next() % 41;
            let sequence: Vec<u8> = (0..length).map(|_| b"ACGUN"[next() % 5]).collect();
            for options in &option_sets {
                let prediction = fold(&sequence, None, options).unwrap();
                assert_eq!(prediction.structure.len(), sequence.len());
                assert_balanced_and_canonical(&sequence, &prediction.structure);
                if prediction.structure.contains('(') {
                    paired_predictions += 1;
                }
            }
        }
        // Guard against a sweep that passes because nothing ever pairs.
        assert!(
            paired_predictions > 100,
            "only {paired_predictions} predictions contained a pair"
        );
    }

    #[test]
    fn unknown_bases_never_pair() {
        let cases = [
            // Nothing is known, so nothing can pair, even with room for a helix.
            (b"NNNNNNNNNNNNNNNN".as_slice(), "................"),
            // An unknown base inside the hairpin loop must not stop the
            // surrounding helix from forming.
            (b"GGGGGNAAACCCCC".as_slice(), "(((((....)))))"),
            // Unknown bases flanking a helix stay unpaired while the known
            // bases between them pair normally.
            (b"NGGGGGAAAACCCCCN".as_slice(), ".(((((....)))))."),
        ];
        for (sequence, expected_structure) in cases {
            let prediction = fold(sequence, None, &FoldOptions::default()).unwrap();
            assert_eq!(
                prediction.structure,
                expected_structure,
                "sequence {}",
                String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn contrafold_beam_size_changes_the_optimum() {
        let contra_sequence =
            b"UGAGUUCUCGAUCUCUAAAAUCGUAGGGCUCGUAGAUCAGCGGUAGAUCGCUUCCUUCGCAAGGAAGCCC";
        let default = fold(contra_sequence, None, &FoldOptions::default()).unwrap();
        // Beam size 1 must differ from the default on this sequence.
        let beam_one = FoldOptions {
            beam_size: 1,
            ..FoldOptions::default()
        };
        let small = fold(contra_sequence, None, &beam_one).unwrap();
        assert_ne!(
            default.structure, small.structure,
            "beam 1 must change ContraFold optimum"
        );
        // Beam size 0 disables pruning and must recover the default.
        let unlimited = FoldOptions {
            beam_size: 0,
            ..FoldOptions::default()
        };
        let unbounded = fold(contra_sequence, None, &unlimited).unwrap();
        assert_eq!(default, unbounded);
    }

    #[test]
    fn sharp_turns_change_the_contrafold_optimum() {
        // Vienna gives hairpins shorter than three bases no finite energy, so
        // only ContraFold can use a sharp turn when it is allowed.
        let sequence = b"GAUGUCAAACCCCGGGGGGA";
        let without = fold(sequence, None, &FoldOptions::default()).unwrap();
        let with = fold(
            sequence,
            None,
            &FoldOptions {
                allow_sharp_turns: true,
                ..FoldOptions::default()
            },
        )
        .unwrap();
        assert_eq!(without.structure, ".........(((....))).");
        assert_eq!(with.structure, ".........(((())))...");
    }

    #[test]
    fn multiloop_branch_near_three_prime_end_is_reachable() {
        // The second branch `()` closes two bases before the 3' end, so the
        // multiloop is only reachable if M states are kept that close to it.
        for allow_sharp_turns in [false, true] {
            let options = FoldOptions {
                allow_sharp_turns,
                ..FoldOptions::default()
            };
            let prediction = fold(b"GGGCCGCC", Some(b"((())())"), &options)
                .expect("forced multiloop must be foldable");
            assert_eq!(prediction.structure, "((())())");
        }
    }

    #[test]
    fn constraints_pin_the_structure_they_describe() {
        // Each case differs from the fold the free positions would take on
        // their own, so an ignored constraint fails the comparison.
        let cases = [
            // Free, this sequence folds to `.....((((((....)))))).....`.
            // Forcing base 5 unpaired shortens the helix, and the other `?`
            // positions still fold.
            (
                b"AAAACGGUCCUUAUCAGGACCAAACA".as_slice(),
                b"?????.????????????????????".as_slice(),
                "......(((((....)))))......",
            ),
            // With only the brackets forced, the terminal G and C pair onto the
            // helix. `.` keeps them apart.
            (
                b"GGGAAACCC".as_slice(),
                b".((...)).".as_slice(),
                ".((...)).",
            ),
        ];

        for (sequence, constraints, expected) in cases {
            let prediction = fold(sequence, Some(constraints), &FoldOptions::default())
                .unwrap_or_else(|error| {
                    panic!(
                        "{} must fold: {error:?}",
                        String::from_utf8_lossy(constraints)
                    )
                });
            assert_eq!(
                prediction.structure,
                expected,
                "constraints {}",
                String::from_utf8_lossy(constraints)
            );
        }
    }

    #[test]
    fn constrained_loop_shapes_are_reconstructed_by_traceback() {
        let cases = [
            (b"GGGAAACCC".as_slice(), b"(((...)))".as_slice()),
            (b"GAGAAAACAC".as_slice(), b"(.(....).)".as_slice()),
            (
                b"GGAAAACAAAGAAAACC".as_slice(),
                b"((....)...(....))".as_slice(),
            ),
        ];

        for (sequence, constraints) in cases {
            let prediction = fold(sequence, Some(constraints), &FoldOptions::default())
                .unwrap_or_else(|error| {
                    panic!(
                        "{} must be reconstructed: {error:?}",
                        String::from_utf8_lossy(constraints)
                    )
                });
            assert_eq!(prediction.structure.as_bytes(), constraints);
        }
    }
}
