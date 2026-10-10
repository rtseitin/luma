//! Rule and declaration parsing, following CSS Syntax Module Level 3 §5.
//!
//! Tokens are first grouped into component values (tokens plus nested
//! `{}`/`[]`/`()` blocks and functions); rules, selectors, and declarations
//! are then parsed from those.

use crate::media::{self, MediaQuery};
use crate::selector::{self, Selector};
use crate::tokenizer::{Token, tokenize};

/// Which bracket pair a block uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// `{ ... }`
    Curly,
    /// `[ ... ]`
    Square,
    /// `( ... )`
    Paren,
}

/// A token or a nested structure, the unit that preludes and declaration
/// values are made of.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentValue {
    /// A plain token (never an opening bracket or function token).
    Token(Token),
    /// `name(args)`.
    Function {
        /// Function name as written.
        name: String,
        /// Arguments, including whitespace and comma tokens.
        args: Vec<ComponentValue>,
    },
    /// A bracketed block.
    Block {
        /// Bracket type.
        kind: BlockKind,
        /// Contents, including whitespace tokens.
        content: Vec<ComponentValue>,
    },
}

impl ComponentValue {
    /// Whether this is a whitespace token.
    pub fn is_whitespace(&self) -> bool {
        matches!(self, ComponentValue::Token(Token::Whitespace))
    }

    /// The identifier text, if this is an ident token.
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            ComponentValue::Token(Token::Ident(s)) => Some(s),
            _ => None,
        }
    }

    /// Serialize back to CSS text (approximately).
    pub fn to_css(&self, out: &mut String) {
        match self {
            ComponentValue::Token(t) => out.push_str(&t.to_string()),
            ComponentValue::Function { name, args } => {
                out.push_str(name);
                out.push('(');
                for a in args {
                    a.to_css(out);
                }
                out.push(')');
            }
            ComponentValue::Block { kind, content } => {
                let (open, close) = match kind {
                    BlockKind::Curly => ('{', '}'),
                    BlockKind::Square => ('[', ']'),
                    BlockKind::Paren => ('(', ')'),
                };
                out.push(open);
                for c in content {
                    c.to_css(out);
                }
                out.push(close);
            }
        }
    }
}

/// A problem recovered from while parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssError {
    /// Short description.
    pub message: &'static str,
}

/// A parsed stylesheet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stylesheet {
    /// Top-level rules in source order.
    pub rules: Vec<Rule>,
}

/// A rule in a stylesheet.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    /// `selectors { declarations }`
    Style(StyleRule),
    /// `@media queries { rules }`
    Media(MediaRule),
}

/// A style rule.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleRule {
    /// The selector list. Never empty.
    pub selectors: Vec<Selector>,
    /// Declarations in source order.
    pub declarations: Vec<Declaration>,
}

/// An `@media` rule.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRule {
    /// The media query list; the rule applies if any query matches.
    pub queries: Vec<MediaQuery>,
    /// Nested rules.
    pub rules: Vec<Rule>,
}

/// A `name: value !important` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    /// Lowercased property name.
    pub name: String,
    /// The value, with whitespace tokens removed and `!important` stripped.
    pub value: Vec<ComponentValue>,
    /// Whether `!important` was present.
    pub important: bool,
}

/// Tokenize `source` and wrap each token as a component value (nested
/// blocks are grouped lazily by the parser).
pub(crate) fn component_values(source: &str) -> Vec<ComponentValue> {
    let flat: Vec<ComponentValue> = tokenize(source)
        .into_iter()
        .map(ComponentValue::Token)
        .collect();
    let mut errors = Vec::new();
    let mut p = Parser::new(&flat, &mut errors);
    let mut out = Vec::new();
    while let Some(cv) = p.consume_component_value() {
        out.push(cv);
    }
    out
}

pub(crate) fn parse_stylesheet(source: &str) -> (Stylesheet, Vec<CssError>) {
    let items: Vec<ComponentValue> = tokenize(source)
        .into_iter()
        .map(ComponentValue::Token)
        .collect();
    let mut errors = Vec::new();
    let rules = Parser::new(&items, &mut errors).consume_rule_list(true);
    (Stylesheet { rules }, errors)
}

pub(crate) fn parse_declaration_list(source: &str) -> (Vec<Declaration>, Vec<CssError>) {
    let items: Vec<ComponentValue> = tokenize(source)
        .into_iter()
        .map(ComponentValue::Token)
        .collect();
    let mut errors = Vec::new();
    let decls = Parser::new(&items, &mut errors).consume_declaration_list();
    (decls, errors)
}

