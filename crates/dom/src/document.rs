use std::fmt::Write as _;
use std::ops::{Index, IndexMut};

use crate::element::{Attribute, Element};
use crate::node::{Node, NodeData, NodeId};
use crate::traverse::{Ancestors, Children, Descendants};

/// An arena-backed document tree.
///
/// All nodes are stored in one `Vec`; links between them are [`NodeId`]
/// indices. Nodes are never freed: detaching a node unlinks it from the tree
/// but keeps its slot, so every `NodeId` ever handed out stays valid.
#[derive(Debug, Clone)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Create an empty document containing only the root node.
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::new(NodeData::Document)],
            root: NodeId(0),
        }
    }

    /// The root node. Its data is always [`NodeData::Document`].
    #[inline]
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Total number of nodes ever allocated, including detached ones.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // ----- allocation -------------------------------------------------------

    fn alloc(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("document exceeded u32::MAX nodes"));
        self.nodes.push(Node::new(data));
        id
    }

    /// Allocate a detached element node. Any tag name is accepted.
    pub fn create_element(&mut self, name: &str, attrs: Vec<Attribute>) -> NodeId {
        self.alloc(NodeData::Element(Element::new(name, attrs)))
    }

    /// Allocate a detached text node.
    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.alloc(NodeData::Text(text.to_owned()))
    }

    /// Allocate a detached comment node.
    pub fn create_comment(&mut self, text: &str) -> NodeId {
        self.alloc(NodeData::Comment(text.to_owned()))
    }

    /// Allocate a detached doctype node.
    pub fn create_doctype(&mut self, name: &str) -> NodeId {
        self.alloc(NodeData::Doctype {
            name: name.to_owned(),
        })
    }

    // ----- access -----------------------------------------------------------

    /// Borrow a node.
    #[inline]
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Mutably borrow a node.
    #[inline]
    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    /// Borrow the element payload of a node, if it is an element.
    #[inline]
    pub fn element(&self, id: NodeId) -> Option<&Element> {
        self.get(id).as_element()
    }

    /// Mutably borrow the element payload of a node, if it is an element.
    #[inline]
    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        self.get_mut(id).as_element_mut()
    }

    // ----- traversal --------------------------------------------------------

    /// Direct children of a node, in order.
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children::new(self, id)
    }

    /// All nodes beneath `id` in document (pre-) order, excluding `id`.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants::new(self, id)
    }

    /// Parent, grandparent, ... up to the root.
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors::new(self, id)
    }

    /// Concatenated text of every text node beneath `id`.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        if let Some(t) = self.get(id).as_text() {
            out.push_str(t);
        }
        for d in self.descendants(id) {
            if let Some(t) = self.get(d).as_text() {
                out.push_str(t);
            }
        }
        out
    }

    // ----- mutation ---------------------------------------------------------

    /// Append `child` as the last child of `parent`.
    ///
    /// If `child` is already in the tree it is detached from its current
    /// position first, so this also serves as "move".
    ///
    /// # Panics
    ///
    /// In debug builds, if `child` is `parent` or one of its ancestors
    /// (which would create a cycle).
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        debug_assert!(child != parent, "cannot append a node to itself");
        debug_assert!(
            !self.ancestors(parent).any(|a| a == child),
            "cannot append an ancestor beneath its own descendant"
        );

        self.detach(child);

        let prev = self.nodes[parent.index()].last_child;
        {
            let c = &mut self.nodes[child.index()];
            c.parent = Some(parent);
            c.prev_sibling = prev;
            c.next_sibling = None;
        }
        match prev {
            Some(p) => self.nodes[p.index()].next_sibling = Some(child),
            None => self.nodes[parent.index()].first_child = Some(child),
        }
        self.nodes[parent.index()].last_child = Some(child);
    }

    /// Insert `new` as a child of `parent`, immediately before `before`.
    ///
    /// If `new` is already in the tree it is detached first.
    ///
    /// # Panics
    ///
    /// If `before` is not a child of `parent`. In debug builds, also if
    /// `new` is `parent` or one of its ancestors.
    pub fn insert_before(&mut self, parent: NodeId, new: NodeId, before: NodeId) {
        assert_eq!(
            self.nodes[before.index()].parent,
            Some(parent),
            "insert_before: reference node is not a child of parent"
        );
        debug_assert!(new != parent, "cannot insert a node into itself");
        debug_assert!(
            !self.ancestors(parent).any(|a| a == new),
            "cannot insert an ancestor beneath its own descendant"
        );
        if new == before {
            return;
        }

        self.detach(new);

        let prev = self.nodes[before.index()].prev_sibling;
        {
            let n = &mut self.nodes[new.index()];
            n.parent = Some(parent);
            n.prev_sibling = prev;
            n.next_sibling = Some(before);
        }
        self.nodes[before.index()].prev_sibling = Some(new);
        match prev {
            Some(p) => self.nodes[p.index()].next_sibling = Some(new),
            None => self.nodes[parent.index()].first_child = Some(new),
        }
    }

    /// Unlink a node from its parent and siblings. Its subtree stays intact
    /// and the node remains addressable by its id. Detaching the root or an
    /// already-detached node is a no-op.
    pub fn detach(&mut self, node: NodeId) {
        let (parent, prev, next) = {
            let n = &self.nodes[node.index()];
            (n.parent, n.prev_sibling, n.next_sibling)
        };
        let Some(parent) = parent else {
            return;
        };

        match prev {
            Some(p) => self.nodes[p.index()].next_sibling = next,
            None => self.nodes[parent.index()].first_child = next,
        }
        match next {
            Some(n) => self.nodes[n.index()].prev_sibling = prev,
            None => self.nodes[parent.index()].last_child = prev,
        }

        let n = &mut self.nodes[node.index()];
        n.parent = None;
        n.prev_sibling = None;
        n.next_sibling = None;
    }

    /// Append character data to `parent`.
    ///
    /// If the last child of `parent` is already a text node, the text is
    /// merged into it (as the HTML spec requires for adjacent character
    /// tokens). Otherwise a new text node is created and appended. Returns
    /// the id of the text node that received the data.
    pub fn append_text(&mut self, parent: NodeId, text: &str) -> NodeId {
        if let Some(last) = self.nodes[parent.index()].last_child
            && let NodeData::Text(existing) = &mut self.nodes[last.index()].data
        {
            existing.push_str(text);
            return last;
        }
        let id = self.create_text(text);
        self.append_child(parent, id);
        id
    }

    // ----- debugging --------------------------------------------------------

    /// Render the tree in the html5lib-tests "tree construction" format.
    ///
    /// ```text
    /// #document
    /// | <!DOCTYPE html>
    /// | <html>
    /// |   <body>
    /// |     <p>
    /// |       class="intro"
    /// |       "Hello"
    /// ```
    ///
    /// Attributes are listed sorted by name, matching html5lib, so parser
    /// tests can diff against upstream fixtures.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.dump_node(self.root, 0, &mut out);
        out
    }

    fn dump_node(&self, id: NodeId, depth: usize, out: &mut String) {
        let node = &self.nodes[id.index()];

        if depth == 0 {
            out.push_str("#document\n");
        } else {
            let indent = "  ".repeat(depth - 1);
            let _ = write!(out, "| {indent}");
            match &node.data {
                NodeData::Document => out.push_str("#document"),
                NodeData::Doctype { name } => {
                    let _ = write!(out, "<!DOCTYPE {name}>");
                }
                NodeData::Element(el) => {
                    let _ = write!(out, "<{}>", el.name);
                    let mut attrs: Vec<&Attribute> = el.attrs.iter().collect();
                    attrs.sort_by(|a, b| a.name.cmp(&b.name));
                    for a in attrs {
                        let _ = write!(out, "\n| {indent}  {}=\"{}\"", a.name, a.value);
                    }
                }
                NodeData::Text(t) => {
                    let _ = write!(out, "\"{t}\"");
                }
                NodeData::Comment(c) => {
                    let _ = write!(out, "<!-- {c} -->");
                }
            }
            out.push('\n');
        }

        for child in self.children(id) {
            self.dump_node(child, depth + 1, out);
        }
    }
}

