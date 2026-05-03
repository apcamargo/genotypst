//! GFF3 parsing and filtering for genome-map inputs.

use bio::io::gff::{GffType, Reader, Record};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

#[derive(Deserialize)]
struct ParseGffConfig {
    #[serde(default)]
    feature_types: Option<Vec<String>>,
    #[serde(default)]
    range: Option<RangeFilter>,
    #[serde(default)]
    strand: Option<String>,
    #[serde(default)]
    exclude_partial: bool,
    #[serde(default = "default_label_attribute")]
    label_attribute: String,
}

#[derive(Deserialize)]
struct RangeFilter {
    accession: String,
    start: Option<u64>,
    end: Option<u64>,
}

#[derive(Clone, Copy)]
enum StrandFilter {
    Positive,
    Negative,
}

struct NormalizedRange {
    accession: String,
    start: u64,
    end: Option<u64>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Deserialize))]
struct GenomeMapFeature {
    start: u64,
    end: u64,
    strand: Option<i8>,
    label: Option<String>,
    partial: bool,
    accession: String,
    #[serde(rename = "feature-type")]
    feature_type: String,
    source: String,
    score: Option<f64>,
    phase: Option<u8>,
    attributes: BTreeMap<String, Vec<String>>,
    #[serde(rename = "original-start")]
    original_start: u64,
    #[serde(rename = "original-end")]
    original_end: u64,
}

impl ParseGffConfig {
    fn validate(&self) -> Result<(), String> {
        if let Some(feature_types) = &self.feature_types {
            for (index, feature_type) in feature_types.iter().enumerate() {
                if feature_type.is_empty() {
                    return Err(format!("feature_types[{index}] must not be empty"));
                }
            }
        }

        if let Some(range) = &self.range {
            range.validate()?;
        }

        self.strand_filter()?;
        if self.label_attribute.is_empty() {
            return Err("label_attribute must not be empty".into());
        }
        Ok(())
    }

    fn strand_filter(&self) -> Result<Option<StrandFilter>, String> {
        match self.strand.as_deref() {
            None => Ok(None),
            Some("positive") => Ok(Some(StrandFilter::Positive)),
            Some("negative") => Ok(Some(StrandFilter::Negative)),
            Some(other) => Err(format!(
                "strand must be null, 'positive', or 'negative'; got '{other}'"
            )),
        }
    }
}

fn default_label_attribute() -> String {
    "ID".into()
}

impl RangeFilter {
    fn validate(&self) -> Result<(), String> {
        if self.accession.is_empty() {
            return Err("range.accession must not be empty".into());
        }

        if self.start.is_some_and(|start| start == 0) {
            return Err("range.start must be >= 1".into());
        }
        if self.end.is_some_and(|end| end == 0) {
            return Err("range.end must be >= 1".into());
        }
        if let (Some(start), Some(end)) = (self.start, self.end)
            && start > end
        {
            return Err("range.start must be <= range.end".into());
        }

        Ok(())
    }
}

fn strip_fasta_tail(input: &str) -> String {
    let mut feature_text = String::with_capacity(input.len());
    for line in input.lines() {
        if line.trim_end() == "##FASTA" {
            break;
        }
        feature_text.push_str(line);
        feature_text.push('\n');
    }
    feature_text
}

fn build_feature_type_filter(feature_types: &Option<Vec<String>>) -> Option<BTreeSet<&str>> {
    feature_types
        .as_ref()
        .map(|types| types.iter().map(String::as_str).collect())
}

fn normalize_range(range: &Option<RangeFilter>) -> Option<NormalizedRange> {
    range.as_ref().map(|range| NormalizedRange {
        accession: range.accession.clone(),
        start: range.start.unwrap_or(1),
        end: range.end,
    })
}

fn selected_intersection(
    start: u64,
    end: u64,
    range: Option<&NormalizedRange>,
    exclude_partial: bool,
) -> Option<(u64, u64, bool)> {
    let Some(range) = range else {
        return Some((start, end, false));
    };

    let clipped_start = start.max(range.start);
    let clipped_end = end.min(range.end.unwrap_or(u64::MAX));
    if clipped_start > clipped_end {
        return None;
    }

    let partial = clipped_start != start || clipped_end != end;
    if exclude_partial && partial {
        return None;
    }
    Some((clipped_start, clipped_end, partial))
}

