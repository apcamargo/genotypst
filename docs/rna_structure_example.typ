#import "../src/lib.typ": *

#set page(
  fill: none,
  height: auto,
  width: 200mm,
  margin: 0cm,
)

#let theme = sys.inputs.at("theme", default: "light")
#let text-color = if theme == "dark" { rgb("#f0f6fc") } else { rgb("#000000") }
#set text(font: "Source Sans 3", fill: text-color)
#set align(center)
#show raw: set text(font: "Source Code Pro", size: 9pt)

#let sequence = "GUACGGCUUCGAUUGAAUCCGUGAUGC"
#let prediction = predict-rna-structure(sequence)

#render-rna-structure(
  sequence,
  prediction.structure,
  width: 85mm,
  layout: "rna_puzzler",
  show-nucleotide-circles: true,
  show-terminal-labels: true,
  palette: residue-palette.rna.default,
  backbone-stroke: stroke(thickness: 1pt, paint: text-color),
)
