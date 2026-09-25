//! Typst backend for drawing and predicting RNA secondary structures.

mod alphabet;
mod drawing;
mod prediction;

#[cfg(target_arch = "wasm32")]
wasm_minimal_protocol::initiate_protocol!();

/// Generates drawing geometry for an RNA secondary structure.
///
/// # Errors
///
/// Returns an error string if decoding, input validation, layout generation, or
/// response serialization fails.
#[cfg_attr(target_arch = "wasm32", wasm_minimal_protocol::wasm_func)]
pub fn layout(sequence: &[u8], structure: &[u8], config: &[u8]) -> Result<Vec<u8>, String> {
    let sequence = std::str::from_utf8(sequence)
        .map_err(|error| format!("Invalid UTF-8 in sequence: {error}"))?;
    let structure = std::str::from_utf8(structure)
        .map_err(|error| format!("Invalid UTF-8 in structure: {error}"))?;
    let config =
        drawing::config::parse(config).map_err(|error| format!("Invalid config JSON: {error}"))?;
    let input = drawing::validation::validate(sequence, structure)
        .map_err(|error| format!("Invalid RNA input: {error}"))?;

    let algorithm_layout = drawing::layout(&config, &input.pair_table)
        .map_err(|error| format!("Layout failed: {error}"))?;

    let response = drawing::output::into_response(input.base_pairs, algorithm_layout)
        .map_err(|error| format!("Layout failed: {error}"))?;
    serde_json::to_vec(&response).map_err(|error| format!("Serialization failed: {error}"))
}

/// Fits measured RNA drawing primitives into a viewport.
///
/// # Errors
///
/// Returns an error string if the JSON is invalid, the geometry is non-finite or
/// inconsistent, the content does not fit, or serialization fails.
#[cfg_attr(target_arch = "wasm32", wasm_minimal_protocol::wasm_func)]
pub fn fit(config: &[u8]) -> Result<Vec<u8>, String> {
    drawing::fit(config).map_err(|error| format!("Fit failed: {error}"))
}

/// Predicts an RNA secondary structure and returns its model-specific score.
///
/// The sequence is UTF-8 bytes and the configuration is JSON. Configuration
/// accepts the model, beam size, sharp-turn policy, dangle model, topology, and
/// optional dot-bracket constraints.
///
/// # Errors
///
/// Returns an error string if parsing, validation, folding, or serialization
/// fails.
#[cfg_attr(target_arch = "wasm32", wasm_minimal_protocol::wasm_func)]
pub fn predict(sequence: &[u8], config: &[u8]) -> Result<Vec<u8>, String> {
    prediction::predict(sequence, config).map_err(|error| error.to_string())
}

/// Tests for the three exported entry points and their Typst byte interface.
#[cfg(test)]
mod tests {
    use crate::drawing::testing::all_numbers_are_finite;
    use serde_json::{Value, json};

    const TRNA_SEQUENCE: &str =
        "GCGGAUUUAGCUCAGUUGGGAGAGCGCCAGACUGAAGAUCUGGAGGUCCUGUGUUCGAUCCACAGAAUUCGCA";
    const TRNA_STRUCTURE: &str =
        "(((((((..((((........)))).(((((.......))))).....(((((.......)))))))))))).";
    const ALGORITHMS: [&str; 5] = ["radial", "naview", "rna_turtle", "rna_puzzler", "circular"];

