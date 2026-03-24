#let _light-gray = oklch(85%, 0.012, 264.5deg)
#let _medium-gray = oklch(68%, 0.012, 264.5deg)
#let _dark-gray = oklch(37.5%, 0.012, 264.5deg)
#let _yellow = oklch(87.5%, 0.165, 93.9deg)

#let _diverging-gradient = (
  gradient
    .linear(
      rgb("#d9353c"),
      rgb("#f3634c"),
      rgb("#ff936d"),
      rgb("#f9c2a6"),
      rgb("#dedfe0"),
      rgb("#a9dbed"),
      rgb("#72c3e1"),
      rgb("#57a2d0"),
      rgb("#5b79c0"),
    )
    .sharp(256)
    .stops()
)
