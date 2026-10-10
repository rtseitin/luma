//! HTML tokenization and tree construction for Luma.
//!
//! [`parse`] turns HTML source into a [`luma_dom::Document`]. The parser
//! follows the shape of the parsing algorithm in the [WHATWG HTML Living
//! Standard, section 13.2 "Parsing HTML documents"][spec]: a state-machine
//! [`Tokenizer`] produces [`Token`]s, and a tree builder driven by insertion
//! modes turns them into a tree. State and mode names match the spec's so
//! the two can be read side by side.
//!
//! [spec]: https://html.spec.whatwg.org/multipage/parsing.html
//!
//! # Error handling
//!
//! This parser never fails. Malformed input (unclosed tags,
//! stray end tags, misnested formatting, junk inside tags, truncated
//! comments) is recovered from according to the WHATWG spec's rules, and the
//! result is always a well-formed tree containing every piece of content
//! that could be interpreted. Problems are recorded as [`ParseError`]s,
//! which [`parse_with_errors`] returns alongside the document.
//!
//! # Simplifications
//!
//! This is a learning project, so several parts of the WHATWG algorithm are
//! reduced:
//!
//! - Tables are built with implied `tbody`/`tr` and cell auto-closing, but
//!   there are no table insertion modes and no foster parenting. Text
//!   between table tags stays where it is instead of being hoisted.
//! - No `frameset`, `template`, `select`, or ruby insertion modes; those
//!   elements are treated as ordinary elements.
//! - `svg` and `math` subtrees are parsed as ordinary HTML elements, except
//!   that self-closing tags are honoured inside them.
//! - `<script>` content is RAWTEXT. Scripts are never executed, so the
//!   `<!--` escaping states are omitted.
//! - Only a subset of named character references is known (Latin-1, common
//!   punctuation, arrows, math, Greek). Numeric references are complete.

mod entities;
mod tokenizer;
mod tree_builder;

pub use tokenizer::{TextMode, Token, Tokenizer};

use luma_dom::Document;

/// A recoverable problem found while parsing.
///
/// Errors never stop parsing; they are informational, mirroring the
/// spec's "parse error" notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Character offset into the (newline-normalized) source where the
    /// error was detected.
    pub offset: usize,
    /// Short identifier for the error, in the spec's `kebab-case` style
    /// where one exists (e.g. `eof-in-tag`, `unexpected-end-tag`).
    pub message: &'static str,
}

/// Parse an HTML document, discarding parse errors.
pub fn parse(source: &str) -> Document {
    parse_with_errors(source).0
}

/// Parse an HTML document and return the parse errors encountered.
///
/// The document is always complete; errors are purely diagnostic.
pub fn parse_with_errors(source: &str) -> (Document, Vec<ParseError>) {
    tree_builder::TreeBuilder::new(Tokenizer::new(source)).run()
}
