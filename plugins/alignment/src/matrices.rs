use crate::scoring::AlignmentError;
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/generated_matrices.rs"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixData {
    pub name: String,
    pub alphabet: Vec<u8>,
    pub scores: Vec<i32>,
}

impl MatrixData {
    pub fn from_builtin(matrix: BuiltinMatrix) -> Self {
        let alphabet = matrix.alphabet().to_vec();

        Self {
            name: matrix.name().to_string(),
            alphabet,
            scores: matrix.scores().to_vec(),
        }
    }
}

pub fn matrix_data_by_name(name: &str) -> Option<MatrixData> {
    BuiltinMatrix::from_str(name).map(MatrixData::from_builtin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_data_by_name() {
        let data = matrix_data_by_name("BLOSUM62").expect("matrix not found");
        let n = data.alphabet.len();
        assert_eq!(data.name, "BLOSUM62");
        assert_eq!(data.scores.len(), n * n);
    }
}