struct Parser<'i, 'e> {
    items: &'i [ComponentValue],
    pos: usize,
    errors: &'e mut Vec<CssError>,
}

impl<'i, 'e> Parser<'i, 'e> {
    fn new(items: &'i [ComponentValue], errors: &'e mut Vec<CssError>) -> Self {
        Self {
            items,
            pos: 0,
            errors,
        }
    }

    fn error(&mut self, message: &'static str) {
        self.errors.push(CssError { message });
    }

    fn peek(&self) -> Option<&'i ComponentValue> {
        self.items.get(self.pos)
    }

    fn peek_token(&self) -> Option<&'i Token> {
        match self.peek() {
            Some(ComponentValue::Token(t)) => Some(t),
            _ => None,
        }
    }

    fn next(&mut self) -> Option<&'i ComponentValue> {
        let item = self.items.get(self.pos)?;
        self.pos += 1;
        Some(item)
    }

    /// §5.4.7. Groups blocks and functions; everything else passes through.
    fn consume_component_value(&mut self) -> Option<ComponentValue> {
        let item = self.next()?;
        Some(match item {
            ComponentValue::Token(Token::OpenCurly) => self.consume_simple_block(BlockKind::Curly),
            ComponentValue::Token(Token::OpenSquare) => {
                self.consume_simple_block(BlockKind::Square)
            }
            ComponentValue::Token(Token::OpenParen) => self.consume_simple_block(BlockKind::Paren),
            ComponentValue::Token(Token::Function(name)) => self.consume_function(name.clone()),
            other => other.clone(),
        })
    }

    /// §5.4.8. Called after the opening token has been consumed.
    fn consume_simple_block(&mut self, kind: BlockKind) -> ComponentValue {
        let close = match kind {
            BlockKind::Curly => Token::CloseCurly,
            BlockKind::Square => Token::CloseSquare,
            BlockKind::Paren => Token::CloseParen,
        };
        let mut content = Vec::new();
        loop {
            match self.peek_token() {
                None if self.peek().is_none() => {
                    self.error("unclosed block");
                    break;
                }
                Some(t) if *t == close => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    if let Some(cv) = self.consume_component_value() {
                        content.push(cv);
                    }
                }
            }
        }
        ComponentValue::Block { kind, content }
    }

    /// §5.4.9. Called after the function token has been consumed.
    fn consume_function(&mut self, name: String) -> ComponentValue {
        let mut args = Vec::new();
        loop {
            match self.peek_token() {
                None if self.peek().is_none() => {
                    self.error("unclosed function");
                    break;
                }
                Some(Token::CloseParen) => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    if let Some(cv) = self.consume_component_value() {
                        args.push(cv);
                    }
                }
            }
        }
        ComponentValue::Function { name, args }
    }

    /// §5.4.1.
    fn consume_rule_list(&mut self, top_level: bool) -> Vec<Rule> {
        let mut rules = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(ComponentValue::Token(Token::Whitespace)) => self.pos += 1,
                Some(ComponentValue::Token(Token::Cdo | Token::Cdc)) if top_level => self.pos += 1,
                Some(ComponentValue::Token(Token::AtKeyword(_))) => {
                    if let Some(r) = self.consume_at_rule() {
                        rules.push(r);
                    }
                }
                _ => {
                    if let Some(r) = self.consume_qualified_rule() {
                        rules.push(r);
                    }
                }
            }
        }
        rules
    }

    /// §5.4.2, plus interpretation of the at-rules we understand.
    fn consume_at_rule(&mut self) -> Option<Rule> {
        let Some(ComponentValue::Token(Token::AtKeyword(name))) = self.next() else {
            return None;
        };
        let name = name.to_ascii_lowercase();
        let mut prelude = Vec::new();
        let mut block: Option<Vec<ComponentValue>> = None;
        loop {
            match self.peek() {
                None => {
                    self.error("eof in at-rule");
                    break;
                }
                Some(ComponentValue::Token(Token::Semicolon)) => {
                    self.pos += 1;
                    break;
                }
                Some(ComponentValue::Token(Token::OpenCurly)) => {
                    self.pos += 1;
                    if let ComponentValue::Block { content, .. } =
                        self.consume_simple_block(BlockKind::Curly)
                    {
                        block = Some(content);
                    }
                    break;
                }
                Some(ComponentValue::Block {
                    kind: BlockKind::Curly,
                    content,
                }) => {
                    self.pos += 1;
                    block = Some(content.clone());
                    break;
                }
                _ => {
                    if let Some(cv) = self.consume_component_value() {
                        prelude.push(cv);
                    }
                }
            }
        }

        match name.as_str() {
            "media" => {
                let Some(block) = block else {
                    self.error("@media without block");
                    return None;
                };
                let queries = media::parse_media_query_list(&prelude);
                let rules = Parser::new(&block, self.errors).consume_rule_list(false);
                Some(Rule::Media(MediaRule { queries, rules }))
            }
            _ => {
                self.error("unsupported at-rule");
                None
            }
        }
    }

    /// §5.4.3.
    fn consume_qualified_rule(&mut self) -> Option<Rule> {
        let mut prelude = Vec::new();
        loop {
            match self.peek() {
                None => {
                    self.error("eof in qualified rule");
                    return None;
                }
                Some(ComponentValue::Token(Token::OpenCurly)) => {
                    self.pos += 1;
                    let ComponentValue::Block { content, .. } =
                        self.consume_simple_block(BlockKind::Curly)
                    else {
                        return None;
                    };
                    return self.finish_style_rule(&prelude, &content);
                }
                Some(ComponentValue::Block {
                    kind: BlockKind::Curly,
                    content,
                }) => {
                    self.pos += 1;
                    return self.finish_style_rule(&prelude, content);
                }
                _ => {
                    if let Some(cv) = self.consume_component_value() {
                        prelude.push(cv);
                    }
                }
            }
        }
    }

    fn finish_style_rule(
        &mut self,
        prelude: &[ComponentValue],
        block: &[ComponentValue],
    ) -> Option<Rule> {
        let Some(selectors) = selector::parse_selector_list(prelude) else {
            self.error("invalid selector");
            return None;
        };
        let declarations = Parser::new(block, self.errors).consume_declaration_list();
        Some(Rule::Style(StyleRule {
            selectors,
            declarations,
        }))
    }

    /// §5.4.5.
    fn consume_declaration_list(&mut self) -> Vec<Declaration> {
        let mut decls = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(ComponentValue::Token(Token::Whitespace | Token::Semicolon)) => self.pos += 1,
                Some(ComponentValue::Token(Token::AtKeyword(_))) => {
                    self.error("at-rule inside declaration list");
                    self.consume_at_rule();
                }
                Some(ComponentValue::Token(Token::Ident(_))) => {
                    let items = self.collect_until_semicolon();
                    match parse_declaration(items) {
                        Some(d) => decls.push(d),
                        None => self.error("invalid declaration"),
                    }
                }
                _ => {
                    self.error("invalid declaration");
                    self.collect_until_semicolon();
                }
            }
        }
        decls
    }

    /// Consume component values up to (not including) the next `;`, so
    /// that nested blocks containing `;` are skipped correctly.
    fn collect_until_semicolon(&mut self) -> Vec<ComponentValue> {
        let mut items = Vec::new();
        while let Some(cv) = self.peek() {
            if matches!(cv, ComponentValue::Token(Token::Semicolon)) {
                break;
            }
            if let Some(cv) = self.consume_component_value() {
                items.push(cv);
            }
        }
        items
    }
}

