//! Core alignment data structures and DP helpers.

use crate::scoring::{ScoringConfig, SubstitutionScorer};

/// Arrow directions stored as a 3-bit bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Arrows(u8);

impl Arrows {
    pub(crate) fn new() -> Self {
        Self(0)
    }

    pub(crate) fn bits(&self) -> u8 {
        self.0
    }

    pub(crate) fn has_diagonal(&self) -> bool {
        (self.0 & 1) != 0
    }

    pub(crate) fn has_up(&self) -> bool {
        (self.0 & 2) != 0
    }

    pub(crate) fn has_left(&self) -> bool {
        (self.0 & 4) != 0
    }
}

impl Arrows {
    pub(crate) const DIAGONAL: u8 = 1;
    pub(crate) const UP: u8 = 2;
    pub(crate) const LEFT: u8 = 4;

    #[cfg(test)]
    pub(crate) fn set_diagonal(&mut self) {
        self.0 |= Self::DIAGONAL;
    }

    pub(crate) fn set_up(&mut self) {
        self.0 |= Self::UP;
    }

    pub(crate) fn set_left(&mut self) {
        self.0 |= Self::LEFT;
    }
}

/// A cell in the dynamic programming matrix.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cell {
    pub(crate) score: i32,
    pub(crate) arrows: Arrows,
}

impl Cell {
    pub(crate) fn new(score: i32) -> Self {
        Self {
            score,
            arrows: Arrows::new(),
        }
    }

    pub(crate) fn with_arrows(score: i32, arrows: Arrows) -> Self {
        Self { score, arrows }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            score: i32::MIN,
            arrows: Arrows::new(),
        }
    }
}

/// The dynamic programming matrix.
#[derive(Debug, Clone)]
pub(crate) struct DPMatrix {
    pub(crate) rows: usize,
    pub(crate) cols: usize,
    pub(crate) cells: Vec<Cell>,
}

impl DPMatrix {
    pub(crate) fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            cells: vec![Cell::default(); rows * cols],
        }
    }

    #[inline]
    pub(crate) fn get(&self, i: usize, j: usize) -> &Cell {
        &self.cells[i * self.cols + j]
    }

    #[inline]
    pub(crate) fn set(&mut self, i: usize, j: usize, cell: Cell) {
        self.cells[i * self.cols + j] = cell;
    }
}

/// A step in the traceback path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TracebackStep {
    pub(crate) i: usize,
    pub(crate) j: usize,
}

/// A complete traceback path.
#[derive(Debug, Clone)]
pub(crate) struct TracebackPath {
    pub(crate) steps: Vec<TracebackStep>,
}

impl TracebackPath {
    pub(crate) fn push(&mut self, i: usize, j: usize) {
        self.steps.push(TracebackStep { i, j });
    }
}

/// A pair of aligned sequences.
#[derive(Debug, Clone)]
pub(crate) struct AlignedPair {
    pub(crate) seq1_aligned: String,
    pub(crate) seq2_aligned: String,
}

