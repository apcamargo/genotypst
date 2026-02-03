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

fn convert_node_to_simple(
    tree: &NewickTree,
    node_id: usize,
    trim_quotes: bool,
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
                .map(|&child_id| convert_node_to_simple(tree, child_id, trim_quotes))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };

    // Get the node name and strip quotes if `trim_quotes` is true
    let name = match node.data().name.clone() {
        Some(s) if trim_quotes => Some(s.trim_matches(['"', '\'']).to_string()),
        other => other,
    };

    // Get the branch length (edge to parent)
    let length = node.branch().map(|&l| l as f64);

    Ok(SimpleTreeNode {
        name,
        length,
        children,
    })
}

#[wasm_func]
pub fn parse_newick(input: &[u8], trim_quotes: &[u8]) -> Result<Vec<u8>, String> {
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
    // Interpret the single-byte flag: nonzero means trim quotes, zero means keep them.
    let trim_quotes_bool = trim_quotes.first().copied().unwrap_or_default() != 0;
    let simple_tree = convert_node_to_simple(&tree, root_id, trim_quotes_bool)?;

    let result = ParseResult {
        rooted: is_rooted,
        tree: simple_tree,
    };

    serde_json::to_vec(&result).map_err(|e| e.to_string())
}
