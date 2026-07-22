#import "./ontology.typ": _classify-biotype, _classify-feature-type

#let _biotype-attributes = ("gene_biotype", "biotype")

#let genome-map-palette = (
  "CDS": rgb("#56B4E9"),
  "tRNA": rgb("#7ad598"),
  "rRNA": rgb("#efad1d"),
  "ncRNA": rgb("#c98ce6"),
  "repeat_region": rgb("#ea6e70"),
  "regulatory_region": rgb("#F7ED6C"),
  "pseudogene": rgb("#84848e"),
)

#let _canonicalize-palette(palette) = {
  assert(
    type(palette) == dictionary,
    message: "palette must be a dictionary mapping feature types to colors.",
  )

  let prepared = (:)
  for (key, value) in palette.pairs() {
    assert(
      type(key) == str and key.len() > 0,
      message: "palette keys must be non-empty strings.",
    )
    assert(type(value) == color, message: "palette values must be colors.")

    let canonical-key = lower(key)
    if canonical-key in prepared {
      assert(
        prepared.at(canonical-key) == value,
        message: "Palette defines conflicting colors for feature types that "
          + "normalize to '"
          + canonical-key
          + "'.",
      )
      continue
    }
    prepared.insert(canonical-key, value)
  }

  prepared
}

#let _lookup-color(palette, name, classify: _classify-feature-type) = {
  let canonical = lower(name)
  if canonical in palette {
    return palette.at(canonical)
  }

  let class = classify(name)
  if class == none {
    return none
  }
  palette.at(lower(class), default: none)
}

#let _feature-biotype(feature) = {
  let attributes = feature.at("attributes", default: (:))
  if type(attributes) != dictionary {
    return none
  }

  for key in _biotype-attributes {
    let value = attributes.at(key, default: none)
    if type(value) == str {
      return value
    }
    if type(value) == array and value.len() > 0 {
      return value.first()
    }
  }

  none
}

/// colors features by their GFF3 "feature type"
#let _color-features(features, palette) = {
  let prepared = _canonicalize-palette(
    if palette == auto { genome-map-palette } else { palette },
  )

  let by-type = (:)
  let colored = ()

  for feature in features {
    let feature-type = feature.at("feature-type", default: none)
    if feature.at("color", default: none) != none or feature-type == none {
      colored.push(feature)
      continue
    }

    let fill-color = none

    if feature.at("pseudogenic", default: false) == true {
      fill-color = prepared.at("pseudogene", default: none)
    } else {
      if feature-type not in by-type {
        by-type.insert(feature-type, _lookup-color(prepared, feature-type))
      }
      fill-color = by-type.at(feature-type)

      if fill-color == none {
        let biotype = _feature-biotype(feature)
        if biotype != none {
          fill-color = _lookup-color(
            prepared,
            biotype,
            classify: _classify-biotype,
          )
        }
      }
    }

    colored.push(if fill-color == none {
      feature
    } else {
      (..feature, color: fill-color)
    })
  }

  colored
}

/// resolves the fill colors of a genome map's features
#let _resolve-gene-colors(genes, colors, palette) = {
  assert(type(colors) == bool, message: "colors must be a boolean.")
  assert(
    colors or palette == auto,
    message: "palette requires colors to be enabled.",
  )
  if not colors { return genes }

  assert(type(genes) == array, message: "genes must be an array.")
  assert(
    genes.len() == 0 or genes.any(gene => "feature-type" in gene),
    message: "colors needs the genes to carry a feature-type, as the ones "
      + "returned by parse-gff do. Give each gene a color of its own instead, "
      + "or leave colors off.",
  )

  _color-features(genes, palette)
}
