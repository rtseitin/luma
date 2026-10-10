use crate::document::Document;
use crate::node::NodeId;

/// Iterator over the direct children of a node, in order.
///
/// Created by [`Document::children`].
#[derive(Debug, Clone)]
pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl<'a> Children<'a> {
    pub(crate) fn new(doc: &'a Document, parent: NodeId) -> Self {
        Self {
            doc,
            next: doc[parent].first_child,
        }
    }
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc[current].next_sibling;
        Some(current)
    }
}

/// Iterator over the ancestors of a node, starting with its parent and
/// ending at the document root.
///
/// Created by [`Document::ancestors`].
#[derive(Debug, Clone)]
pub struct Ancestors<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl<'a> Ancestors<'a> {
    pub(crate) fn new(doc: &'a Document, node: NodeId) -> Self {
        Self {
            doc,
            next: doc[node].parent,
        }
    }
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc[current].parent;
        Some(current)
    }
}

/// Pre-order (document order) iterator over every node beneath a root,
/// not including the root itself.
///
/// Created by [`Document::descendants`].
#[derive(Debug, Clone)]
pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl<'a> Descendants<'a> {
    pub(crate) fn new(doc: &'a Document, root: NodeId) -> Self {
        Self {
            doc,
            root,
            next: doc[root].first_child,
        }
    }
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        let node = &self.doc[current];

        self.next = match node.first_child {
            Some(child) => Some(child),
            None => {
                // Climb until we find a next sibling, stopping at the root.
                let mut n = current;
                loop {
                    if n == self.root {
                        break None;
                    }
                    let nn = &self.doc[n];
                    if let Some(sib) = nn.next_sibling {
                        break Some(sib);
                    }
                    match nn.parent {
                        Some(p) => n = p,
                        None => break None,
                    }
                }
            }
        };

        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use crate::Document;

    /// Builds:
    /// ```text
    /// root
    /// ├── a
    /// │   ├── a1
    /// │   └── a2
    /// │       └── a2x
    /// ├── b
    /// └── c
    ///     └── c1
    /// ```
    fn fixture() -> (Document, Vec<crate::NodeId>) {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_element("a", vec![]);
        let a1 = doc.create_element("a1", vec![]);
        let a2 = doc.create_element("a2", vec![]);
        let a2x = doc.create_element("a2x", vec![]);
        let b = doc.create_element("b", vec![]);
        let c = doc.create_element("c", vec![]);
        let c1 = doc.create_element("c1", vec![]);
        doc.append_child(root, a);
        doc.append_child(a, a1);
        doc.append_child(a, a2);
        doc.append_child(a2, a2x);
        doc.append_child(root, b);
        doc.append_child(root, c);
        doc.append_child(c, c1);
        (doc, vec![a, a1, a2, a2x, b, c, c1])
    }

    fn names(doc: &Document, ids: impl Iterator<Item = crate::NodeId>) -> Vec<String> {
        ids.map(|id| doc.element(id).unwrap().name.clone())
            .collect()
    }

    #[test]
    fn children_in_order() {
        let (doc, _) = fixture();
        assert_eq!(names(&doc, doc.children(doc.root())), ["a", "b", "c"]);
    }

    #[test]
    fn children_of_leaf_is_empty() {
        let (doc, ids) = fixture();
        assert_eq!(doc.children(ids[1]).count(), 0);
    }

    #[test]
    fn descendants_pre_order_excludes_root() {
        let (doc, _) = fixture();
        assert_eq!(
            names(&doc, doc.descendants(doc.root())),
            ["a", "a1", "a2", "a2x", "b", "c", "c1"]
        );
    }

    #[test]
    fn descendants_of_subtree_stay_inside() {
        let (doc, ids) = fixture();
        let a = ids[0];
        assert_eq!(names(&doc, doc.descendants(a)), ["a1", "a2", "a2x"]);
        let c = ids[5];
        assert_eq!(names(&doc, doc.descendants(c)), ["c1"]);
    }

    #[test]
    fn ancestors_walk_to_root() {
        let (doc, ids) = fixture();
        let a2x = ids[3];
        let anc: Vec<_> = doc.ancestors(a2x).collect();
        assert_eq!(anc, [ids[2], ids[0], doc.root()]);
        assert_eq!(doc.ancestors(doc.root()).count(), 0);
    }
}
