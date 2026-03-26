/// Normalizes a node label for internal rendering.
///
/// - name (str, none): Raw node name.
/// -> str, none
#let _normalize-tree-label(name) = {
  if name == none or name == "" { none } else { name }
}

/// Validates a raw branch length value.
///
/// - length (int, float, none): Raw branch length.
/// -> none
#let _validate-tree-length(length) = {
  if length != none {
    let value = float(length)
    assert(
      not float.is-nan(value) and not float.is-infinite(value),
      message: "Node length must be a finite number or none",
    )
    assert(value >= 0.0, message: "Node length must be non-negative")
  }
}

/// Validates one tree node before normalization descends into its children.
///
/// - node (dictionary): Tree node.
/// - is-root (bool): Whether this is the root node.
/// -> none
#let _validate-tree-node(node, is-root: false) = {
  assert(type(node) == dictionary, message: "Tree nodes must be dictionaries")
  assert("children" in node, message: "Tree nodes must define children")

  if "name" in node {
    assert(
      node.name == none or type(node.name) == str,
      message: "Node name must be a string or none",
    )
  }

  if "length" in node {
    assert(
      node.length == none
        or type(node.length) == int
        or type(node.length) == float,
      message: "Node length must be a number or none",
    )
    _validate-tree-length(node.length)
  }

  if is-root and "rooted" in node {
    assert(type(node.rooted) == bool, message: "rooted must be a boolean")
  }

  let children = node.children
  assert(
    children == none or type(children) == array,
    message: "children must be an array or none",
  )
}

/// Visits a tree node and appends normalized entries to the node table.
///
/// - node (dictionary): Current tree node.
/// - nodes (dictionary): String-keyed node table.
/// - next-id (int): Next available node identifier.
/// - parent-id (int, none): Parent node identifier.
/// - is-root (bool): Whether this node is the input root.
/// -> dictionary
#let _normalize-tree-node(
  node,
  nodes,
  next-id,
  parent-id: none,
  is-root: false,
) = {
  _validate-tree-node(node, is-root: is-root)

  let id = next-id
  let next-id = next-id + 1
  let nodes = nodes

  let children = if node.children == none { () } else { node.children }
  let is-leaf = children.len() == 0
  let label-text = _normalize-tree-label(node.at("name", default: none))
  let length = node.at("length", default: none)

  let child-ids = ()
  let has_explicit_non_root_length = not is-root and length != none
  for child in children {
    let result = _normalize-tree-node(
      child,
      nodes,
      next-id,
      parent-id: id,
    )
    child-ids.push(result.id)
    nodes = result.nodes
    next-id = result.next-id
    has_explicit_non_root_length = (
      has_explicit_non_root_length or result.has-explicit-non-root-length
    )
  }

  nodes.insert(str(id), (
    id: id,
    parent-id: parent-id,
    children-ids: child-ids,
    is-root: is-root,
    input-rooted: if is-root { node.at("rooted", default: false) } else {
      false
    },
    is-leaf: is-leaf,
    label-text: label-text,
    length: length,
  ))
  return (
    nodes: nodes,
    id: id,
    next-id: next-id,
    has-explicit-non-root-length: has_explicit_non_root_length,
  )
}

/// Resolves normalized branch lengths once cladogram mode is known.
///
/// - nodes (dictionary): Normalized node table.
/// - node-keys (array): Cached string keys for node lookup.
/// - node-count (int): Number of nodes.
/// - effective-cladogram (bool): Whether cladogram fallback is active.
/// -> dictionary
#let _resolve-normalized-tree-lengths(
  nodes,
  node-keys,
  node-count,
  effective-cladogram,
) = {
  let nodes = nodes
  for id in range(node-count) {
    let key = node-keys.at(id)
    let node = nodes.at(key)
    let updated = node
    updated.insert(
      "resolved-length",
      if effective-cladogram {
        if node.is-root { 0.0 } else { 1.0 }
      } else if node.length != none {
        float(node.length)
      } else {
        0.0
      },
    )
    nodes.insert(key, updated)
  }
  nodes
}

/// Flattens a nested tree into a normalized node table.
///
/// - tree-data (dictionary): Tree data accepted by `render-tree`.
/// - cladogram (bool): Whether explicit cladogram mode is enabled.
/// -> dictionary
#let _normalize-tree(tree-data, cladogram: false) = {
  let result = _normalize-tree-node(
    tree-data,
    (:),
    0,
    is-root: true,
  )
  let node_count = result.next-id
  let node_keys = range(node_count).map(id => str(id))
  let effective_cladogram = cladogram or not result.has-explicit-non-root-length
  let nodes = _resolve-normalized-tree-lengths(
    result.nodes,
    node_keys,
    node_count,
    effective_cladogram,
  )
  (
    nodes: nodes,
    root-id: result.id,
    node-count: node_count,
    effective-cladogram: effective_cladogram,
    node-keys: node_keys,
  )
}

/// Computes abstract rectangular coordinates for a normalized tree.
///
/// The rendered root edge is controlled separately by `root-length`, so the
/// root node starts at metric depth zero here. Parsed/manual root-node lengths
/// stay preserved on the node records, but they do not contribute to
/// descendant branch geometry or auto scale-bar depth.
///
/// - normalized-tree (dictionary): Output from `_normalize-tree`.
/// -> dictionary
#let _layout-tree(normalized-tree) = {
  let nodes = normalized-tree.nodes
  let root-id = normalized-tree.root-id
  let node_count = normalized-tree.node-count
  let node_keys = normalized-tree.node-keys

  for id in range(node_count).rev() {
    let node_key = node_keys.at(id)
    let node = nodes.at(node_key)
    if node.is-leaf {
      let updated = node
      updated.insert("subtree-height", 1.0)
      updated.insert("y-local", 0.5)
      updated.insert("child-offsets", ())
      nodes.insert(node_key, updated)
    } else {
      let subtree-height = 0.0
      let child-offsets = ()
      let first-center = none
      let last-center = none

      for child-id in node.children-ids {
        let child = nodes.at(node_keys.at(child-id))
        child-offsets.push(subtree-height)
        let child-center = subtree-height + child.y-local
        if first-center == none { first-center = child-center }
        last-center = child-center
        subtree-height += child.subtree-height
      }

      let updated = node
      updated.insert("subtree-height", subtree-height)
      updated.insert(
        "y-local",
        (first-center + last-center) / 2.0,
      )
      updated.insert("child-offsets", child-offsets)
      nodes.insert(node_key, updated)
    }
  }

  let root_key = node_keys.at(root-id)
  let root = nodes.at(root_key)
  let root = root
  root.insert("x-unit", 0.0)
  root.insert("y-unit", root.y-local)
  nodes.insert(root_key, root)

  let tree-depth = 0.0
  for id in range(node_count) {
    let node = nodes.at(node_keys.at(id))
    tree-depth = calc.max(tree-depth, node.x-unit)

    let subtree-top = node.y-unit - node.y-local
    for (index, child-id) in node.children-ids.enumerate() {
      let child_key = node_keys.at(child-id)
      let child = nodes.at(child_key)
      let updated = child
      updated.insert("x-unit", node.x-unit + child.resolved-length)
      updated.insert(
        "y-unit",
        subtree-top + node.child-offsets.at(index) + child.y-local,
      )
      nodes.insert(child_key, updated)
      tree-depth = calc.max(tree-depth, updated.x-unit)
    }
  }

  (
    nodes: nodes,
    node-keys: node_keys,
    root-id: root-id,
    node-count: node_count,
    tree-depth: tree-depth,
    tree-height: root.subtree-height,
  )
}