impl Index<NodeId> for Document {
    type Output = Node;

    #[inline]
    fn index(&self, id: NodeId) -> &Node {
        self.get(id)
    }
}

impl IndexMut<NodeId> for Document {
    #[inline]
    fn index_mut(&mut self, id: NodeId) -> &mut Node {
        self.get_mut(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk every reachable node and check that parent/child/sibling links
    /// agree with each other.
    fn assert_consistent(doc: &Document) {
        fn check(doc: &Document, id: NodeId) {
            let node = &doc[id];
            let kids: Vec<NodeId> = doc.children(id).collect();

            assert_eq!(
                node.first_child,
                kids.first().copied(),
                "first_child of {id:?}"
            );
            assert_eq!(
                node.last_child,
                kids.last().copied(),
                "last_child of {id:?}"
            );

            for (i, &k) in kids.iter().enumerate() {
                let kn = &doc[k];
                assert_eq!(kn.parent, Some(id), "parent of {k:?}");
                let expected_prev = if i > 0 { Some(kids[i - 1]) } else { None };
                let expected_next = kids.get(i + 1).copied();
                assert_eq!(kn.prev_sibling, expected_prev, "prev_sibling of {k:?}");
                assert_eq!(kn.next_sibling, expected_next, "next_sibling of {k:?}");
                check(doc, k);
            }
        }
        let root = doc.root();
        assert_eq!(doc[root].parent, None);
        assert_eq!(doc[root].prev_sibling, None);
        assert_eq!(doc[root].next_sibling, None);
        check(doc, root);
    }

    fn names(doc: &Document, parent: NodeId) -> Vec<String> {
        doc.children(parent)
            .map(|c| match &doc[c].data {
                NodeData::Element(e) => e.name.clone(),
                NodeData::Text(t) => format!("\"{t}\""),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn new_document_has_only_root() {
        let doc = Document::new();
        assert_eq!(doc.node_count(), 1);
        assert_eq!(doc[doc.root()].data, NodeData::Document);
        assert_eq!(doc.children(doc.root()).count(), 0);
        assert_consistent(&doc);
    }

    #[test]
    fn append_child_links() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_element("a", vec![]);
        let b = doc.create_element("b", vec![]);
        let c = doc.create_element("c", vec![]);
        doc.append_child(root, a);
        doc.append_child(root, b);
        doc.append_child(root, c);

        assert_eq!(names(&doc, root), ["a", "b", "c"]);
        assert_eq!(doc[a].prev_sibling(), None);
        assert_eq!(doc[a].next_sibling(), Some(b));
        assert_eq!(doc[c].next_sibling(), None);
        assert_consistent(&doc);
    }

    #[test]
    fn append_child_moves_existing_node() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_element("a", vec![]);
        let b = doc.create_element("b", vec![]);
        let c = doc.create_element("c", vec![]);
        doc.append_child(root, a);
        doc.append_child(root, b);
        doc.append_child(root, c);

        // Move `a` to the end.
        doc.append_child(root, a);
        assert_eq!(names(&doc, root), ["b", "c", "a"]);
        assert_consistent(&doc);

        // Move `b` under `c` (reparent).
        doc.append_child(c, b);
        assert_eq!(names(&doc, root), ["c", "a"]);
        assert_eq!(names(&doc, c), ["b"]);
        assert_consistent(&doc);
    }

    #[test]
    fn insert_before_positions() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_element("a", vec![]);
        let c = doc.create_element("c", vec![]);
        doc.append_child(root, a);
        doc.append_child(root, c);

        // Middle.
        let b = doc.create_element("b", vec![]);
        doc.insert_before(root, b, c);
        assert_eq!(names(&doc, root), ["a", "b", "c"]);
        assert_consistent(&doc);

        // Front.
        let z = doc.create_element("z", vec![]);
        doc.insert_before(root, z, a);
        assert_eq!(names(&doc, root), ["z", "a", "b", "c"]);
        assert_consistent(&doc);

        // Moving a node before itself is a no-op.
        doc.insert_before(root, b, b);
        assert_eq!(names(&doc, root), ["z", "a", "b", "c"]);
        assert_consistent(&doc);

        // Moving an existing node.
        doc.insert_before(root, c, z);
        assert_eq!(names(&doc, root), ["c", "z", "a", "b"]);
        assert_consistent(&doc);
    }

    #[test]
    #[should_panic(expected = "not a child of parent")]
    fn insert_before_rejects_foreign_reference() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_element("a", vec![]);
        let b = doc.create_element("b", vec![]);
        doc.append_child(root, a);
        doc.append_child(a, b);
        let x = doc.create_element("x", vec![]);
        doc.insert_before(root, x, b); // b's parent is a, not root
    }

    #[test]
    fn detach_middle_first_last_and_only() {
        let mut doc = Document::new();
        let root = doc.root();
        let ids: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|n| {
                let id = doc.create_element(n, vec![]);
                doc.append_child(root, id);
                id
            })
            .collect();

        doc.detach(ids[1]); // middle
        assert_eq!(names(&doc, root), ["a", "c", "d"]);
        assert_consistent(&doc);

        doc.detach(ids[0]); // first
        assert_eq!(names(&doc, root), ["c", "d"]);
        assert_consistent(&doc);

        doc.detach(ids[3]); // last
        assert_eq!(names(&doc, root), ["c"]);
        assert_consistent(&doc);

        doc.detach(ids[2]); // only
        assert_eq!(names(&doc, root), Vec::<String>::new());
        assert_consistent(&doc);

        // Detached node has no links but is still addressable.
        let b = &doc[ids[1]];
        assert_eq!(b.parent(), None);
        assert_eq!(b.prev_sibling(), None);
        assert_eq!(b.next_sibling(), None);
        assert_eq!(doc.element(ids[1]).unwrap().name, "b");

        // Detaching again, or detaching the root, is harmless.
        doc.detach(ids[1]);
        doc.detach(root);
        assert_consistent(&doc);
    }

