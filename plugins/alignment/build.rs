use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

fn normalize_residue(token: &str, matrix: &str) -> String {
    if token.len() != 1 || !token.is_ascii() {
        panic!(
            "Matrix {} has non-ASCII or multi-character residue: {}",
            matrix, token
        );
    }
    let b = token.as_bytes()[0];
    if b.is_ascii_alphabetic() {
        (b.to_ascii_uppercase() as char).to_string()
    } else {
        token.to_string()
    }
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("generated_matrices.rs");
    let mut f = File::create(&dest_path).unwrap();

    let mut variants = Vec::new();
    let mut from_str_arms = Vec::new();
    let mut name_arms = Vec::new();
    let mut score_dimension_arms = Vec::new();
    let mut scores_arms = Vec::new();
    let mut lookup_map_arms = Vec::new();
    let mut alphabet_arms = Vec::new();

    let entries = fs::read_dir("src/data").expect("Failed to read src/data");
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("mat") {
            let filename = path.file_stem().unwrap().to_str().unwrap();
            let uppercase_name = filename.to_ascii_uppercase();
            let pascal_name = to_pascal_case(&uppercase_name);

            let file = File::open(&path).unwrap();
            let reader = BufReader::new(file);
            let mut residues = Vec::new();
            let mut scores: Vec<i32> = Vec::new();
            let mut row_index = 0usize;

            for line in reader.lines() {
                let line = line.unwrap();
                let line = match line.split_once('#') {
                    Some((before, _)) => before.trim(),
                    None => line.trim(),
                };
                if line.is_empty() {
                    continue;
                }

                if residues.is_empty() {
                    residues = line
                        .split_whitespace()
                        .map(|s| normalize_residue(s, &uppercase_name))
                        .collect();

                    let dim = residues.len();
                    scores = Vec::with_capacity(dim * dim);
                } else {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() == residues.len() + 1 {
                        if row_index >= residues.len() {
                            panic!("Matrix {} has too many rows", uppercase_name);
                        }
                        let row_label = normalize_residue(parts[0], &uppercase_name);
                        if row_label != residues[row_index] {
                            panic!(
                                "Matrix {} row label mismatch: expected {}, got {}",
                                uppercase_name, residues[row_index], row_label
                            );
                        }

                        // Skip the first part (the residue label) and parse the scores
                        for score_str in &parts[1..] {
                            let score = match *score_str {
                                "inf" => i32::MAX,
                                "-inf" => i32::MIN,
                                _ => score_str.parse::<i32>().unwrap_or_else(|_| {
                                    panic!(
                                        "Matrix {} has invalid score token: {}",
                                        uppercase_name, score_str
                                    )
                                }),
                            };
                            scores.push(score);
                        }
                        row_index += 1;
                    } else if !parts.is_empty() {
                        panic!(
                            "Matrix {} row has unexpected column count: {}",
                            uppercase_name,
                            parts.len()
                        );
                    }
                }
            }

            if residues.is_empty() {
                panic!("Matrix {} is missing residues", uppercase_name);
            }
            if row_index != residues.len() {
                panic!(
                    "Matrix {} has {} rows, expected {}",
                    uppercase_name,
                    row_index,
                    residues.len()
                );
            }

            let dim = residues.len();
            assert_eq!(scores.len(), dim * dim, "Matrix {} is not square", uppercase_name);

            // Generate Map
            let mut map = [None; 256];
            for (i, res) in residues.iter().enumerate() {
                let b = res.as_bytes()[0];
                map[b as usize] = Some(i as u8);
                if b.is_ascii_alphabetic() {
                    map[b.to_ascii_uppercase() as usize] = Some(i as u8);
                    map[b.to_ascii_lowercase() as usize] = Some(i as u8);
                }
            }

            // Write static data
            writeln!(f, "const {}_SCORES: [i32; {}] = {:?};", uppercase_name, scores.len(), scores).unwrap();
            
            let map_str = format!("{:?}", map).replace("None", "None").replace("Some", "Some");
            writeln!(f, "const {}_MAP: [Option<u8>; 256] = {};", uppercase_name, map_str).unwrap();

            let alphabet: Vec<u8> = residues
                .iter()
                .map(|res| res.as_bytes()[0].to_ascii_uppercase())
                .collect();
            writeln!(
                f,
                "const {}_ALPHABET: [u8; {}] = {:?};",
                uppercase_name,
                alphabet.len(),
                alphabet
            )
            .unwrap();

            variants.push(pascal_name.clone());
            from_str_arms.push(format!("\"{}\" => Some(Self::{})", uppercase_name, pascal_name));
            name_arms.push(format!("Self::{} => \"{}\"", pascal_name, uppercase_name));
            score_dimension_arms.push(format!("Self::{} => {}", pascal_name, dim));
            scores_arms.push(format!("Self::{} => &{}_SCORES", pascal_name, uppercase_name));
            lookup_map_arms.push(format!("Self::{} => &{}_MAP", pascal_name, uppercase_name));
            alphabet_arms.push(format!("Self::{} => &{}_ALPHABET", pascal_name, uppercase_name));
        }
    }

    // Generate list of all matrix names for all_names() method
    let all_names: Vec<String> = variants.iter()
        .map(|v| {
            // Convert PascalCase back to UPPERCASE (e.g., Blosum62 -> BLOSUM62)
            v.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_uppercase()
        })
        .collect();
    let all_names_str = all_names.iter()
        .map(|n| format!("\"{}\"", n))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(f, "
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuiltinMatrix {{
    {}
}}

impl BuiltinMatrix {{
    pub fn from_str(s: &str) -> Option<Self> {{
        match s.to_ascii_uppercase().as_str() {{
            {},
            _ => None,
        }}
    }}

    pub fn name(&self) -> &'static str {{
        match self {{
            {}
        }}
    }}

    pub fn score_dimension(&self) -> usize {{
        match self {{
            {}
        }}
    }}

    pub fn scores(&self) -> &'static [i32] {{
        match self {{
            {}
        }}
    }}

    pub fn lookup_map(&self) -> &'static [Option<u8>; 256] {{
        match self {{
            {}
        }}
    }}

    pub fn alphabet(&self) -> &'static [u8] {{
        match self {{
            {}
        }}
    }}

    pub fn score(&self, a: u8, b: u8) -> Result<i32, AlignmentError> {{
        let map = self.lookup_map();
        let i = map[a as usize].ok_or(AlignmentError::InvalidCharacter(a))?;
        let j = map[b as usize].ok_or(AlignmentError::InvalidCharacter(b))?;
        let n = self.score_dimension();
        Ok(self.scores()[i as usize * n + j as usize])
    }}

    pub fn all_names() -> &'static [&'static str] {{
        &[{}]
    }}
}}
",
    variants.join(",\n    "),
    from_str_arms.join(",\n            "),
    name_arms.join(",\n            "),
    score_dimension_arms.join(",\n            "),
    scores_arms.join(",\n            "),
    lookup_map_arms.join(",\n            "),
    alphabet_arms.join(",\n            "),
    all_names_str
    ).unwrap();

    println!("cargo:rerun-if-changed=src/data");
}
