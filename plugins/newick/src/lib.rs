use newick::{NewickTree, one_from_string};
use serde::Serialize;
use wasm_minimal_protocol::*;

initiate_protocol!();

#[derive(Serialize)]
struct ParseResult {
    rooted: bool,
    #[serde(flatten)]
    tree: SimpleTreeNode,
}

#[derive(Serialize)]
struct SimpleTreeNode {
    name: Option<String>,
    length: Option<f64>,
    children: Option<Vec<SimpleTreeNode>>,
}

/// Check if the tree is rooted by examining the root node.
/// A tree is considered rooted if the root has exactly 2 children.
fn is_tree_rooted(tree: &NewickTree) -> bool {
    let root_id = tree.root();
    if let Ok(root_node) = tree.get(root_id) {
        root_node.children().len() == 2
    } else {
        false
    }
}

// In Newick, outer single quotes are delimiters, and a literal apostrophe is encoded as ''.
// We strip the delimiters and decode '' -> ' so labels render as their semantic text.
fn normalize_label(raw: &str) -> String {
    if let Some(inner) = raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        raw.to_owned()
    }
}

fn convert_node_to_simple(
    tree: &NewickTree,
    node_id: usize,
) -> Result<SimpleTreeNode, String> {
    let node = tree
        .get(node_id)
        .map_err(|e| format!("Failed to get node {}: {:?}", node_id, e))?;

    let children_ids = node.children();
    let children = if children_ids.is_empty() {
        None
    } else {
        Some(
            children_ids
                .iter()
                .map(|&child_id| convert_node_to_simple(tree, child_id))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };

    // Get the node name
    let name = node.data().name.as_deref().map(normalize_label);

    // Get the branch length (edge to parent)
    let length = node.branch().map(|&l| l as f64);

    Ok(SimpleTreeNode {
        name,
        length,
        children,
    })
}

#[wasm_func]
pub fn parse_newick(input: &[u8]) -> Result<Vec<u8>, String> {
    // Convert raw UTF-8 bytes into the Newick string input.
    let input_str = std::str::from_utf8(input)
        .map(|s| s.to_string())
        .map_err(|e| e.to_string())?;

    // Parse with newick crate
    let tree = one_from_string(&input_str)
        .map_err(|_| format!("Failed to parse Newick string: {input_str}"))?;

    // Check if rooted (root has exactly 2 children)
    let is_rooted = is_tree_rooted(&tree);

    // Get root node and convert to simple tree structure
    let root_id = tree.root();
    let simple_tree = convert_node_to_simple(&tree, root_id)?;

    let result = ParseResult {
        rooted: is_rooted,
        tree: simple_tree,
    };

    serde_json::to_vec(&result).map_err(|e| e.to_string())
}