/// The complete result of an alignment operation.
#[derive(Debug, Clone)]
pub(crate) struct AlignmentResult {
    pub(crate) matrix: DPMatrix,
    pub(crate) traceback_paths: Vec<TracebackPath>,
    pub(crate) alignments: Vec<AlignedPair>,
    pub(crate) final_score: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct FillResult {
    pub max_score: i32,
    pub max_positions: Vec<(usize, usize)>,
}

pub(crate) fn fill_matrix_linear(
    matrix: &mut DPMatrix,
    seq1: &[u8],
    seq2: &[u8],
    scoring: &ScoringConfig,
    local: bool,
) -> FillResult {
    match &scoring.scorer {
        SubstitutionScorer::Simple {
            match_score,
            mismatch_score,
        } => fill_matrix_linear_simple(
            matrix,
            seq1,
            seq2,
            *match_score,
            *mismatch_score,
            scoring.gap_open,
            local,
        ),
        SubstitutionScorer::Matrix(matrix_scorer) => fill_matrix_linear_matrix(
            matrix,
            seq1,
            seq2,
            matrix_scorer.lookup_map(),
            matrix_scorer.scores(),
            matrix_scorer.score_dimension(),
            scoring.gap_open,
            local,
        ),
    }
}

fn fill_matrix_linear_simple(
    matrix: &mut DPMatrix,
    seq1: &[u8],
    seq2: &[u8],
    match_score: i32,
    mismatch_score: i32,
    gap: i32,
    local: bool,
) -> FillResult {
    let n = seq1.len();
    let m = seq2.len();
    let cols = matrix.cols;
    let seq2_upper_storage = uppercase_ascii_if_needed(seq2);
    let seq2_upper = seq2_upper_storage.as_deref().unwrap_or(seq2);

    if local {
        let mut max_score = 0;
        let mut max_positions = Vec::new();

        for i in 1..=n {
            let row_offset = i * cols;
            let prev_row_offset = (i - 1) * cols;
            let (prior_rows, current_and_after) = matrix.cells.split_at_mut(row_offset);
            let prev_row = &prior_rows[prev_row_offset..row_offset];
            let row = &mut current_and_after[..cols];
            let seq1_char = seq1[i - 1].to_ascii_uppercase();

            for j in 1..=m {
                let seq2_char = seq2_upper[j - 1];
                let substitution = if seq1_char == seq2_char {
                    match_score
                } else {
                    mismatch_score
                };
                let diag_score = prev_row[j - 1].score.saturating_add(substitution);
                let up_score = prev_row[j].score.saturating_add(gap);
                let left_score = row[j - 1].score.saturating_add(gap);

                let max_candidate = diag_score.max(up_score).max(left_score);
                let cell_score = max_candidate.max(0);

                let mut arrows = 0;
                if cell_score > 0 {
                    if diag_score == cell_score {
                        arrows |= Arrows::DIAGONAL;
                    }
                    if up_score == cell_score {
                        arrows |= Arrows::UP;
                    }
                    if left_score == cell_score {
                        arrows |= Arrows::LEFT;
                    }
                }

                row[j] = Cell::with_arrows(cell_score, Arrows(arrows));

                if cell_score > max_score {
                    max_score = cell_score;
                    max_positions.clear();
                    if cell_score > 0 {
                        max_positions.push((i, j));
                    }
                } else if cell_score == max_score && cell_score > 0 {
                    max_positions.push((i, j));
                }
            }
        }

        FillResult {
            max_score,
            max_positions,
        }
    } else {
        for i in 1..=n {
            let row_offset = i * cols;
            let prev_row_offset = (i - 1) * cols;
            let (prior_rows, current_and_after) = matrix.cells.split_at_mut(row_offset);
            let prev_row = &prior_rows[prev_row_offset..row_offset];
            let row = &mut current_and_after[..cols];
            let seq1_char = seq1[i - 1].to_ascii_uppercase();

            for j in 1..=m {
                let seq2_char = seq2_upper[j - 1];
                let substitution = if seq1_char == seq2_char {
                    match_score
                } else {
                    mismatch_score
                };
                let diag_score = prev_row[j - 1].score.saturating_add(substitution);
                let up_score = prev_row[j].score.saturating_add(gap);
                let left_score = row[j - 1].score.saturating_add(gap);

                let max_score = diag_score.max(up_score).max(left_score);

                let mut arrows = 0;
                if diag_score == max_score {
                    arrows |= Arrows::DIAGONAL;
                }
                if up_score == max_score {
                    arrows |= Arrows::UP;
                }
                if left_score == max_score {
                    arrows |= Arrows::LEFT;
                }

                row[j] = Cell::with_arrows(max_score, Arrows(arrows));
            }
        }

        FillResult {
            max_score: matrix.cells[n * cols + m].score,
            max_positions: Vec::new(),
        }
    }
}

fn fill_matrix_linear_matrix(
    matrix: &mut DPMatrix,
    seq1: &[u8],
    seq2: &[u8],
    lookup_map: &[Option<u8>; 256],
    score_table: &[i32],
    score_dimension: usize,
    gap: i32,
    local: bool,
) -> FillResult {
    let n = seq1.len();
    let m = seq2.len();
    let cols = matrix.cols;
    let seq1_indices = encode_matrix_sequence(seq1, lookup_map);
    let seq2_indices = encode_matrix_sequence(seq2, lookup_map);

    if local {
        let mut max_score = 0;
        let mut max_positions = Vec::new();

        for i in 1..=n {
            let row_offset = i * cols;
            let prev_row_offset = (i - 1) * cols;
            let (prior_rows, current_and_after) = matrix.cells.split_at_mut(row_offset);
            let prev_row = &prior_rows[prev_row_offset..row_offset];
            let row = &mut current_and_after[..cols];
            let seq1_index = seq1_indices[i - 1] * score_dimension;

            for j in 1..=m {
                let substitution = score_table[seq1_index + seq2_indices[j - 1]];
                let diag_score = prev_row[j - 1].score.saturating_add(substitution);
                let up_score = prev_row[j].score.saturating_add(gap);
                let left_score = row[j - 1].score.saturating_add(gap);

                let max_candidate = diag_score.max(up_score).max(left_score);
                let cell_score = max_candidate.max(0);

                let mut arrows = 0;
                if cell_score > 0 {
                    if diag_score == cell_score {
                        arrows |= Arrows::DIAGONAL;
                    }
                    if up_score == cell_score {
                        arrows |= Arrows::UP;
                    }
                    if left_score == cell_score {
                        arrows |= Arrows::LEFT;
                    }
                }

                row[j] = Cell::with_arrows(cell_score, Arrows(arrows));

                if cell_score > max_score {
                    max_score = cell_score;
                    max_positions.clear();
                    if cell_score > 0 {
                        max_positions.push((i, j));
                    }
                } else if cell_score == max_score && cell_score > 0 {
                    max_positions.push((i, j));
                }
            }
        }

        FillResult {
            max_score,
            max_positions,
        }
    } else {
        for i in 1..=n {
            let row_offset = i * cols;
            let prev_row_offset = (i - 1) * cols;
            let (prior_rows, current_and_after) = matrix.cells.split_at_mut(row_offset);
            let prev_row = &prior_rows[prev_row_offset..row_offset];
            let row = &mut current_and_after[..cols];
            let seq1_index = seq1_indices[i - 1] * score_dimension;

            for j in 1..=m {
                let substitution = score_table[seq1_index + seq2_indices[j - 1]];
                let diag_score = prev_row[j - 1].score.saturating_add(substitution);
                let up_score = prev_row[j].score.saturating_add(gap);
                let left_score = row[j - 1].score.saturating_add(gap);

                let max_score = diag_score.max(up_score).max(left_score);

                let mut arrows = 0;
                if diag_score == max_score {
                    arrows |= Arrows::DIAGONAL;
                }
                if up_score == max_score {
                    arrows |= Arrows::UP;
                }
                if left_score == max_score {
                    arrows |= Arrows::LEFT;
                }

                row[j] = Cell::with_arrows(max_score, Arrows(arrows));
            }
        }

        FillResult {
            max_score: matrix.cells[n * cols + m].score,
            max_positions: Vec::new(),
        }
    }
}

fn uppercase_ascii_if_needed(seq: &[u8]) -> Option<Vec<u8>> {
    seq.iter()
        .any(|byte| byte.is_ascii_lowercase())
        .then(|| seq.iter().map(|byte| byte.to_ascii_uppercase()).collect())
}

fn encode_matrix_sequence(seq: &[u8], lookup_map: &[Option<u8>; 256]) -> Vec<usize> {
    seq.iter()
        .map(|&residue| {
            lookup_map[residue as usize]
                .expect("matrix-scored sequences must be validated before DP filling")
                as usize
        })
        .collect()
}

pub(crate) fn traceback_all_paths(
    matrix: &DPMatrix,
    seq1: &[u8],
    seq2: &[u8],
    start_positions: &[(usize, usize)],
    stop_condition: impl Fn(usize, usize, &Cell) -> bool + Copy,
    stop_on_no_arrows: bool,
) -> (Vec<TracebackPath>, Vec<AlignedPair>) {
    let mut all_paths = Vec::new();
    let mut all_alignments = Vec::new();
    let capacity = seq1.len() + seq2.len();
    let mut current_path = TracebackPath {
        steps: Vec::with_capacity(capacity),
    };
    let mut current_aln1 = Vec::with_capacity(capacity);
    let mut current_aln2 = Vec::with_capacity(capacity);

    for &(start_i, start_j) in start_positions {
        current_path.steps.clear();
        current_aln1.clear();
        current_aln2.clear();

        traceback_recursive(
            matrix,
            start_i,
            start_j,
            &mut current_path,
            &mut current_aln1,
            &mut current_aln2,
            seq1,
            seq2,
            &mut all_paths,
            &mut all_alignments,
            stop_condition,
            stop_on_no_arrows,
        );
    }

    (all_paths, all_alignments)
}

#[allow(clippy::too_many_arguments)]
fn traceback_recursive(
    matrix: &DPMatrix,
    i: usize,
    j: usize,
    current_path: &mut TracebackPath,
    current_aln1: &mut Vec<u8>,
    current_aln2: &mut Vec<u8>,
    seq1: &[u8],
    seq2: &[u8],
    all_paths: &mut Vec<TracebackPath>,
    all_alignments: &mut Vec<AlignedPair>,
    stop_condition: impl Fn(usize, usize, &Cell) -> bool + Copy,
    stop_on_no_arrows: bool,
) {
    current_path.push(i, j);

    let cell = matrix.get(i, j);
    if stop_condition(i, j, cell) || (stop_on_no_arrows && cell.arrows.bits() == 0) {
        let pair = AlignedPair {
            seq1_aligned: reversed_utf8_string(current_aln1),
            seq2_aligned: reversed_utf8_string(current_aln2),
        };

        all_paths.push(TracebackPath {
            steps: current_path.steps.clone(),
        });
        all_alignments.push(pair);
        current_path.steps.pop();
        return;
    }

    let arrows = cell.arrows;

    if arrows.has_diagonal() && i > 0 && j > 0 {
        current_aln1.push(seq1[i - 1]);
        current_aln2.push(seq2[j - 1]);

        traceback_recursive(
            matrix,
            i - 1,
            j - 1,
            current_path,
            current_aln1,
            current_aln2,
            seq1,
            seq2,
            all_paths,
            all_alignments,
            stop_condition,
            stop_on_no_arrows,
        );

        current_aln1.pop();
        current_aln2.pop();
    }

    if arrows.has_up() && i > 0 {
        current_aln1.push(seq1[i - 1]);
        current_aln2.push(b'-');

        traceback_recursive(
            matrix,
            i - 1,
            j,
            current_path,
            current_aln1,
            current_aln2,
            seq1,
            seq2,
            all_paths,
            all_alignments,
            stop_condition,
            stop_on_no_arrows,
        );

        current_aln1.pop();
        current_aln2.pop();
    }

    if arrows.has_left() && j > 0 {
        current_aln1.push(b'-');
        current_aln2.push(seq2[j - 1]);

        traceback_recursive(
            matrix,
            i,
            j - 1,
            current_path,
            current_aln1,
            current_aln2,
            seq1,
            seq2,
            all_paths,
            all_alignments,
            stop_condition,
            stop_on_no_arrows,
        );

        current_aln1.pop();
        current_aln2.pop();
    }

    current_path.steps.pop();
}

fn reversed_utf8_string(bytes: &[u8]) -> String {
    let reversed: Vec<u8> = bytes.iter().rev().copied().collect();
    String::from_utf8_lossy(&reversed).into_owned()
}
