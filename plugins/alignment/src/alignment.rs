//! Core alignment data structures and algorithm trait.

use serde::{Deserialize, Serialize};

use crate::scoring::{AlignmentError, ScoringConfig};

/// Arrow directions stored as a 3-bit bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Arrows(u8);

impl Arrows {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn bits(&self) -> u8 {
        self.0
    }

    pub fn has_diagonal(&self) -> bool {
        (self.0 & 1) != 0
    }

    pub fn has_up(&self) -> bool {
        (self.0 & 2) != 0
    }

    pub fn has_left(&self) -> bool {
        (self.0 & 4) != 0
    }
}

impl Arrows {
    pub const NONE: u8 = 0;
    pub const DIAGONAL: u8 = 1;
    pub const UP: u8 = 2;
    pub const LEFT: u8 = 4;

    pub fn set_diagonal(&mut self) {
        self.0 |= Self::DIAGONAL;
    }

    pub fn set_up(&mut self) {
        self.0 |= Self::UP;
    }

    pub fn set_left(&mut self) {
        self.0 |= Self::LEFT;
    }
}

/// A cell in the dynamic programming matrix.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cell {
    pub score: i32,
    pub arrows: Arrows,
}

impl Cell {
    pub fn new(score: i32) -> Self {
        Self {
            score,
            arrows: Arrows::new(),
        }
    }

    pub fn with_arrows(score: i32, arrows: Arrows) -> Self {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DPMatrix {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Cell>,
}

impl DPMatrix {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            cells: vec![Cell::default(); rows * cols],
        }
    }

    #[inline]
    pub fn get(&self, i: usize, j: usize) -> &Cell {
        &self.cells[i * self.cols + j]
    }

    #[inline]
    pub fn set(&mut self, i: usize, j: usize, cell: Cell) {
        self.cells[i * self.cols + j] = cell;
    }
}

/// A step in the traceback path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracebackStep {
    pub i: usize,
    pub j: usize,
}

impl TracebackStep {
    pub fn new(i: usize, j: usize) -> Self {
        Self { i, j }
    }
}

/// A complete traceback path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracebackPath {
    pub steps: Vec<TracebackStep>,
}

impl TracebackPath {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            steps: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, i: usize, j: usize) {
        self.steps.push(TracebackStep::new(i, j));
    }
}

/// A pair of aligned sequences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignedPair {
    pub seq1_aligned: String,
    pub seq2_aligned: String,
}

impl AlignedPair {
    pub fn new(seq1: String, seq2: String) -> Self {
        Self {
            seq1_aligned: seq1,
            seq2_aligned: seq2,
        }
    }
}

/// The complete result of an alignment operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentResult {
    pub seq1: String,
    pub seq2: String,
    pub scoring: ScoringConfig,
    pub matrix: DPMatrix,
    pub traceback_paths: Vec<TracebackPath>,
    pub alignments: Vec<AlignedPair>,
    pub final_score: i32,
}

/// Trait for sequence alignment algorithms.
pub trait Aligner {
    fn align(&self, seq1: &[u8], seq2: &[u8]) -> Result<AlignmentResult, AlignmentError>;
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
) -> Result<FillResult, AlignmentError> {
    let n = seq1.len();
    let m = seq2.len();
    let gap = scoring.gap_open;

    if local {
        let mut max_score = 0;
        let mut max_positions = Vec::new();

        for i in 1..=n {
            for j in 1..=m {
                let s = scoring.substitution_score(seq1[i - 1], seq2[j - 1])?;
                let diag_score = matrix.get(i - 1, j - 1).score.saturating_add(s);
                let up_score = matrix.get(i - 1, j).score.saturating_add(gap);
                let left_score = matrix.get(i, j - 1).score.saturating_add(gap);

                let max_candidate = diag_score.max(up_score).max(left_score);
                let cell_score = max_candidate.max(0);

                let mut arrows = Arrows::new();
                if cell_score > 0 {
                    if diag_score == cell_score {
                        arrows.set_diagonal();
                    }
                    if up_score == cell_score {
                        arrows.set_up();
                    }
                    if left_score == cell_score {
                        arrows.set_left();
                    }
                }

                matrix.set(i, j, Cell::with_arrows(cell_score, arrows));

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

        Ok(FillResult {
            max_score,
            max_positions,
        })
    } else {
        for i in 1..=n {
            for j in 1..=m {
                let s = scoring.substitution_score(seq1[i - 1], seq2[j - 1])?;
                let diag_score = matrix.get(i - 1, j - 1).score.saturating_add(s);
                let up_score = matrix.get(i - 1, j).score.saturating_add(gap);
                let left_score = matrix.get(i, j - 1).score.saturating_add(gap);

                let max_score = diag_score.max(up_score).max(left_score);

                let mut arrows = Arrows::new();
                if diag_score == max_score {
                    arrows.set_diagonal();
                }
                if up_score == max_score {
                    arrows.set_up();
                }
                if left_score == max_score {
                    arrows.set_left();
                }

                matrix.set(i, j, Cell::with_arrows(max_score, arrows));
            }
        }

        Ok(FillResult {
            max_score: matrix.get(n, m).score,
            max_positions: Vec::new(),
        })
    }
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

    for &(start_i, start_j) in start_positions {
        let initial_path = TracebackPath::with_capacity(capacity);
        let aln1 = Vec::with_capacity(capacity);
        let aln2 = Vec::with_capacity(capacity);

        traceback_recursive(
            matrix,
            start_i,
            start_j,
            initial_path,
            aln1,
            aln2,
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
    mut current_path: TracebackPath,
    mut current_aln1: Vec<u8>,
    mut current_aln2: Vec<u8>,
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
        current_aln1.reverse();
        current_aln2.reverse();

        let pair = AlignedPair::new(
            String::from_utf8_lossy(&current_aln1).into_owned(),
            String::from_utf8_lossy(&current_aln2).into_owned(),
        );

        all_paths.push(current_path);
        all_alignments.push(pair);
        return;
    }

    let arrows = cell.arrows;

    if arrows.has_diagonal() && i > 0 && j > 0 {
        let mut next_aln1 = current_aln1.clone();
        let mut next_aln2 = current_aln2.clone();
        next_aln1.push(seq1[i - 1]);
        next_aln2.push(seq2[j - 1]);

        traceback_recursive(
            matrix,
            i - 1,
            j - 1,
            current_path.clone(),
            next_aln1,
            next_aln2,
            seq1,
            seq2,
            all_paths,
            all_alignments,
            stop_condition,
            stop_on_no_arrows,
        );
    }

    if arrows.has_up() && i > 0 {
        let mut next_aln1 = current_aln1.clone();
        let mut next_aln2 = current_aln2.clone();
        next_aln1.push(seq1[i - 1]);
        next_aln2.push(b'-');

        traceback_recursive(
            matrix,
            i - 1,
            j,
            current_path.clone(),
            next_aln1,
            next_aln2,
            seq1,
            seq2,
            all_paths,
            all_alignments,
            stop_condition,
            stop_on_no_arrows,
        );
    }

    if arrows.has_left() && j > 0 {
        let mut next_aln1 = current_aln1.clone();
        let mut next_aln2 = current_aln2.clone();
        next_aln1.push(b'-');
        next_aln2.push(seq2[j - 1]);

        traceback_recursive(
            matrix,
            i,
            j - 1,
            current_path,
            next_aln1,
            next_aln2,
            seq1,
            seq2,
            all_paths,
            all_alignments,
            stop_condition,
            stop_on_no_arrows,
        );
    }
}
