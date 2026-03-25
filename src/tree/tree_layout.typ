/// Returns the normalized children array for a tree node.
///
/// - node (dictionary): Tree node.
/// -> array
#let _tree-node-children(node) = {
  let children = node.at("children", default: none)
  if children == none { () } else { children }
}

/// Normalizes a node label for internal rendering.
///
/// - name (str, none): Raw node name.
/// -> str, none
#let _normalize-tree-label(name) = {
  if name == none or name == "" { none } else { name }
}

/// Converts a numeric node identifier into a dictionary key.
///
/// - id (int): Node identifier.
/// -> str
#let _tree-node-key(id) = str(id)

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

/// Validates the tree data structure recursively.
///
/// - node (dictionary): Tree node.
/// - is-root (bool): Whether this is the root node.
/// -> none
#let _validate-tree-data(node, is-root: true) = {
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

  if children != none {
    for child in children {
      _validate-tree-data(child, is-root: false)
    }
  }
}

/// Returns whether the tree contains any explicit non-root branch length.
///
/// - node (dictionary): Tree node.
/// - is-root (bool): Whether this is the input root node.
/// -> bool
#let _tree-has-explicit-non-root-length(node, is-root: true) = {
  if not is-root and node.at("length", default: none) != none {
    true
  } else {
    let children = _tree-node-children(node)
    children.any(child => _tree-has-explicit-non-root-length(
      child,
      is-root: false,
    ))
  }
}

/// Visits a tree node and appends normalized entries to the node table.
///
/// - node (dictionary): Current tree node.
/// - nodes (dictionary): String-keyed node table.
/// - next-id (int): Next available node identifier.
/// - cladogram (bool): Whether cladogram mode is enabled.
/// - parent-id (int, none): Parent node identifier.
/// - is-root (bool): Whether this node is the input root.
/// -> dictionary
#let _normalize-tree-node(
  node,
  nodes,
  next-id,
  cladogram: false,
  parent-id: none,
  is-root: false,
) = {
  let id = next-id
  let next-id = next-id + 1
  let nodes = nodes

  let children = _tree-node-children(node)
  let is-leaf = children.len() == 0
  let label-text = _normalize-tree-label(node.at("name", default: none))

  let entry = (
    id: id,
    parent-id: parent-id,
    children-ids: (),
    child-offsets: (),
    is-root: is-root,
    input-rooted: if is-root { node.at("rooted", default: false) } else {
      false
    },
    is-leaf: is-leaf,
    label-text: label-text,
    length: node.at("length", default: none),
    resolved-length: if cladogram {
      if is-root { 0.0 } else { 1.0 }
    } else if node.at("length", default: none) != none {
      float(node.length)
    } else {
      0.0
    },
    tip-count: 0,
    subtree-height: 0.0,
    y-local: 0.0,
    x-unit: 0.0,
    y-unit: 0.0,
  )
  nodes.insert(_tree-node-key(id), entry)

  let child-ids = ()
  for child in children {
    let result = _normalize-tree-node(
      child,
      nodes,
      next-id,
      cladogram: cladogram,
      parent-id: id,
    )
    child-ids.push(result.id)
    nodes = result.nodes
    next-id = result.next-id
  }

  let entry = nodes.at(_tree-node-key(id))
  entry.children-ids = child-ids
  nodes.insert(_tree-node-key(id), entry)
  (nodes: nodes, id: id, next-id: next-id)
}

/// Flattens a nested tree into a normalized node table.
///
/// - tree-data (dictionary): Tree data accepted by `render-tree`.
/// - cladogram (bool): Whether explicit cladogram mode is enabled.
/// -> dictionary
#let _normalize-tree(tree-data, cladogram: false) = {
  _validate-tree-data(tree-data)
  let has-explicit-non-root-length = _tree-has-explicit-non-root-length(
    tree-data,
  )
  let effective-cladogram = cladogram or not has-explicit-non-root-length

  let result = _normalize-tree-node(
    tree-data,
    (:),
    0,
    cladogram: effective-cladogram,
    is-root: true,
  )
  (
    nodes: result.nodes,
    root-id: result.id,
    node-count: result.next-id,
    effective-cladogram: effective-cladogram,
  )
}

/// Computes abstract rectangular coordinates for a normalized tree.
///
/// - normalized-tree (dictionary): Output from `_normalize-tree`.
/// -> dictionary
#let _layout-tree(normalized-tree) = {
  let nodes = normalized-tree.nodes
  let root-id = normalized-tree.root-id
  let node-count = normalized-tree.node-count

  for id in range(node-count).rev() {
    let node = nodes.at(_tree-node-key(id))
    if node.is-leaf {
      let updated = node
      updated.insert("tip-count", 1)
      updated.insert("subtree-height", 1.0)
      updated.insert("y-local", 0.5)
      updated.insert("child-offsets", ())
      nodes.insert(_tree-node-key(id), updated)
    } else {
      let subtree-height = 0.0
      let tip-count = 0
      let child-offsets = ()
      let first-center = none
      let last-center = none

      for child-id in node.children-ids {
        let child = nodes.at(_tree-node-key(child-id))
        child-offsets.push(subtree-height)
        let child-center = subtree-height + child.y-local
        if first-center == none { first-center = child-center }
        last-center = child-center
        subtree-height += child.subtree-height
        tip-count += child.tip-count
      }

      let updated = node
      updated.insert("tip-count", tip-count)
      updated.insert("subtree-height", subtree-height)
      updated.insert(
        "y-local",
        if first-center == none { 0.5 } else if last-center == none {
          first-center
        } else {
          (first-center + last-center) / 2.0
        },
      )
      updated.insert("child-offsets", child-offsets)
      nodes.insert(_tree-node-key(id), updated)
    }
  }

  let root = nodes.at(_tree-node-key(root-id))
  let root = root
  root.insert("x-unit", root.resolved-length)
  root.insert("y-unit", root.y-local)
  nodes.insert(_tree-node-key(root-id), root)

  let tree-depth = root.x-unit
  for id in range(node-count) {
    let node = nodes.at(_tree-node-key(id))
    tree-depth = calc.max(tree-depth, node.x-unit)

    let subtree-top = node.y-unit - node.y-local
    for (index, child-id) in node.children-ids.enumerate() {
      let child = nodes.at(_tree-node-key(child-id))
      let updated = child
      updated.insert("x-unit", node.x-unit + child.resolved-length)
      updated.insert(
        "y-unit",
        subtree-top + node.child-offsets.at(index) + child.y-local,
      )
      nodes.insert(_tree-node-key(child-id), updated)
      tree-depth = calc.max(tree-depth, updated.x-unit)
    }
  }

  (
    nodes: nodes,
    root-id: root-id,
    node-count: node-count,
    tree-depth: tree-depth,
    tree-height: nodes.at(_tree-node-key(root-id)).subtree-height,
  )
}
