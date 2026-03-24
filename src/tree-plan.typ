#import "tree-layout.typ": _tree-node-key

#let _zero-point = (x: 0pt, y: 0pt)

/// Builds a label content element from a tree label primitive.
///
/// - label-primitive (dictionary): Label primitive metadata.
/// -> content
#let _build-tree-label-content(label-primitive) = {
  text(
    size: label-primitive.text-size,
    fill: label-primitive.text-fill,
    style: label-primitive.text-style,
    bottom-edge: "descender",
  )[#label-primitive.text]
}

/// Builds a line primitive record.
///
/// - role (str): Primitive role.
/// - node-id (int): Related node identifier.
/// - stroke (stroke): Stroke styling.
/// - stroke-thickness (length): Stroke thickness used for bounds.
/// - tree-start (dictionary): Start point in tree space.
/// - tree-end (dictionary): End point in tree space.
/// - page-start (dictionary): Start offset in page space.
/// - page-end (dictionary): End offset in page space.
/// -> dictionary
#let _line-primitive(
  role,
  node-id,
  stroke,
  stroke-thickness,
  tree-start,
  tree-end,
  page-start: _zero-point,
  page-end: _zero-point,
) = {
  (
    kind: "line",
    role: role,
    unit-space: "mixed",
    node-id: node-id,
    stroke: stroke,
    stroke-thickness: stroke-thickness,
    tree-start: tree-start,
    tree-end: tree-end,
    page-start: page-start,
    page-end: page-end,
  )
}

/// Builds a label primitive record.
///
/// - role (str): Primitive role.
/// - node-id (int): Related node identifier.
/// - anchor-tree (dictionary): Anchor point in tree space.
/// - text (str): Label text.
/// - text-size (length): Font size.
/// - text-fill (color): Label color.
/// - text-style (str): Text style.
/// - rotation-mode (str): Rotation behavior during orientation transforms.
/// -> dictionary
#let _label-primitive(
  role,
  node-id,
  anchor-tree,
  text,
  text-size,
  text-fill,
  text-style,
  rotation-mode,
) = {
  (
    kind: "label",
    role: role,
    placement-role: role,
    unit-space: "mixed",
    node-id: node-id,
    anchor-tree: anchor-tree,
    anchor-page: _zero-point,
    text: text,
    text-size: text-size,
    text-fill: text-fill,
    text-style: text-style,
    rotation-mode: rotation-mode,
    rotation-origin: "top-left",
    measure-width: 0pt,
    measure-height: 0pt,
  )
}

/// Builds explicit tree primitives from a laid-out normalized tree.
///
/// - layout-tree (dictionary): Output from `_layout-tree`.
/// - style (dictionary): Tree styling configuration.
/// -> dictionary
#let _build-tree-plan(layout-tree, style) = {
  let primitives = ()
  let nodes = layout-tree.nodes
  let root = nodes.at(_tree-node-key(layout-tree.root-id))

  if root.input-rooted {
    primitives.push(_line-primitive(
      "root-edge",
      root.id,
      style.root-stroke,
      style.branch-weight,
      (x: root.x-unit, y: root.y-unit),
      (x: root.x-unit, y: root.y-unit),
      page-start: (x: -style.root-length, y: 0pt),
      page-end: _zero-point,
    ))
  }

  for id in range(layout-tree.node-count) {
    let node = nodes.at(_tree-node-key(id))
    let node-point = (x: node.x-unit, y: node.y-unit)

    if not node.is-root {
      let parent = nodes.at(_tree-node-key(node.parent-id))
      primitives.push(_line-primitive(
        "branch-horizontal",
        node.id,
        style.branch-stroke,
        style.branch-weight,
        (x: parent.x-unit, y: node.y-unit),
        node-point,
      ))
    }

    if node.is-leaf {
      if node.label-text != none {
        primitives.push(_label-primitive(
          "tip-label",
          node.id,
          node-point,
          node.label-text,
          style.tip-label-size,
          style.tip-label-color,
          if style.tip-label-italics { "italic" } else { "normal" },
          "rotate-with-tree",
        ))
      }
    } else {
      let first-child = nodes.at(_tree-node-key(node.children-ids.first()))
      let last-child = nodes.at(_tree-node-key(node.children-ids.last()))
      primitives.push(_line-primitive(
        "branch-vertical",
        node.id,
        style.branch-stroke,
        style.branch-weight,
        (x: node.x-unit, y: first-child.y-unit),
        (x: node.x-unit, y: last-child.y-unit),
      ))

      if node.label-text != none {
        primitives.push(_label-primitive(
          "internal-label",
          node.id,
          node-point,
          node.label-text,
          style.internal-label-size,
          style.internal-label-color,
          "normal",
          "stay-horizontal",
        ))
      }
    }
  }

  let plan = layout-tree
  plan.insert("tree-primitives", primitives)
  plan
}