fn parse_strand(raw_strand: &str, record_index: usize) -> Result<Option<i8>, String> {
    match raw_strand {
        "+" => Ok(Some(1)),
        "-" => Ok(Some(-1)),
        "." | "?" => Ok(None),
        _ => Err(format!(
            "Invalid GFF3 strand value '{raw_strand}' in record {record_index}"
        )),
    }
}

fn matches_strand_filter(strand: Option<i8>, strand_filter: Option<StrandFilter>) -> bool {
    match strand_filter {
        None => true,
        Some(StrandFilter::Positive) => strand == Some(1),
        Some(StrandFilter::Negative) => strand == Some(-1),
    }
}

fn parse_score(raw_score: &str, record_index: usize) -> Result<Option<f64>, String> {
    if raw_score == "." {
        return Ok(None);
    }

    let score = raw_score
        .parse::<f64>()
        .map_err(|e| format!("Invalid GFF3 score '{raw_score}' in record {record_index}: {e}"))?;
    if !score.is_finite() {
        return Err(format!(
            "Invalid GFF3 score '{raw_score}' in record {record_index}: score must be finite"
        ));
    }

    Ok(Some(score))
}

fn phase_to_wire(record: &Record, record_index: usize) -> Result<Option<u8>, String> {
    record
        .phase()
        .clone()
        .try_into()
        .map_err(|_| format!("Invalid GFF3 phase in record {record_index}"))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_gff3_percent_escapes(input: &str, context: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    if !bytes.contains(&b'%') {
        return Ok(input.to_string());
    }

    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }

        if index + 2 >= bytes.len() {
            return Err(format!(
                "Invalid GFF3 percent escape in {context}: '%' must be followed by two hexadecimal digits"
            ));
        }

        let high = hex_value(bytes[index + 1]);
        let low = hex_value(bytes[index + 2]);
        let (Some(high), Some(low)) = (high, low) else {
            return Err(format!(
                "Invalid GFF3 percent escape in {context}: expected two hexadecimal digits"
            ));
        };

        decoded.push((high << 4) | low);
        index += 3;
    }

    String::from_utf8(decoded)
        .map_err(|e| format!("Invalid UTF-8 after decoding GFF3 percent escapes in {context}: {e}"))
}

fn decode_record_field(
    input: &str,
    record_index: usize,
    field_name: &str,
) -> Result<String, String> {
    decode_gff3_percent_escapes(input, &format!("record {record_index} {field_name}"))
}

fn record_attributes(
    record: &Record,
    record_index: usize,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut attributes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, values) in record.attributes().iter_all() {
        let decoded_key = decode_gff3_percent_escapes(
            key,
            &format!("record {record_index} attribute key '{key}'"),
        )?;
        let entry = attributes.entry(decoded_key).or_default();
        for (value_index, value) in values.iter().enumerate() {
            entry.push(decode_gff3_percent_escapes(
                value,
                &format!("record {record_index} attribute '{key}' value {value_index}"),
            )?);
        }
    }
    Ok(attributes)
}

fn first_attribute_value(attributes: &BTreeMap<String, Vec<String>>, key: &str) -> Option<String> {
    attributes
        .get(key)
        .and_then(|values| values.first())
        .cloned()
}

