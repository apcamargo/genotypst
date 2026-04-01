include!(concat!(env!("OUT_DIR"), "/generated_matrices.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_matrix_from_str() {
        let matrix = BuiltinMatrix::from_str("BLOSUM62").expect("matrix not found");
        let n = matrix.alphabet().len();
        assert_eq!(matrix.name(), "BLOSUM62");
        assert_eq!(matrix.scores().len(), n * n);
    }
}
