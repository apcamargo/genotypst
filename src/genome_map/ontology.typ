#let _rna-tail = (
  "($|_([a-z0-9_]*_)?(gene|region|transcript|intron|exon|cluster)$)"
)

#let _class-patterns = (
  (regex("pseudogen"), "pseudogene"),
  (
    regex(
      "cds|(^|_)mrna|codon|peptide|selenocysteine|pyrrolysine|protein_region"
        + "|orf$|^protein$|_gene_segment$|^[cdjnv]_(region|segment)$",
    ),
    "CDS",
  ),
  (regex("trna" + _rna-tail), "tRNA"),
  (regex("rrna" + _rna-tail), "rRNA"),
  (regex("rna" + _rna-tail + "|ribozyme"), "ncRNA"),
  (
    regex(
      "repeat|transpos|retroposon|mobile|insertion_sequence|integron|helitron"
        + "|satellite|crispr|(^|_)ltr(_|$)|(line|sine)_element$",
    ),
    "repeat_region",
  ),
  (
    regex(
      "promoter|terminator|enhancer|silencer|insulator|operator|regulatory"
        + "|attenuator|riboswitch|bind|ribosome_entry|^rbs$|shine_dalgarno"
        + "|transcription_start_site|polya|box|hypersensitive|^ori|rep_origin"
        + "|^ars$|control_region|^d[-_]loop$|_signal$",
    ),
    "regulatory_region",
  ),
)

#let _classify-feature-type(feature-type) = {
  let canonical = lower(feature-type)

  for (pattern, class) in _class-patterns {
    if canonical.contains(pattern) {
      return class
    }
  }

  none
}

#let _classify-biotype(biotype) = {
  if lower(biotype).starts-with("protein_coding") {
    return "CDS"
  }

  _classify-feature-type(biotype)
}
