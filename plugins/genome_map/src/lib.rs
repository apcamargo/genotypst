//! genome-map: Genome map layout and GFF3 parsing WASM plugin

mod gff;
mod layout;

#[cfg(target_arch = "wasm32")]
use wasm_minimal_protocol::*;

#[cfg(target_arch = "wasm32")]
initiate_protocol!();

#[cfg(target_arch = "wasm32")]
fn unsupported_getrandom(_: &mut [u8]) -> Result<(), getrandom::Error> {
    Err(getrandom::Error::UNSUPPORTED)
}

#[cfg(target_arch = "wasm32")]
getrandom::register_custom_getrandom!(unsupported_getrandom);

#[cfg_attr(target_arch = "wasm32", wasm_func)]
/// Full label layout pipeline: packing, vertical geometry, and leader routing.
///
/// Typst sends measured label geometry. Rust sorts by packing key, assigns
/// dodge levels with first-fit, computes vertical positions, routes leader
/// segments, and returns positioned labels.
pub fn layout_labels(config: &[u8]) -> Result<Vec<u8>, String> {
    let request: layout::LayoutRequest =
        serde_json::from_slice(config).map_err(|e| format!("Invalid config JSON: {e}"))?;
    let response = layout::compute_layout(&request)?;
    serde_json::to_vec(&response).map_err(|e| format!("Serialization failed: {e}"))
}

#[cfg_attr(target_arch = "wasm32", wasm_func)]
/// Parses GFF3 feature data into genome-map feature dictionaries.
///
/// # Arguments
/// * `data` - GFF3 source as UTF-8 bytes
/// * `config` - JSON-encoded parser filter configuration
///
/// # Returns
/// JSON bytes of genome-map-compatible feature dictionaries or an error string.
pub fn parse_gff(data: &[u8], config: &[u8]) -> Result<Vec<u8>, String> {
    gff::parse_gff(data, config)
}