/// §5.4.6.
fn parse_declaration(items: Vec<ComponentValue>) -> Option<Declaration> {
    let mut it = items.into_iter().peekable();
    let name = it.next()?.as_ident()?.to_ascii_lowercase();
    while it.peek().is_some_and(ComponentValue::is_whitespace) {
        it.next();
    }
    if !matches!(it.next(), Some(ComponentValue::Token(Token::Colon))) {
        return None;
    }
    let mut value: Vec<ComponentValue> = it.collect();

    // Strip trailing whitespace, then look for `!important`.
    while value.last().is_some_and(ComponentValue::is_whitespace) {
        value.pop();
    }
    let mut important = false;
    if value.len() >= 2 {
        let is_important = value
            .last()
            .and_then(ComponentValue::as_ident)
            .is_some_and(|s| s.eq_ignore_ascii_case("important"));
        if is_important {
            // Find the `!`, allowing whitespace between it and `important`.
            let mut i = value.len() - 2;
            while i > 0 && value[i].is_whitespace() {
                i -= 1;
            }
            if matches!(value[i], ComponentValue::Token(Token::Delim('!'))) {
                value.truncate(i);
                important = true;
            }
        }
    }

    value.retain(|cv| !cv.is_whitespace());
    Some(Declaration {
        name,
        value,
        important,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(src: &str) -> Stylesheet {
        parse_stylesheet(src).0
    }

    fn style_rules(s: &Stylesheet) -> Vec<&StyleRule> {
        s.rules
            .iter()
            .filter_map(|r| match r {
                Rule::Style(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn decl_names(r: &StyleRule) -> Vec<&str> {
        r.declarations.iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn simple_stylesheet() {
        let s = sheet("p { color: red; margin: 0 auto }\n.a, #b > c { x: 1 }");
        let rules = style_rules(&s);
        assert_eq!(rules.len(), 2);
        assert_eq!(decl_names(rules[0]), ["color", "margin"]);
        assert_eq!(
            rules[0].declarations[1].value.len(),
            2,
            "whitespace removed"
        );
        assert_eq!(rules[1].selectors.len(), 2);
    }

    #[test]
    fn important() {
        let s = sheet("p { a: 1 !important; b: 2 ! important ; c: 3 }");
        let d = &style_rules(&s)[0].declarations;
        assert!(d[0].important);
        assert!(d[1].important);
        assert!(!d[2].important);
        assert_eq!(d[0].value.len(), 1);
        assert_eq!(d[1].value.len(), 1);
    }

    #[test]
    fn invalid_declarations_are_skipped_individually() {
        let (s, errors) =
            parse_stylesheet("p { color: red; junk; : 1; margin: { nested; } ; good: 1 }");
        let d = &style_rules(&s)[0].declarations;
        assert_eq!(
            d.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["color", "margin", "good"],
            "the block value is syntactically a declaration; property parsing rejects it later"
        );
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn invalid_selector_drops_whole_rule() {
        let (s, errors) = parse_stylesheet("p, 123 { a: 1 } q { b: 2 }");
        let rules = style_rules(&s);
        assert_eq!(rules.len(), 1);
        assert_eq!(decl_names(rules[0]), ["b"]);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn unclosed_block_at_eof() {
        let s = sheet("p { color: red; margin: 0");
        let rules = style_rules(&s);
        assert_eq!(decl_names(rules[0]), ["color", "margin"]);
    }

    #[test]
    fn rule_without_block_is_dropped() {
        let (s, errors) = parse_stylesheet("p q r");
        assert!(s.rules.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn stray_close_brace_recovers() {
        let s = sheet("} p { a: 1 }");
        // The `}` is consumed as the prelude of a rule that then finds `{`;
        // selector `} p` is invalid, so the rule is dropped. Nothing else.
        assert!(style_rules(&s).is_empty());
        let s = sheet("p { a: 1 } } q { b: 2 }");
        assert_eq!(
            style_rules(&s).len(),
            1,
            "the extra `}}` eats the next rule's prelude"
        );
    }

    #[test]
    fn unknown_at_rules_are_dropped() {
        let (s, errors) = parse_stylesheet(
            "@import url(x.css); @font-face { font-family: X } p { a: 1 } @charset \"utf-8\";",
        );
        assert_eq!(style_rules(&s).len(), 1);
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn media_rule() {
        let s = sheet("@media screen and (min-width: 600px) { p { a: 1 } q { b: 2 } } r { c: 3 }");
        assert_eq!(s.rules.len(), 2);
        let Rule::Media(m) = &s.rules[0] else {
            panic!("expected media rule");
        };
        assert_eq!(m.queries.len(), 1);
        assert_eq!(m.rules.len(), 2);
    }

    #[test]
    fn cdo_cdc_ignored_at_top_level() {
        let s = sheet("<!-- p { a: 1 } -->");
        assert_eq!(style_rules(&s).len(), 1);
    }

    #[test]
    fn declaration_list_for_style_attribute() {
        let d = parse_declaration_list("color: red; bogus; margin: 1px 2px !important").0;
        assert_eq!(d.len(), 2);
        assert_eq!(d[1].name, "margin");
        assert!(d[1].important);
    }

    #[test]
    fn nested_semicolons_inside_blocks_do_not_split() {
        let d = parse_declaration_list("a: f(1;2); b: 1").0;
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "a");
    }
}
