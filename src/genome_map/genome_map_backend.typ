#let _genome-map-backend = plugin("genome_map.wasm")

/// Encodes prepositioned label geometry to the pt-based leader-routing wire format.
///
/// - payload (dictionary): Typst-positioned labels and shared leader bounds.
/// -> dictionary
#let _encode-genome-map-routing-request(payload) = (
  line_bottom_pt: payload.line-bottom / 1pt,
  labels: payload.labels.map(label => (
    level: label.level,
    hit_left_pt: label.hit-left / 1pt,
    hit_right_pt: label.hit-right / 1pt,
    query_x_pt: label.gene-center / 1pt,
    line_top_pt: label.underline-y / 1pt,
    raw_top_pt: label.raw-top / 1pt,
    raw_bottom_pt: label.raw-bottom / 1pt,
    block_top_pt: label.block-top / 1pt,
    block_bottom_pt: label.block-bottom / 1pt,
  )),
)

/// Decodes pt-based leader segments to Typst-native lengths.
///
/// - response (dictionary): Raw backend response.
/// -> dictionary
#let _decode-genome-map-routing-response(response) = (
  labels: response.labels.map(label => (
    leader-segments: label.leader_segments.map(segment => (
      top: segment.top_pt * 1pt,
      length: segment.length_pt * 1pt,
    )),
  )),
)

/// Routes leader segments for prepositioned genome-map labels through the WASM backend.
///
/// - payload (dictionary): Typst-positioned labels and shared leader bounds.
/// -> dictionary
#let _genome-map-route-leaders(payload) = {
  let result = _genome-map-backend.route_leaders(bytes(json.encode(
    _encode-genome-map-routing-request(payload),
  )))
  _decode-genome-map-routing-response(json(result))
}
