//! seq-align: Pairwise sequence alignment library

pub mod aligners;
pub mod alignment;
pub mod matrices;
pub mod output;
pub mod scoring;

use serde::Deserialize;
use wasm_minimal_protocol::*;

// Re-export main types
pub use aligners::{GlobalAligner, LocalAligner};
pub use alignment::{AlignedPair, Aligner, AlignmentResult, Arrows, Cell, DPMatrix};
pub use matrices::BuiltinMatrix;
use matrices::matrix_data_by_name;
pub use output::AlignmentResultOutput;
pub use scoring::{AlignmentError, ScoringConfig, SubstitutionScorer};

initiate_protocol!();

/// Configuration for alignment, deserialized from JSON.
#[derive(Deserialize)]
struct AlignConfig {
    mode: String,
    matrix: Option<String>,
    match_score: Option<i32>,
    mismatch_score: Option<i32>,
    gap_open: i32,
    gap_extend: i32,
}

impl AlignConfig {
    fn validate(&self) -> Result<(), String> {
        let has_matrix = self.matrix.is_some();
        let has_match = self.match_score.is_some();
        let has_mismatch = self.mismatch_score.is_some();

        if has_matrix && (has_match || has_mismatch) {
            return Err("Cannot use both 'matrix' and 'match_score'/'mismatch_score' - they are mutually exclusive".into());
        }
        if !has_matrix && has_match != has_mismatch {
            return Err(
                "Both 'match_score' and 'mismatch_score' are required when not using a matrix"
                    .into(),
            );
        }
        if !has_matrix && !has_match {
            return Err("Scoring method required: provide either 'matrix' or both 'match_score' and 'mismatch_score'".into());
        }
        if self.gap_open != self.gap_extend {
            return Err(format!(
                "Affine gap penalties not supported: gap_open ({}) must equal gap_extend ({})",
                self.gap_open, self.gap_extend
            ));
        }
        Ok(())
    }
}

/// WASM entry point for sequence alignment (supports both global and local).
///
/// # Arguments
/// * `seq1` - First sequence as UTF-8 bytes
/// * `seq2` - Second sequence as UTF-8 bytes
/// * `config` - JSON-encoded configuration object
///
/// # Returns
/// JSON bytes of AlignmentResult or an error string.
#[wasm_func]
pub fn align(seq1: &[u8], seq2: &[u8], config: &[u8]) -> Result<Vec<u8>, String> {
    let seq1_str =
        std::str::from_utf8(seq1).map_err(|e| format!("Invalid UTF-8 in seq1: {}", e))?;
    let seq2_str =
        std::str::from_utf8(seq2).map_err(|e| format!("Invalid UTF-8 in seq2: {}", e))?;

    let config: AlignConfig =
        serde_json::from_slice(config).map_err(|e| format!("Invalid config JSON: {}", e))?;

    config.validate()?;

    let scoring = if let Some(ref name) = config.matrix {
        if let Some(bm) = BuiltinMatrix::from_str(name) {
            ScoringConfig::with_matrix(bm, config.gap_open, config.gap_extend)
        } else {
            return Err(format!("Unknown matrix name: '{}'", name));
        }
    } else {
        ScoringConfig::linear(
            config.match_score.unwrap(),
            config.mismatch_score.unwrap(),
            config.gap_open,
            config.gap_extend,
        )
    };

    let result = match config.mode.to_lowercase().as_str() {
        "global" => {
            let aligner = GlobalAligner::new(scoring);
            aligner.align(seq1_str.as_bytes(), seq2_str.as_bytes())
        }
        "local" => {
            let aligner = LocalAligner::new(scoring);
            aligner.align(seq1_str.as_bytes(), seq2_str.as_bytes())
        }
        _ => {
            return Err(format!(
                "Unknown alignment mode '{}'. Use 'global' or 'local'.",
                config.mode
            ));
        }
    };

    match result {
        Ok(alignment_result) => {
            let output = AlignmentResultOutput::from(&alignment_result);
            serde_json::to_vec(&output).map_err(|e| format!("Serialization failed: {}", e))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// WASM entry point for retrieving built-in scoring matrix data.
///
/// # Arguments
/// * `name` - Matrix name as UTF-8 bytes (e.g., "BLOSUM62")
///
/// # Returns
/// JSON bytes with matrix data (name, alphabet, scores) or an error string.
#[wasm_func]
pub fn matrix_info(name: &[u8]) -> Result<Vec<u8>, String> {
    let name_str =
        std::str::from_utf8(name).map_err(|e| format!("Invalid UTF-8 in matrix name: {}", e))?;

    let data = matrix_data_by_name(name_str)
        .ok_or_else(|| format!("Unknown matrix name: '{}'", name_str))?;

    // Convert alphabet from Vec<u8> to Vec<String> for JSON output
    let output = serde_json::json!({
        "name": data.name,
        "alphabet": data.alphabet.iter().map(|&b| (b as char).to_string()).collect::<Vec<_>>(),
        "scores": data.scores
    });

    serde_json::to_vec(&output).map_err(|e| format!("Serialization failed: {}", e))
}

/// WASM entry point for listing all available scoring matrices.
///
/// # Returns
/// JSON bytes with array of matrix names.
#[wasm_func]
pub fn list_matrices() -> Result<Vec<u8>, String> {
    let names = BuiltinMatrix::all_names();
    let result = serde_json::json!({ "matrices": names });
    serde_json::to_vec(&result).map_err(|e| format!("Serialization failed: {}", e))
}
