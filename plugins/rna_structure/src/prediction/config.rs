use super::{DangleModel, FoldOptions, Model, SequenceTopology};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PredictionRequest {
    pub(crate) model: ModelWire,
    pub(crate) beam_size: usize,
    pub(crate) allow_sharp_turns: bool,
    pub(crate) dangles: DangleModelWire,
    pub(crate) topology: SequenceTopologyWire,
    pub(crate) constraints: Option<String>,
}

impl Default for PredictionRequest {
    fn default() -> Self {
        Self {
            model: ModelWire::ContraFold,
            beam_size: 100,
            allow_sharp_turns: false,
            dangles: DangleModelWire::Both,
            topology: SequenceTopologyWire::Linear,
            constraints: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
pub(crate) enum ModelWire {
    #[serde(rename = "ViennaRnafold")]
    ViennaRnafold,
    #[serde(rename = "ContraFold")]
    #[default]
    ContraFold,
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
pub(crate) enum DangleModelWire {
    #[serde(rename = "None")]
    None,
    #[serde(rename = "Both")]
    #[default]
    Both,
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
pub(crate) enum SequenceTopologyWire {
    #[serde(rename = "Linear")]
    #[default]
    Linear,
    #[serde(rename = "Circular")]
    Circular,
}

pub(crate) struct ParsedPredictionRequest {
    pub(crate) options: FoldOptions,
    pub(crate) constraints: Option<Vec<u8>>,
}

pub(crate) fn parse(input: &[u8]) -> serde_json::Result<ParsedPredictionRequest> {
    let request: PredictionRequest = serde_json::from_slice(input)?;
    let options = FoldOptions {
        model: request.model.into(),
        beam_size: request.beam_size,
        allow_sharp_turns: request.allow_sharp_turns,
        dangles: request.dangles.into(),
        topology: request.topology.into(),
    };
    Ok(ParsedPredictionRequest {
        options,
        constraints: request.constraints.map(String::into_bytes),
    })
}

impl From<ModelWire> for Model {
    fn from(model: ModelWire) -> Self {
        match model {
            ModelWire::ViennaRnafold => Self::ViennaRnafold,
            ModelWire::ContraFold => Self::ContraFold,
        }
    }
}

impl From<DangleModelWire> for DangleModel {
    fn from(dangles: DangleModelWire) -> Self {
        match dangles {
            DangleModelWire::None => Self::None,
            DangleModelWire::Both => Self::Both,
        }
    }
}

impl From<SequenceTopologyWire> for SequenceTopology {
    fn from(topology: SequenceTopologyWire) -> Self {
        match topology {
            SequenceTopologyWire::Linear => Self::Linear,
            SequenceTopologyWire::Circular => Self::Circular,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::prediction::{DangleModel, FoldOptions, Model, SequenceTopology};

    #[test]
    fn omitted_fields_match_fold_defaults() {
        let request = parse(b"{}").expect("empty request must use defaults");
        assert_eq!(request.options, FoldOptions::default());
        assert!(request.constraints.is_none());
    }

    #[test]
    fn wire_names_are_exact_and_unknown_fields_are_rejected() {
        // The remaining variant names. `every_wire_field_reaches_fold_options`
        // covers the others.
        let named = parse(br#"{"model":"ContraFold","dangles":"Both","topology":"Linear"}"#)
            .expect("wire names must parse");
        assert_eq!(named.options, FoldOptions::default());
        // Wire names are case-sensitive and exact. Lowercase and snake-case variants must fail.
        assert!(parse(br#"{"model":"viennaRnafold"}"#).is_err());
        assert!(parse(br#"{"model":"vienna_rnafold"}"#).is_err());
        assert!(parse(br#"{"dangles":"both"}"#).is_err());
        assert!(parse(br#"{"topology":"circular"}"#).is_err());
        // An unknown field is rejected rather than silently ignored.
        assert!(parse(br#"{"unknown":true}"#).is_err());
    }

    #[test]
    fn every_wire_field_reaches_fold_options() {
        let request = parse(
            br#"{
                "model":"ViennaRnafold",
                "beam_size":17,
                "allow_sharp_turns":true,
                "dangles":"None",
                "topology":"Circular",
                "constraints":"?(())"
            }"#,
        )
        .expect("all prediction wire fields must parse");

        assert_eq!(
            request.options,
            FoldOptions {
                model: Model::ViennaRnafold,
                beam_size: 17,
                allow_sharp_turns: true,
                dangles: DangleModel::None,
                topology: SequenceTopology::Circular,
            }
        );
        assert_eq!(request.constraints, Some(b"?(())".to_vec()));
    }
}
