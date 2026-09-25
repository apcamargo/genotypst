/// Clamps a numeric value between bounds.
///
/// - value (length): Value to clamp.
/// - min (length): Lower bound.
/// - max (length): Upper bound.
/// -> length
#let _clamp(value, min, max) = {
  if value < min { min } else if value > max { max } else { value }
}

/// Clamps a centered label so it stays inside a track.
///
/// - center (length): Desired label center.
/// - label-width (length): Measured label width.
/// - left (length): Left edge of the track.
/// - extent (length): Track width.
/// -> length
#let _clamp-centered-label-left(center, label-width, left, extent) = _clamp(
  center - label-width / 2,
  left,
  calc.max(left, left + extent - label-width),
)

/// Resolves a potentially relative length to an absolute length.
///
/// Finite non-negative absolute lengths are returned directly, skipping the
/// layout pass. Everything else goes through `measure`, which floors negative
/// and infinite lengths to zero; use `_resolve-signed-length` to keep the sign.
///
/// - value (length, ratio, relative): Length value to resolve.
/// -> length
#let _resolve-length(value) = {
  if type(value) == length and value.em == 0.0 {
    let absolute = value.abs
    // Infinite widths reach here from inside `measure`, where the available
    // width is unbounded; they must keep falling through to the layout pass.
    if absolute >= 0pt and absolute.pt() < float.inf { return absolute }
  }
  measure(box(width: value)[]).width
}

/// Resolves one possibly signed length.
///
/// An em component is resolved before the sign is taken, since a mixed em and
/// absolute length cannot be compared against `0pt`.
///
/// - value (length): Length to resolve.
/// -> length
#let _resolve-signed-length(value) = {
  let resolved = if value.em == 0.0 {
    value.abs
  } else {
    value.abs + _resolve-length(1em) * value.em
  }
  if resolved < 0pt {
    -_resolve-length(-resolved)
  } else {
    _resolve-length(resolved)
  }
}

/// Asserts that a public argument is a non-negative or positive length.
///
/// The em component is resolved before the sign check, since a mixed em and
/// absolute length cannot be compared against `0pt`. Must be called in context.
///
/// - value (any): Public argument to validate.
/// - message (str): Error message for a non-length or out-of-range value.
/// - positive (bool): Whether zero is also rejected.
/// -> length
#let _assert-length(value, message, positive: false) = {
  assert(type(value) == length, message: message)
  let resolved = _resolve-signed-length(value)
  assert(
    if positive { resolved > 0pt } else { resolved >= 0pt },
    message: message,
  )
  resolved
}

/// Returns whether a public render width uses an accepted form.
///
/// Must be called in context, so em lengths can be resolved.
///
/// - width (length, auto, ratio, relative): Requested rendered width.
/// -> bool
#let _render-width-is-valid(width) = {
  if width == auto {
    true
  } else if type(width) == length {
    _resolve-signed-length(width) > 0pt
  } else if type(width) == ratio {
    width > 0%
  } else if type(width) == relative {
    width.ratio > 0% or _resolve-signed-length(width.length) > 0pt
  } else {
    false
  }
}

/// Classifies a public width for plugin fitting during layout.
///
/// A ratio or relative width is provisional while Typst measures it against
/// an unknown container width.
///
/// - width (length, auto, ratio, relative): Requested rendered width.
/// - raw-width (length): Measured width.
/// -> str: "auto", "provisional", or "resolved".
#let _width-mode(width, raw-width) = {
  if width == auto {
    "auto"
  } else if type(width) == ratio and width != 0% and raw-width == 0pt {
    "provisional"
  } else if (
    type(width) == relative
      and width.ratio != 0%
      and raw-width == _resolve-length(width.length)
  ) {
    "provisional"
  } else {
    "resolved"
  }
}
