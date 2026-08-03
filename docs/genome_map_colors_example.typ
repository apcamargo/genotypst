#import "../src/lib.typ": *

#set page(
  fill: none,
  height: auto,
  width: 200mm,
  margin: (rest: 0cm, bottom: 1pt),
)

#let theme = sys.inputs.at("theme", default: "light")
#let text-color = if theme == "dark" { rgb("#f0f6fc") } else { rgb("#000000") }
#set text(font: "Source Sans 3", fill: text-color)
#set align(center)
#show raw: set text(font: "Source Code Pro", size: 9pt)

#let locus = parse-gff(
  read("data/NC_000913.gff3"),
  feature-types: ("CDS", "tRNA", "ncRNA", "pseudogene"),
  label-attribute: "gene",
)

#render-genome-map(
  locus,
  colors: true,
  coordinate-axis: true,
  unit: "bp",
  width: 90%,
  gene-outline-color: text-color,
)

#v(1em)

#let legend-classes = ("CDS", "tRNA", "ncRNA", "pseudogene")

#let legend-entry(class) = grid(
  columns: (auto, auto),
  column-gutter: 0.45em,
  align: horizon,
  circle(
    radius: 0.42em,
    fill: genome-map-palette.at(class),
    stroke: 0.7pt + text-color,
  ),
  raw(class),
)

#let legend(classes) = grid(
  columns: classes.len(),
  column-gutter: 1.8em,
  ..classes.map(legend-entry),
)

#legend(legend-classes)
