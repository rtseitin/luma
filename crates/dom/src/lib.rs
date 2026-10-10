//! DOM tree representation for Luma.
//!
//! This crate owns the document tree and nothing else: no parsing, no
//! styling. The HTML parser writes into it; style and layout read from it.
//!
//! # Storage model
//!
//! Nodes live in a single arena inside [`Document`] and refer to each other
//! by [`NodeId`], a small `Copy` index. Downstream stages (style, layout)
//! keep their per-node data in side tables keyed by `NodeId` rather than
//! adding fields here, so this crate never needs to know about them.
//!
//! # Tolerance
//!
//! The DOM is deliberately permissive. Element and attribute names are not
//! validated, unknown tags are stored exactly as given, and nothing here can
//! fail on malformed input. Error recovery for broken HTML (unclosed tags,
//! misnested elements, stray end tags) is the HTML tree builder's job; its
//! output is always a well-formed tree, and this crate just stores it.

mod document;
mod element;
mod node;
mod traverse;

pub use document::Document;
pub use element::{Attribute, Element};
pub use node::{Node, NodeData, NodeId};
pub use traverse::{Ancestors, Children, Descendants};
