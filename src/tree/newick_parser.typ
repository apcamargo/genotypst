#import "./tree_backend.typ": _tree-parse-newick

/// Parses a Newick string into a tree structure.
///
/// Parses a string containing Newick-formatted phylogenetic tree data
/// into a dictionary structure suitable for rendering.
///
/// - data (str): A string containing the Newick data.
/// -> dictionary representing the root node with keys:
///   - children (array): Child node dictionaries.
///   - name (str, none): Optional node label.
///   - length (int, float, none): Optional branch length.
///   - rooted (bool, none): Optional root-only rootedness flag.
#let parse-newick(data) = {
  _tree-parse-newick(data)
}