fn record_to_feature(
    record: &mut Record,
    record_index: usize,
    config: &ParseGffConfig,
    strand_filter: Option<StrandFilter>,
    feature_type_filter: Option<&BTreeSet<&str>>,
    range_filter: Option<&NormalizedRange>,
) -> Result<Option<GenomeMapFeature>, String> {
    let accession = decode_record_field(record.seqname(), record_index, "seqid")?;
    let feature_type = decode_record_field(record.feature_type(), record_index, "feature type")?;

    if feature_type_filter.is_some_and(|types| !types.contains(feature_type.as_str())) {
        return Ok(None);
    }

    // rust-bio's strand() is lossy: it collapses "?", ".", and invalid values.
    // The raw strand token is exposed only through strand_mut().
    let strand = parse_strand(record.strand_mut().as_str(), record_index)?;
    if !matches_strand_filter(strand, strand_filter) {
        return Ok(None);
    }

    if range_filter.is_some_and(|range| range.accession.as_str() != accession.as_str()) {
        return Ok(None);
    }

    let start = *record.start();
    let end = *record.end();
    if start == 0 || end == 0 {
        return Err(format!(
            "Invalid GFF3 coordinates in record {record_index}: start and end must be >= 1"
        ));
    }
    if start > end {
        return Err(format!(
            "Invalid GFF3 coordinates in record {record_index}: start must be <= end"
        ));
    }

    let Some((clipped_start, clipped_end, partial)) =
        selected_intersection(start, end, range_filter, config.exclude_partial)
    else {
        return Ok(None);
    };

    let source = decode_record_field(record.source(), record_index, "source")?;
    // rust-bio's score() parses only unsigned integers, but GFF3 scores may be
    // floating point. The raw score token is exposed only through score_mut().
    let score = parse_score(record.score_mut().as_str(), record_index)?;
    let phase = phase_to_wire(record, record_index)?;
    let attributes = record_attributes(record, record_index)?;
    let label = first_attribute_value(&attributes, &config.label_attribute);

    let feature = GenomeMapFeature {
        start: clipped_start,
        end: clipped_end,
        strand,
        label,
        partial,
        accession,
        feature_type,
        source,
        score,
        phase,
        attributes,
        original_start: start,
        original_end: end,
    };

    Ok(Some(feature))
}

fn parse_records(data: &str, config: &ParseGffConfig) -> Result<Vec<GenomeMapFeature>, String> {
    let feature_type_filter = build_feature_type_filter(&config.feature_types);
    let normalized_range = normalize_range(&config.range);
    let strand_filter = config.strand_filter()?;

    let feature_text = strip_fasta_tail(data);
    let cursor = Cursor::new(feature_text.as_bytes());
    let mut reader = Reader::new(cursor, GffType::GFF3);
    let mut features = Vec::new();

    for (record_offset, result) in reader.records().enumerate() {
        let record_index = record_offset + 1;
        let mut record =
            result.map_err(|e| format!("Failed to parse GFF3 record {record_index}: {e}"))?;
        if let Some(feature) = record_to_feature(
            &mut record,
            record_index,
            config,
            strand_filter,
            feature_type_filter.as_ref(),
            normalized_range.as_ref(),
        )? {
            features.push(feature);
        }
    }

    Ok(features)
}

