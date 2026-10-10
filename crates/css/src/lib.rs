//! CSS parsing for Luma.
//!
//! This crate is pure syntax: it turns stylesheet text into rules,
//! selectors, and declarations, and provides the property-agnostic value
//! types (lengths, colors) that every property grammar needs. It knows
//! nothing about the DOM or about what any particular property means;
//! selector matching and the cascade live in `luma-style`.
//!
//! The tokenizer and parser follow [CSS Syntax Module Level 3][syntax], and
//! selectors follow [Selectors Level 4][selectors]. Both specs define error
//! recovery precisely: a malformed declaration is skipped up to the next
//! `;`, a rule with an invalid selector is dropped, unclosed blocks are
//! closed at end of input. Nothing here ever fails; bad input shrinks the
//! stylesheet rather than rejecting it.
//!
//! [syntax]: https://www.w3.org/TR/css-syntax-3/
//! [selectors]: https://www.w3.org/TR/selectors-4/
//!
//! # Supported
//!
//! - Style rules with full selector lists: type, universal, `#id`,
//!   `.class`, all attribute operators (with `i` flag), the structural
//!   pseudo-classes (`:first-child`, `:nth-child(an+b)`, `-of-type`, ...),
//!   `:not()`, `:is()`, `:where()`, `:root`, `:empty`, link and form-state
//!   pseudo-classes, `::before`/`::after`, and all four combinators.
//! - `@media` with media types and `width`/`height` range features.
//! - `!important`.
//! - Lengths in all absolute and the common relative units; colors as hex,
//!   named, `rgb()`, `hsl()`, `transparent`, `currentcolor`.
//!
//! Other at-rules (`@import`, `@font-face`, `@keyframes`, ...) are parsed
//! and dropped with an error.

mod media;
mod parser;
mod selector;
mod tokenizer;
mod value;

pub use media::{MediaFeature, MediaQuery, MediaType, Range};
pub use parser::{
    BlockKind, ComponentValue, CssError, Declaration, MediaRule, Rule, StyleRule, Stylesheet,
};
pub use selector::{
    AttrOp, AttrSelector, Combinator, Compound, Nth, PseudoClass, PseudoElement, Selector,
    SimpleSelector, Specificity,
};
pub use tokenizer::{Token, Tokenizer, tokenize};
pub use value::{Color, ColorValue, Length, LengthPercentage, LengthUnit};

/// Parse a stylesheet, discarding errors.
pub fn parse_stylesheet(source: &str) -> Stylesheet {
    parse_stylesheet_with_errors(source).0
}

/// Parse a stylesheet and report the problems that were recovered from.
pub fn parse_stylesheet_with_errors(source: &str) -> (Stylesheet, Vec<CssError>) {
    parser::parse_stylesheet(source)
}

/// Parse the body of a `style="..."` attribute.
pub fn parse_declaration_list(source: &str) -> Vec<Declaration> {
    parser::parse_declaration_list(source).0
}

/// Parse a selector list such as `div > p.intro, #main`.
///
/// Returns `None` if any selector in the list is invalid, matching how a
/// rule with a bad selector is dropped as a whole.
pub fn parse_selector_list(source: &str) -> Option<Vec<Selector>> {
    let items: Vec<ComponentValue> = parser::component_values(source);
    selector::parse_selector_list(&items)
}