    #[test]
    fn detach_keeps_subtree() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_element("a", vec![]);
        let a1 = doc.create_element("a1", vec![]);
        doc.append_child(root, a);
        doc.append_child(a, a1);

        doc.detach(a);
        assert_eq!(doc.children(root).count(), 0);
        assert_eq!(doc[a1].parent(), Some(a));
        assert_eq!(names(&doc, a), ["a1"]);
    }

    #[test]
    fn append_text_merges_adjacent_runs() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = doc.create_element("p", vec![]);
        doc.append_child(root, p);

        let t1 = doc.append_text(p, "Hello");
        let t2 = doc.append_text(p, ", ");
        let t3 = doc.append_text(p, "world");
        assert_eq!(t1, t2);
        assert_eq!(t2, t3);
        assert_eq!(names(&doc, p), ["\"Hello, world\""]);

        // An element in between breaks the run.
        let br = doc.create_element("br", vec![]);
        doc.append_child(p, br);
        let t4 = doc.append_text(p, "!");
        assert_ne!(t4, t1);
        assert_eq!(names(&doc, p), ["\"Hello, world\"", "br", "\"!\""]);
        assert_consistent(&doc);
    }

    #[test]
    fn text_content_concatenates_descendants() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = doc.create_element("p", vec![]);
        let b = doc.create_element("b", vec![]);
        doc.append_child(root, p);
        doc.append_text(p, "a ");
        doc.append_child(p, b);
        doc.append_text(b, "b");
        doc.append_text(p, " c");
        let t = doc.append_text(p, "");
        assert_eq!(doc.text_content(p), "a b c");
        assert_eq!(doc.text_content(root), "a b c");
        assert_eq!(doc.text_content(t), " c");
        assert_eq!(doc.text_content(b), "b");
    }

    #[test]
    fn unknown_tags_are_stored_verbatim() {
        let mut doc = Document::new();
        let root = doc.root();
        let weird = doc.create_element("not-a-real-tag", vec![Attribute::new("x-y", "1")]);
        doc.append_child(root, weird);
        let el = doc.element(weird).unwrap();
        assert_eq!(el.name, "not-a-real-tag");
        assert_eq!(el.attr("x-y"), Some("1"));
    }

    #[test]
    fn index_operators() {
        let mut doc = Document::new();
        let p = doc.create_element("p", vec![]);
        doc[p].as_element_mut().unwrap().set_attr("id", "x");
        assert_eq!(doc[p].as_element().unwrap().id(), Some("x"));
    }

    #[test]
    fn dump_matches_html5lib_format() {
        let mut doc = Document::new();
        let root = doc.root();
        let doctype = doc.create_doctype("html");
        let html = doc.create_element("html", vec![]);
        let head = doc.create_element("head", vec![]);
        let body = doc.create_element("body", vec![]);
        let p = doc.create_element(
            "p",
            vec![Attribute::new("id", "x"), Attribute::new("class", "intro")],
        );
        let comment = doc.create_comment(" note ");
        doc.append_child(root, doctype);
        doc.append_child(root, html);
        doc.append_child(html, head);
        doc.append_child(html, body);
        doc.append_child(body, p);
        doc.append_text(p, "Hello");
        doc.append_child(body, comment);

        let expected = "\
#document
| <!DOCTYPE html>
| <html>
|   <head>
|   <body>
|     <p>
|       class=\"intro\"
|       id=\"x\"
|       \"Hello\"
|     <!--  note  -->
";
        assert_eq!(doc.dump(), expected);
    }
}