pub fn parse_gff(data: &[u8], config: &[u8]) -> Result<Vec<u8>, String> {
    let data = std::str::from_utf8(data).map_err(|e| format!("Invalid UTF-8 in GFF3 data: {e}"))?;
    let config: ParseGffConfig =
        serde_json::from_slice(config).map_err(|e| format!("Invalid config JSON: {e}"))?;
    config.validate()?;

    let features = parse_records(data, &config)?;
    serde_json::to_vec(&features).map_err(|e| format!("Serialization failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const SIMPLE_GFF: &str = "\
##gff-version 3
chr1	src	gene	10	20	.	+	.	ID=gene1;Name=Alpha
chr1	src	CDS	12	18	3.5	+	0	ID=cds1;Parent=gene1
chr1	src	gene	30	40	.	-	.	ID=gene2;gene_name=Beta
chr1	src	exon	50	60	.	?	.	ID=exon1;tag=basic,canonical
chr2	src	gene	5	15	.	.	.	ID=gene3;locus_tag=Locus3
";

    fn config_json(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    fn parse_with_config(data: &str, config: Value) -> Vec<GenomeMapFeature> {
        let bytes = parse_gff(data.as_bytes(), &config_json(config)).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn default_config() -> Value {
        serde_json::json!({
            "feature_types": null,
            "range": null,
            "strand": null,
            "exclude_partial": false,
            "label_attribute": "ID"
        })
    }

    #[test]
    fn parses_all_features_with_labels_and_attributes() {
        let features = parse_with_config(SIMPLE_GFF, default_config());

        assert_eq!(features.len(), 5);
        assert_eq!(features[0].accession, "chr1");
        assert_eq!(features[0].feature_type, "gene");
        assert_eq!(features[0].label.as_deref(), Some("gene1"));
        assert_eq!(features[0].strand, Some(1));
        assert!(!features[0].partial);
        assert_eq!(features[1].score, Some(3.5));
        assert_eq!(features[1].phase, Some(0));
        assert_eq!(features[2].label.as_deref(), Some("gene2"));
        assert_eq!(features[2].strand, Some(-1));
        assert_eq!(features[3].strand, None);
        assert_eq!(
            features[3].attributes.get("tag").unwrap(),
            &vec!["basic".to_string(), "canonical".to_string()]
        );
        assert_eq!(features[4].label.as_deref(), Some("gene3"));
    }

    #[test]
    fn uses_requested_label_attribute_without_fallback() {
        let named = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": null,
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "Name"
            }),
        );
        let missing = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": null,
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "missing"
            }),
        );

        assert_eq!(named[0].label.as_deref(), Some("Alpha"));
        assert_eq!(named[1].label, None);
        assert!(missing.iter().all(|feature| feature.label.is_none()));
    }

    #[test]
    fn decodes_percent_escaped_attributes_and_labels() {
        let data = "\
##gff-version 3
chr1	src	CDS	1	10	.	+	0	ID=cds1;product=1%2C6%3Btest%3Dyes%26ok%25done%3dmid%3bsemi;tag=a,b%2Cc;plus=a+b;encoded%5Fkey=value%2Cok
";

        let features = parse_with_config(
            data,
            serde_json::json!({
                "feature_types": null,
                "range": null,
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "product"
            }),
        );

        let feature = &features[0];
        assert_eq!(
            feature.label.as_deref(),
            Some("1,6;test=yes&ok%done=mid;semi")
        );
        assert_eq!(
            feature.attributes.get("product").unwrap(),
            &vec!["1,6;test=yes&ok%done=mid;semi".to_string()]
        );
        assert_eq!(
            feature.attributes.get("tag").unwrap(),
            &vec!["a".to_string(), "b,c".to_string()]
        );
        assert_eq!(
            feature.attributes.get("plus").unwrap(),
            &vec!["a+b".to_string()]
        );
        assert_eq!(
            feature.attributes.get("encoded_key").unwrap(),
            &vec!["value,ok".to_string()]
        );
    }

    #[test]
    fn decodes_percent_escaped_record_fields_before_filtering() {
        let data = "\
##gff-version 3
chr%201	src%20db	gene%3Aspecial	1	10	.	+	.	ID=gene1
";

        let features = parse_with_config(
            data,
            serde_json::json!({
                "feature_types": ["gene:special"],
                "range": {"accession": "chr 1", "start": null, "end": null},
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "ID"
            }),
        );

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].accession, "chr 1");
        assert_eq!(features[0].feature_type, "gene:special");
        assert_eq!(features[0].source, "src db");
    }

    #[test]
    fn merges_attribute_values_when_decoded_keys_match() {
        let data = "\
##gff-version 3
chr1	src	gene	1	10	.	+	.	ID=gene1;same%5Fkey=a;same_key=b
";

        let features = parse_with_config(data, default_config());
        let mut values = features[0].attributes.get("same_key").unwrap().clone();
        values.sort();

        assert_eq!(values, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn defaults_label_attribute_to_id_when_omitted() {
        let features = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": null,
                "strand": null,
                "exclude_partial": false
            }),
        );

        assert_eq!(features[0].label.as_deref(), Some("gene1"));
    }

    #[test]
    fn filters_feature_types() {
        let features = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": null,
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "ID"
            }),
        );

        assert_eq!(features.len(), 3);
        assert!(
            features
                .iter()
                .all(|feature| feature.feature_type == "gene")
        );
    }

    #[test]
    fn filters_positive_and_negative_strands() {
        let positive = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": null,
                "range": null,
                "strand": "positive",
                "exclude_partial": false,
                "label_attribute": "ID"
            }),
        );
        let negative = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": null,
                "range": null,
                "strand": "negative",
                "exclude_partial": false,
                "label_attribute": "ID"
            }),
        );

        assert_eq!(positive.len(), 2);
        assert!(positive.iter().all(|feature| feature.strand == Some(1)));
        assert_eq!(negative.len(), 1);
        assert_eq!(negative[0].strand, Some(-1));
    }

    #[test]
    fn clips_features_to_range() {
        let features = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": {"accession": "chr1", "start": 15, "end": 35},
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "ID"
            }),
        );

        assert_eq!(features.len(), 2);
        assert_eq!((features[0].start, features[0].end), (15, 20));
        assert!(features[0].partial);
        assert_eq!(
            (features[0].original_start, features[0].original_end),
            (10, 20)
        );
        assert_eq!((features[1].start, features[1].end), (30, 35));
        assert!(features[1].partial);
    }

    #[test]
    fn excludes_partial_features_when_requested() {
        let features = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": {"accession": "chr1", "start": 11, "end": 39},
                "strand": null,
                "exclude_partial": true,
                "label_attribute": "ID"
            }),
        );

        assert!(features.is_empty());
    }

    #[test]
    fn supports_unbounded_range() {
        let features = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": {"accession": "chr2", "start": null, "end": null},
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "ID"
            }),
        );

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].accession, "chr2");
        assert_eq!((features[0].start, features[0].end), (5, 15));
        assert!(!features[0].partial);
    }

    #[test]
    fn skips_features_outside_range_accession() {
        let features = parse_with_config(
            SIMPLE_GFF,
            serde_json::json!({
                "feature_types": ["gene"],
                "range": {"accession": "missing", "start": null, "end": null},
                "strand": null,
                "exclude_partial": false,
                "label_attribute": "ID"
            }),
        );

        assert!(features.is_empty());
    }

    #[test]
    fn stops_at_fasta_tail() {
        let data = format!("{SIMPLE_GFF}##FASTA\n>chr1\nACGTACGT\nthis is not a feature row\n");

        let features = parse_with_config(&data, default_config());

        assert_eq!(features.len(), 5);
    }

    #[test]
    fn reports_invalid_strand_values() {
        let data = "##gff-version 3\nchr1\tsrc\tgene\t1\t10\t.\tx\t.\tID=bad\n";
        let error = parse_gff(data.as_bytes(), &config_json(default_config())).unwrap_err();

        assert!(error.contains("Invalid GFF3 strand value"));
    }

    #[test]
    fn reports_invalid_coordinates() {
        let data = "##gff-version 3\nchr1\tsrc\tgene\t20\t10\t.\t+\t.\tID=bad\n";
        let error = parse_gff(data.as_bytes(), &config_json(default_config())).unwrap_err();

        assert!(error.contains("start must be <= end"));
    }

    #[test]
    fn reports_invalid_percent_escapes_in_attributes() {
        for attributes in ["ID=bad;Note=bad%", "ID=bad;Note=bad%G1"] {
            let data = format!("##gff-version 3\nchr1\tsrc\tgene\t1\t10\t.\t+\t.\t{attributes}\n");
            let error = parse_gff(data.as_bytes(), &config_json(default_config())).unwrap_err();

            assert!(error.contains("Invalid GFF3 percent escape"));
        }
    }

    #[test]
    fn reports_invalid_percent_escapes_in_record_fields() {
        for row in [
            "chr%\tsrc\tgene\t1\t10\t.\t+\t.\tID=bad\n",
            "chr1\tsrc\tgene%G1\t1\t10\t.\t+\t.\tID=bad\n",
            "chr1\tsrc%\tgene\t1\t10\t.\t+\t.\tID=bad\n",
        ] {
            let data = format!("##gff-version 3\n{row}");
            let error = parse_gff(data.as_bytes(), &config_json(default_config())).unwrap_err();

            assert!(error.contains("Invalid GFF3 percent escape"));
        }
    }

    #[test]
    fn reports_invalid_utf8_after_attribute_decoding() {
        let data = "##gff-version 3\nchr1\tsrc\tgene\t1\t10\t.\t+\t.\tID=bad;Note=%FF\n";
        let error = parse_gff(data.as_bytes(), &config_json(default_config())).unwrap_err();

        assert!(error.contains("Invalid UTF-8 after decoding GFF3 percent escapes"));
    }

    #[test]
    fn reports_invalid_config_json() {
        let error = parse_gff(SIMPLE_GFF.as_bytes(), b"{").unwrap_err();

        assert!(error.contains("Invalid config JSON"));
    }
}
