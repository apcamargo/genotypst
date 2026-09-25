use super::{Prediction, PredictionScore};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct PredictionResponse {
    structure: String,
    score: PredictionScoreResponse,
}

#[derive(Debug, Serialize)]
struct PredictionScoreResponse {
    model: &'static str,
    value: f64,
}

impl From<Prediction> for PredictionResponse {
    fn from(prediction: Prediction) -> Self {
        let score = match prediction.score {
            PredictionScore::ViennaRnafold(value) => PredictionScoreResponse {
                model: "ViennaRnafold",
                value,
            },
            PredictionScore::ContraFold(value) => PredictionScoreResponse {
                model: "ContraFold",
                value,
            },
        };
        Self {
            structure: prediction.structure,
            score,
        }
    }
}

pub(crate) fn serialize(prediction: Prediction) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&PredictionResponse::from(prediction))
}

#[cfg(test)]
mod tests {
    use super::serialize;
    use crate::prediction::{Prediction, PredictionScore};

    #[test]
    fn serializes_model_specific_scores() {
        let cases = [
            (
                PredictionScore::ViennaRnafold(-1.8),
                r#"{"structure":".()","score":{"model":"ViennaRnafold","value":-1.8}}"#,
            ),
            (
                PredictionScore::ContraFold(2.5),
                r#"{"structure":".()","score":{"model":"ContraFold","value":2.5}}"#,
            ),
        ];
        for (score, expected) in cases {
            let output = serialize(Prediction {
                structure: ".()".to_owned(),
                score,
            })
            .expect("prediction response must serialize");
            assert_eq!(
                String::from_utf8(output).expect("JSON must be UTF-8"),
                expected
            );
        }
    }
}
