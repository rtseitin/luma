use crate::element::Element;

/// Handle to a node inside a [`Document`](crate::Document).
///
/// Ids are plain indices into the document's arena. They are cheap to copy
/// and compare, and remain valid for the lifetime of the document. Detaching
/// a node does not invalidate its id; the node simply has no parent.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    /// Position of this node in the document's arena.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A node in the tree: its links to neighbouring nodes plus its payload.
#[derive(Debug, Clone)]
pub struct Node {
    pub(crate) parent: Option<NodeId>,
    pub(crate) first_child: Option<NodeId>,
    pub(crate) last_child: Option<NodeId>,
    pub(crate) prev_sibling: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
    /// What kind of node this is and its contents.
    pub data: NodeData,
}

impl Node {
    pub(crate) fn new(data: NodeData) -> Self {
        Self {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            data,
        }
    }

    /// Parent node, or `None` for the document root and detached nodes.
    #[inline]
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// First child, if any.
    #[inline]
    pub fn first_child(&self) -> Option<NodeId> {
        self.first_child
    }

    /// Last child, if any.
    #[inline]
    pub fn last_child(&self) -> Option<NodeId> {
        self.last_child
    }

    /// Previous sibling, if any.
    #[inline]
    pub fn prev_sibling(&self) -> Option<NodeId> {
        self.prev_sibling
    }

    /// Next sibling, if any.
    #[inline]
    pub fn next_sibling(&self) -> Option<NodeId> {
        self.next_sibling
    }

    /// Whether this node has any children.
    #[inline]
    pub fn has_children(&self) -> bool {
        self.first_child.is_some()
    }

    /// The element payload, if this is an element node.
    #[inline]
    pub fn as_element(&self) -> Option<&Element> {
        match &self.data {
            NodeData::Element(el) => Some(el),
            _ => None,
        }
    }

    /// Mutable element payload, if this is an element node.
    #[inline]
    pub fn as_element_mut(&mut self) -> Option<&mut Element> {
        match &mut self.data {
            NodeData::Element(el) => Some(el),
            _ => None,
        }
    }

    /// The text payload, if this is a text node.
    #[inline]
    pub fn as_text(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Whether this is an element node.
    #[inline]
    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element(_))
    }

    /// Whether this is a text node.
    #[inline]
    pub fn is_text(&self) -> bool {
        matches!(self.data, NodeData::Text(_))
    }
}

/// The payload of a [`Node`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeData {
    /// The root of the tree. Exactly one per document.
    Document,
    /// A `<!DOCTYPE ...>` declaration.
    Doctype {
        /// The doctype name, e.g. `html`.
        name: String,
    },
    /// An element such as `<div>`.
    Element(Element),
    /// A run of character data.
    Text(String),
    /// A `<!-- comment -->`.
    Comment(String),
}