    fn layout_json(sequence: &str, structure: &str, algorithm: &str) -> Value {
        let config = format!(r#"{{"algorithm":"{algorithm}"}}"#);
        let response = super::layout(sequence.as_bytes(), structure.as_bytes(), config.as_bytes())
            .expect("layout must succeed");
        serde_json::from_slice(&response).expect("layout response must be JSON")
    }

    fn predict_json(sequence: &[u8], config: Value) -> Result<Value, String> {
        let config = serde_json::to_vec(&config).expect("test config must serialize");
        let response = super::predict(sequence, &config)?;
        serde_json::from_slice(&response).map_err(|error| error.to_string())
    }

    #[test]
    fn decoding_failures_name_the_input_that_failed() {
        // Typst matches these prefixes, so keep each one stable.
        let bad_sequence = super::layout(&[0xff], b".", br#"{"algorithm":"radial"}"#).unwrap_err();
        assert!(bad_sequence.starts_with("Invalid UTF-8 in sequence:"));

        let bad_structure = super::layout(b"A", &[0xff], br#"{"algorithm":"radial"}"#).unwrap_err();
        assert!(bad_structure.starts_with("Invalid UTF-8 in structure:"));

        let bad_config = super::layout(b"A", b".", b"not json").unwrap_err();
        assert!(bad_config.starts_with("Invalid config JSON:"));

        let bad_input = super::layout(b"A", b"..", br#"{"algorithm":"radial"}"#).unwrap_err();
        assert!(bad_input.starts_with("Invalid RNA input:"));

        let bad_fit = super::fit(b"not json").unwrap_err();
        assert!(bad_fit.starts_with("Fit failed:"));

        let bad_prediction_config = super::predict(b"A", b"not json").unwrap_err();
        assert!(bad_prediction_config.starts_with("invalid prediction config JSON:"));
    }

    #[test]
    fn config_requires_an_algorithm() {
        let missing = super::layout(b"A", b".", b"{}").unwrap_err();
        assert!(missing.contains("missing field `algorithm`"));
    }

    #[test]
    fn layout_names_require_the_rna_prefix() {
        // The wire format uses the prefixed names. The upstream names are not
        // aliases, because accepting both would create two spellings in the API.
        for algorithm in ["turtle", "puzzler"] {
            let config = format!(r#"{{"algorithm":"{algorithm}"}}"#);
            let error = super::layout(b"A", b".", config.as_bytes()).unwrap_err();
            assert!(error.starts_with("Invalid config JSON:"));
        }
    }

    #[test]
    fn every_algorithm_returns_complete_geometry_for_a_trna() {
        for algorithm in ALGORITHMS {
            let response = layout_json(TRNA_SEQUENCE, TRNA_STRUCTURE, algorithm);
            assert_eq!(
                response["coordinates"].as_array().unwrap().len(),
                TRNA_SEQUENCE.len(),
                "{algorithm}: wrong coordinate count"
            );
            assert_eq!(
                response["backbone"].as_array().unwrap().len(),
                TRNA_SEQUENCE.len() - 1,
                "{algorithm}: wrong backbone count"
            );
            assert_eq!(
                response["base_pairs"].as_array().unwrap().len(),
                TRNA_STRUCTURE.matches('(').count(),
                "{algorithm}: wrong pair count"
            );
            assert!(
                all_numbers_are_finite(&response),
                "{algorithm}: non-finite geometry"
            );
        }
    }

    #[test]
    fn the_response_carries_only_typst_geometry() {
        let response = layout_json(TRNA_SEQUENCE, TRNA_STRUCTURE, "rna_puzzler");
        let object = response.as_object().expect("response must be an object");

        let mut keys = object.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        // Typst already has the sequence, so returning it would duplicate every
        // drawing's payload.
        assert_eq!(
            keys,
            ["backbone", "base_pairs", "coordinates", "nominal_spacing"]
        );

        for segment in object["backbone"].as_array().unwrap() {
            let Some(arc) = segment.as_object() else {
                assert!(segment.is_null());
                continue;
            };
            let mut arc_keys = arc.keys().cloned().collect::<Vec<_>>();
            arc_keys.sort();
            assert_eq!(arc_keys, ["center", "clockwise", "radius"]);
        }
    }

    #[test]
    fn pair_curves_are_present_only_for_the_layout_that_bows() {
        let circular = layout_json("AAAAAAAAA", "(((...)))", "circular");
        let curves = circular["pair_curves"]
            .as_array()
            .expect("circular must emit pair curves");
        assert_eq!(
            curves.len(),
            circular["base_pairs"].as_array().unwrap().len()
        );
        // Exercise both states of this optional field.
        assert!(curves.iter().any(Value::is_object));
        assert!(curves.iter().any(Value::is_null));
        for curve in curves.iter().filter_map(Value::as_object) {
            let mut keys = curve.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            assert_eq!(keys, ["control_i", "control_j"]);
        }

        for algorithm in ["radial", "naview", "rna_turtle", "rna_puzzler"] {
            let response = layout_json("AAAAAAAAA", "(((...)))", algorithm);
            assert!(
                response.get("pair_curves").is_none(),
                "{algorithm} draws chords, so it must omit pair_curves entirely"
            );
        }
    }

    #[test]
    fn prediction_options_reach_the_folding_engine() {
        // This proves the options survived the byte boundary, not what the
        // engine computes from them. The forced pair differs from the
        // unconstrained fold, so the structure matches only when the
        // constraints arrive intact.
        let constrained = predict_json(
            b"AU",
            json!({"beam_size": 1, "allow_sharp_turns": true, "constraints": "()"}),
        )
        .expect("forced sharp pair must succeed");
        assert_eq!(constrained["structure"], "()");
        assert_eq!(
            predict_json(b"AU", json!({})).expect("unconstrained fold must succeed")["structure"],
            ".."
        );
    }

    #[test]
    fn invalid_prediction_requests_return_contextual_errors() {
        let unknown_field =
            predict_json(b"A", json!({"unknown": true})).expect_err("unknown fields must fail");
        assert!(unknown_field.contains("unknown field"));

        let non_iupac =
            predict_json(b"GGGGXAAAA", json!({})).expect_err("non-IUPAC letters must fail");
        assert!(non_iupac.contains("invalid sequence byte 0x58 at index 4"));

        let circular = predict_json(b"ACGU", json!({"topology": "Circular"}))
            .expect_err("ContraFold does not support circular folding");
        assert!(circular.contains("circular folding is supported only"));
    }

    #[test]
    fn vienna_circular_configuration_reaches_the_exported_predict_api() {
        let response = predict_json(
            b"GGGAAACCCGGGAAACCC",
            json!({
                "model": "ViennaRnafold",
                "topology": "Circular",
                "beam_size": 0
            }),
        )
        .expect("Vienna circular prediction must succeed");

        assert_eq!(response["structure"], "(((...)))(((...)))");
        assert_eq!(response["score"]["model"], "ViennaRnafold");
        assert_eq!(response["score"]["value"], -4.8);
    }

    #[test]
    fn fit_rejects_a_request_with_no_drawing_primitives() {
        // `decoding_failures_name_the_input_that_failed` owns the "Fit failed:"
        // prefix. This owns the message behind it.
        let empty = super::fit(
            br#"{"width_mode":"auto","height_mode":"auto","lines":[],"arcs":[],"boxes":[]}"#,
        )
        .unwrap_err();
        assert!(empty.contains("at least one drawing primitive"));
    }

    #[test]
    fn fit_rejects_a_trimmed_line_below_its_minimum_visible_length() {
        let result = super::fit(
            br#"{
                "width_mode":"auto",
                "height_mode":"auto",
                "lines":[{
                    "start":{"geometry":{"x":0,"y":0},"page_pt":{"x":2,"y":0}},
                    "end":{"geometry":{"x":10,"y":0},"page_pt":{"x":-2,"y":0}},
                    "half_stroke_pt":0.5,
                    "minimum_visible_length_pt":6.5
                }],
                "arcs":[],
                "boxes":[]
            }"#,
        );

        let error = result.expect_err("a line below its visible-length floor must be omitted");
        assert!(error.contains("no visible drawing primitives remain"));
    }

    #[test]
    fn fit_preserves_all_primitive_categories_through_the_wire_api() {
        let response = super::fit(
            br#"{
                "width_mode":"auto",
                "height_mode":"auto",
                "lines":[{
                    "start":{"geometry":{"x":0,"y":0},"page_pt":{"x":0,"y":0}},
                    "end":{"geometry":{"x":10,"y":0},"page_pt":{"x":0,"y":0}},
                    "half_stroke_pt":0.5
                }],
                "arcs":[{
                    "center":{"x":0,"y":0},
                    "radius":5,
                    "start_angle_rad":0,
                    "span_rad":1.5707963267948966,
                    "endpoint_gap_pt":0,
                    "half_stroke_pt":0.5
                }],
                "curves":[{
                    "start":{"geometry":{"x":0,"y":2},"page_pt":{"x":0,"y":0}},
                    "control_1":{"x":1,"y":4},
                    "control_2":{"x":3,"y":4},
                    "end":{"geometry":{"x":4,"y":2},"page_pt":{"x":0,"y":0}},
                    "half_stroke_pt":0.5
                }],
                "boxes":[{
                    "anchor":{"x":10,"y":5},
                    "width_pt":4,
                    "height_pt":6
                }],
                "ticks":[{
                    "anchor":{"x":10,"y":0},
                    "previous":{"x":0,"y":0},
                    "offset_pt":1,
                    "tick_len_pt":3,
                    "gap_pt":1,
                    "half_stroke_pt":0.5,
                    "label_width_pt":4,
                    "label_height_pt":3
                }]
            }"#,
        )
        .expect("all fit primitive categories must be accepted");
        let response: Value = serde_json::from_slice(&response).expect("fit response must be JSON");

        assert_eq!(response["lines"].as_array().map(Vec::len), Some(1));
        assert_eq!(response["arcs"].as_array().map(Vec::len), Some(1));
        assert_eq!(response["curves"].as_array().map(Vec::len), Some(1));
        assert_eq!(response["boxes"].as_array().map(Vec::len), Some(1));
        assert_eq!(response["ticks"].as_array().map(Vec::len), Some(1));

        // At the natural scale the line keeps its ten-unit span and stays level.
        let line = &response["lines"][0];
        let start_x = line["start_pt"]["x"].as_f64().unwrap();
        assert!((line["end_pt"]["x"].as_f64().unwrap() - start_x - 10.0).abs() < 1e-9);
        assert_eq!(line["start_pt"]["y"], line["end_pt"]["y"]);
        assert_eq!(
            response["curves"][0]["end_pt"]["x"].as_f64().unwrap()
                - response["curves"][0]["start_pt"]["x"].as_f64().unwrap(),
            4.0
        );
        assert!(response["arcs"][0]["points_pt"].as_array().unwrap().len() >= 2);
        assert!(response["boxes"][0]["top_left_pt"].is_object());
        assert!(response["ticks"][0]["top_left_pt"].is_object());
        assert!(all_numbers_are_finite(&response));
    }

    #[test]
    fn fit_rejects_unknown_wire_fields_through_the_exported_api() {
        let unknown = super::fit(
            br#"{
                "width_mode":"auto",
                "height_mode":"auto",
                "lines":[],
                "arcs":[],
                "boxes":[{"anchor":{"x":0,"y":0},
                    "width_pt":1,"height_pt":1}],
                "unexpected":true
            }"#,
        )
        .expect_err("unknown fit fields must be rejected");
        assert!(unknown.contains("Fit failed: invalid fit config JSON"));
        assert!(unknown.contains("unknown field `unexpected`"));
    }

    #[test]
    fn fit_rejects_invalid_owner_boxes_through_the_exported_api() {
        let invalid_owner = super::fit(
            br#"{
                "width_mode":"auto",
                "height_mode":"auto",
                "lines":[],
                "arcs":[],
                "boxes":[{"anchor":{"x":0,"y":0},
                    "width_pt":1,"height_pt":1}],
                "ticks":[{
                    "anchor":{"x":0,"y":0},
                    "owner_box":1,
                    "offset_pt":0,
                    "tick_len_pt":1,
                    "gap_pt":0,
                    "half_stroke_pt":0,
                    "label_width_pt":1,
                    "label_height_pt":1
                }]
            }"#,
        )
        .expect_err("an invalid owner box must be rejected");
        assert!(invalid_owner.contains("Fit failed: invalid fit request"));
        assert!(invalid_owner.contains("owner_box must index an existing box"));
    }
}
