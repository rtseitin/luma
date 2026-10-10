//! Selector parsing (Selectors Level 4) and specificity.
//!
//! Selectors are stored right-to-left: `compounds[0]` is the subject (the
//! element being matched) and `combinators[i]` links `compounds[i]` to
//! `compounds[i + 1]`. That is the order matching walks them.

use std::fmt;

use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::Token;

/// A complex selector: compounds joined by combinators.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// Compound selectors, rightmost (subject) first.
    pub compounds: Vec<Compound>,
    /// `combinators[i]` relates `compounds[i]` to `compounds[i + 1]`.
    /// Always one shorter than `compounds`.
    pub combinators: Vec<Combinator>,
    /// Pseudo-element on the subject, if any.
    pub pseudo_element: Option<PseudoElement>,
    /// Precomputed specificity.
    pub specificity: Specificity,
}

/// A sequence of simple selectors with no combinators between them.
#[derive(Debug, Clone, PartialEq)]
pub struct Compound {
    /// Simple selectors, in source order. Never empty unless the compound
    /// consists solely of a pseudo-element.
    pub parts: Vec<SimpleSelector>,
}

/// A single simple selector.
#[derive(Debug, Clone, PartialEq)]
pub enum SimpleSelector {
    /// `div` (lowercased).
    Type(String),
    /// `*`
    Universal,
    /// `#main`
    Id(String),
    /// `.intro`
    Class(String),
    /// `[attr]`, `[attr=value]`, ...
    Attribute(AttrSelector),
    /// `:first-child`, `:not(...)`, ...
    PseudoClass(PseudoClass),
}

/// An attribute selector.
#[derive(Debug, Clone, PartialEq)]
pub struct AttrSelector {
    /// Lowercased attribute name.
    pub name: String,
    /// Operator and value; `None` for presence-only `[attr]`.
    pub op: Option<(AttrOp, String)>,
    /// Whether the `i` flag was given.
    pub case_insensitive: bool,
}

/// Attribute comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `=`
    Equals,
    /// `~=` (whitespace-separated word)
    Includes,
    /// `|=` (exact or followed by `-`)
    DashMatch,
    /// `^=`
    Prefix,
    /// `$=`
    Suffix,
    /// `*=`
    Substring,
}

/// Pseudo-classes.
#[derive(Debug, Clone, PartialEq)]
pub enum PseudoClass {
    /// `:root`
    Root,
    /// `:empty`
    Empty,
    /// `:first-child`
    FirstChild,
    /// `:last-child`
    LastChild,
    /// `:only-child`
    OnlyChild,
    /// `:first-of-type`
    FirstOfType,
    /// `:last-of-type`
    LastOfType,
    /// `:only-of-type`
    OnlyOfType,
    /// `:nth-child(an+b)`
    NthChild(Nth),
    /// `:nth-last-child(an+b)`
    NthLastChild(Nth),
    /// `:nth-of-type(an+b)`
    NthOfType(Nth),
    /// `:nth-last-of-type(an+b)`
    NthLastOfType(Nth),
    /// `:not(list)`
    Not(Vec<Selector>),
    /// `:is(list)` (forgiving)
    Is(Vec<Selector>),
    /// `:where(list)` (forgiving, zero specificity)
    Where(Vec<Selector>),
    /// `:link`
    Link,
    /// `:any-link`
    AnyLink,
    /// `:visited` (never matches)
    Visited,
    /// `:hover` (never matches)
    Hover,
    /// `:active` (never matches)
    Active,
    /// `:focus`, `:focus-visible`, `:focus-within` (never match)
    Focus,
    /// `:enabled`
    Enabled,
    /// `:disabled`
    Disabled,
    /// `:checked`
    Checked,
}

/// The `an+b` argument of the `:nth-*` pseudo-classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nth {
    /// The step.
    pub a: i32,
    /// The offset.
    pub b: i32,
}

impl Nth {
    /// Whether a 1-based `index` is selected by this `an+b`.
    pub fn matches(&self, index: usize) -> bool {
        let Ok(index) = i32::try_from(index) else {
            return false;
        };
        if self.a == 0 {
            return index == self.b;
        }
        let diff = index - self.b;
        diff % self.a == 0 && diff / self.a >= 0
    }
}

/// Pseudo-elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PseudoElement {
    /// `::before`
    Before,
    /// `::after`
    After,
    /// `::first-line`
    FirstLine,
    /// `::first-letter`
    FirstLetter,
}

/// Combinators between compounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    /// Whitespace: any ancestor.
    Descendant,
    /// `>`: parent.
    Child,
    /// `+`: immediately preceding sibling.
    NextSibling,
    /// `~`: any preceding sibling.
    SubsequentSibling,
}

/// Selector specificity `(ids, classes/attributes/pseudo-classes,
/// types/pseudo-elements)`. Compares lexicographically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Specificity(pub u32, pub u32, pub u32);

impl Specificity {
    fn add(self, other: Specificity) -> Specificity {
        Specificity(self.0 + other.0, self.1 + other.1, self.2 + other.2)
    }
}

// ----- parsing ---------------------------------------------------------------

struct Cursor<'a> {
    items: &'a [ComponentValue],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&'a ComponentValue> {
        self.items.get(self.pos)
    }

    fn peek_at(&self, n: usize) -> Option<&'a ComponentValue> {
        self.items.get(self.pos + n)
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn at_end(&self) -> bool {
        self.pos >= self.items.len()
    }

    /// Skip whitespace; returns whether any was skipped.
    fn skip_ws(&mut self) -> bool {
        let start = self.pos;
        while self.peek().is_some_and(ComponentValue::is_whitespace) {
            self.pos += 1;
        }
        self.pos > start
    }
}

fn split_commas(items: &[ComponentValue]) -> Vec<&[ComponentValue]> {
    items
        .split(|cv| matches!(cv, ComponentValue::Token(Token::Comma)))
        .collect()
}

/// Parse a selector list. `None` if any selector is invalid.
pub(crate) fn parse_selector_list(items: &[ComponentValue]) -> Option<Vec<Selector>> {
    let parts = split_commas(items);
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        out.push(parse_complex(part)?);
    }
    Some(out)
}

/// Forgiving list (`:is()`, `:where()`): invalid selectors are dropped.
fn parse_forgiving_list(items: &[ComponentValue]) -> Vec<Selector> {
    split_commas(items)
        .into_iter()
        .filter_map(parse_complex)
        .collect()
}

fn parse_complex(items: &[ComponentValue]) -> Option<Selector> {
    let mut cur = Cursor { items, pos: 0 };
    cur.skip_ws();
    if cur.at_end() {
        return None;
    }

    // Collected left-to-right, reversed at the end.
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut pseudo_element;

    loop {
        let (compound, pe) = parse_compound(&mut cur)?;
        compounds.push(compound);
        pseudo_element = pe;

        let saw_ws = cur.skip_ws();
        let Some(item) = cur.peek() else {
            break;
        };
        let combinator = match item {
            ComponentValue::Token(Token::Delim('>')) => Combinator::Child,
            ComponentValue::Token(Token::Delim('+')) => Combinator::NextSibling,
            ComponentValue::Token(Token::Delim('~')) => Combinator::SubsequentSibling,
            _ if saw_ws => Combinator::Descendant,
            _ => return None,
        };
        if combinator != Combinator::Descendant {
            cur.bump();
            cur.skip_ws();
        }
        if cur.at_end() {
            return None; // dangling combinator
        }
        if pseudo_element.is_some() {
            return None; // pseudo-element must be on the subject
        }
        combinators.push(combinator);
    }

    compounds.reverse();
    combinators.reverse();

    let mut specificity = compounds.iter().fold(Specificity::default(), |acc, c| {
        acc.add(compound_specificity(c))
    });
    if pseudo_element.is_some() {
        specificity.2 += 1;
    }

    Some(Selector {
        compounds,
        combinators,
        pseudo_element,
        specificity,
    })
}

fn parse_compound(cur: &mut Cursor<'_>) -> Option<(Compound, Option<PseudoElement>)> {
    let mut parts = Vec::new();
    let mut pseudo_element = None;

    match cur.peek() {
        Some(ComponentValue::Token(Token::Ident(name))) => {
            parts.push(SimpleSelector::Type(name.to_ascii_lowercase()));
            cur.bump();
        }
        Some(ComponentValue::Token(Token::Delim('*'))) => {
            parts.push(SimpleSelector::Universal);
            cur.bump();
        }
        _ => {}
    }

    while pseudo_element.is_none() {
        match cur.peek() {
            Some(ComponentValue::Token(Token::Hash { value, is_id: true })) => {
                parts.push(SimpleSelector::Id(value.clone()));
                cur.bump();
            }
            Some(ComponentValue::Token(Token::Delim('.'))) => {
                let Some(ComponentValue::Token(Token::Ident(name))) = cur.peek_at(1) else {
                    return None;
                };
                parts.push(SimpleSelector::Class(name.clone()));
                cur.bump();
                cur.bump();
            }
            Some(ComponentValue::Block {
                kind: BlockKind::Square,
                content,
            }) => {
                parts.push(SimpleSelector::Attribute(parse_attr(content)?));
                cur.bump();
            }
            Some(ComponentValue::Token(Token::Colon)) => {
                cur.bump();
                match cur.peek() {
                    Some(ComponentValue::Token(Token::Colon)) => {
                        cur.bump();
                        let Some(ComponentValue::Token(Token::Ident(name))) = cur.peek() else {
                            return None;
                        };
                        pseudo_element = Some(parse_pseudo_element(name)?);
                        cur.bump();
                    }
                    Some(ComponentValue::Token(Token::Ident(name))) => {
                        let lower = name.to_ascii_lowercase();
                        // Legacy single-colon pseudo-elements.
                        if let Some(pe) = parse_pseudo_element(&lower)
                            && matches!(pe, PseudoElement::Before | PseudoElement::After)
                        {
                            pseudo_element = Some(pe);
                        } else if matches!(lower.as_str(), "first-line" | "first-letter") {
                            pseudo_element = parse_pseudo_element(&lower);
                        } else {
                            parts.push(SimpleSelector::PseudoClass(parse_pseudo_class(&lower)?));
                        }
                        cur.bump();
                    }
                    Some(ComponentValue::Function { name, args }) => {
                        parts.push(SimpleSelector::PseudoClass(parse_functional_pseudo(
                            &name.to_ascii_lowercase(),
                            args,
                        )?));
                        cur.bump();
                    }
                    _ => return None,
                }
            }
            _ => break,
        }
    }

    if parts.is_empty() && pseudo_element.is_none() {
        return None;
    }
    Some((Compound { parts }, pseudo_element))
}

fn parse_attr(content: &[ComponentValue]) -> Option<AttrSelector> {
    let mut cur = Cursor {
        items: content,
        pos: 0,
    };
    cur.skip_ws();
    let Some(ComponentValue::Token(Token::Ident(name))) = cur.peek() else {
        return None;
    };
    let name = name.to_ascii_lowercase();
    cur.bump();
    cur.skip_ws();

    if cur.at_end() {
        return Some(AttrSelector {
            name,
            op: None,
            case_insensitive: false,
        });
    }

    let op = match cur.peek() {
        Some(ComponentValue::Token(Token::Delim('='))) => {
            cur.bump();
            AttrOp::Equals
        }
        Some(ComponentValue::Token(Token::Delim(c @ ('~' | '|' | '^' | '$' | '*')))) => {
            cur.bump();
            if !matches!(cur.peek(), Some(ComponentValue::Token(Token::Delim('=')))) {
                return None;
            }
            cur.bump();
            match c {
                '~' => AttrOp::Includes,
                '|' => AttrOp::DashMatch,
                '^' => AttrOp::Prefix,
                '$' => AttrOp::Suffix,
                _ => AttrOp::Substring,
            }
        }
        _ => return None,
    };
    cur.skip_ws();

    let value = match cur.peek() {
        Some(ComponentValue::Token(Token::Ident(v) | Token::Str(v))) => v.clone(),
        _ => return None,
    };
    cur.bump();
    cur.skip_ws();

    let mut case_insensitive = false;
    if let Some(ComponentValue::Token(Token::Ident(flag))) = cur.peek() {
        match flag.to_ascii_lowercase().as_str() {
            "i" => case_insensitive = true,
            "s" => {}
            _ => return None,
        }
        cur.bump();
        cur.skip_ws();
    }
    if !cur.at_end() {
        return None;
    }

    Some(AttrSelector {
        name,
        op: Some((op, value)),
        case_insensitive,
    })
}

fn parse_pseudo_element(name: &str) -> Option<PseudoElement> {
    Some(match name.to_ascii_lowercase().as_str() {
        "before" => PseudoElement::Before,
        "after" => PseudoElement::After,
        "first-line" => PseudoElement::FirstLine,
        "first-letter" => PseudoElement::FirstLetter,
        _ => return None,
    })
}

fn parse_pseudo_class(name: &str) -> Option<PseudoClass> {
    Some(match name {
        "root" => PseudoClass::Root,
        "empty" => PseudoClass::Empty,
        "first-child" => PseudoClass::FirstChild,
        "last-child" => PseudoClass::LastChild,
        "only-child" => PseudoClass::OnlyChild,
        "first-of-type" => PseudoClass::FirstOfType,
        "last-of-type" => PseudoClass::LastOfType,
        "only-of-type" => PseudoClass::OnlyOfType,
        "link" => PseudoClass::Link,
        "any-link" => PseudoClass::AnyLink,
        "visited" => PseudoClass::Visited,
        "hover" => PseudoClass::Hover,
        "active" => PseudoClass::Active,
        "focus" | "focus-visible" | "focus-within" => PseudoClass::Focus,
        "enabled" => PseudoClass::Enabled,
        "disabled" => PseudoClass::Disabled,
        "checked" => PseudoClass::Checked,
        _ => return None,
    })
}

fn parse_functional_pseudo(name: &str, args: &[ComponentValue]) -> Option<PseudoClass> {
    Some(match name {
        "nth-child" => PseudoClass::NthChild(parse_nth(args)?),
        "nth-last-child" => PseudoClass::NthLastChild(parse_nth(args)?),
        "nth-of-type" => PseudoClass::NthOfType(parse_nth(args)?),
        "nth-last-of-type" => PseudoClass::NthLastOfType(parse_nth(args)?),
        "not" => {
            let list = parse_selector_list(args)?;
            if list.iter().any(|s| s.pseudo_element.is_some()) {
                return None;
            }
            PseudoClass::Not(list)
        }
        "is" => PseudoClass::Is(parse_forgiving_list(args)),
        "where" => PseudoClass::Where(parse_forgiving_list(args)),
        _ => return None,
    })
}

/// Parse `an+b`. Tokenization splits it awkwardly (`2n-1` is a single
/// dimension token, `-n` an ident), so we serialize back to text and parse
/// the string.
fn parse_nth(args: &[ComponentValue]) -> Option<Nth> {
    let mut text = String::new();
    let mut prev_was_ws = true;
    for a in args {
        // Number tokens do not record an explicit `+`, so `2n+1`
        // tokenizes as `2n` `1`. A number directly adjacent to the
        // preceding token can only have started with a sign (a bare digit
        // would have been absorbed into the ident), so restore it.
        if let ComponentValue::Token(Token::Number { value, .. }) = a
            && !prev_was_ws
            && *value >= 0.0
        {
            text.push('+');
        }
        a.to_css(&mut text);
        prev_was_ws = a.is_whitespace();
    }
    let s: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();

    match s.as_str() {
        "odd" => return Some(Nth { a: 2, b: 1 }),
        "even" => return Some(Nth { a: 2, b: 0 }),
        "" => return None,
        _ => {}
    }

    let Some(n_pos) = s.find('n') else {
        return Some(Nth {
            a: 0,
            b: s.parse().ok()?,
        });
    };
    let (a_str, b_str) = (&s[..n_pos], &s[n_pos + 1..]);
    let a = match a_str {
        "" | "+" => 1,
        "-" => -1,
        _ => a_str.parse().ok()?,
    };
    let b = if b_str.is_empty() {
        0
    } else {
        if !b_str.starts_with(['+', '-']) {
            return None;
        }
        b_str.parse().ok()?
    };
    Some(Nth { a, b })
}

fn compound_specificity(c: &Compound) -> Specificity {
    c.parts.iter().fold(Specificity::default(), |acc, p| {
        acc.add(simple_specificity(p))
    })
}

fn simple_specificity(s: &SimpleSelector) -> Specificity {
    match s {
        SimpleSelector::Type(_) => Specificity(0, 0, 1),
        SimpleSelector::Universal => Specificity(0, 0, 0),
        SimpleSelector::Id(_) => Specificity(1, 0, 0),
        SimpleSelector::Class(_) | SimpleSelector::Attribute(_) => Specificity(0, 1, 0),
        SimpleSelector::PseudoClass(pc) => match pc {
            PseudoClass::Not(list) | PseudoClass::Is(list) => {
                list.iter().map(|s| s.specificity).max().unwrap_or_default()
            }
            PseudoClass::Where(_) => Specificity(0, 0, 0),
            _ => Specificity(0, 1, 0),
        },
    }
}

// ----- display ---------------------------------------------------------------

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Stored right-to-left; print left-to-right.
        for (i, compound) in self.compounds.iter().enumerate().rev() {
            write!(f, "{compound}")?;
            if i == 0 {
                if let Some(pe) = self.pseudo_element {
                    write!(f, "{pe}")?;
                }
            } else {
                let comb = self.combinators[i - 1];
                write!(f, "{comb}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Compound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for p in &self.parts {
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

fn write_list(f: &mut fmt::Formatter<'_>, list: &[Selector]) -> fmt::Result {
    for (i, s) in list.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{s}")?;
    }
    Ok(())
}

impl fmt::Display for SimpleSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::tokenizer::{write_ident, write_string};
        match self {
            SimpleSelector::Type(t) => write_ident(f, t),
            SimpleSelector::Universal => f.write_str("*"),
            SimpleSelector::Id(id) => {
                f.write_str("#")?;
                write_ident(f, id)
            }
            SimpleSelector::Class(c) => {
                f.write_str(".")?;
                write_ident(f, c)
            }
            SimpleSelector::Attribute(a) => {
                f.write_str("[")?;
                write_ident(f, &a.name)?;
                if let Some((op, v)) = &a.op {
                    let op = match op {
                        AttrOp::Equals => "=",
                        AttrOp::Includes => "~=",
                        AttrOp::DashMatch => "|=",
                        AttrOp::Prefix => "^=",
                        AttrOp::Suffix => "$=",
                        AttrOp::Substring => "*=",
                    };
                    f.write_str(op)?;
                    write_string(f, v)?;
                    if a.case_insensitive {
                        f.write_str(" i")?;
                    }
                }
                f.write_str("]")
            }
            SimpleSelector::PseudoClass(pc) => match pc {
                PseudoClass::Root => f.write_str(":root"),
                PseudoClass::Empty => f.write_str(":empty"),
                PseudoClass::FirstChild => f.write_str(":first-child"),
                PseudoClass::LastChild => f.write_str(":last-child"),
                PseudoClass::OnlyChild => f.write_str(":only-child"),
                PseudoClass::FirstOfType => f.write_str(":first-of-type"),
                PseudoClass::LastOfType => f.write_str(":last-of-type"),
                PseudoClass::OnlyOfType => f.write_str(":only-of-type"),
                PseudoClass::NthChild(n) => write!(f, ":nth-child({n})"),
                PseudoClass::NthLastChild(n) => write!(f, ":nth-last-child({n})"),
                PseudoClass::NthOfType(n) => write!(f, ":nth-of-type({n})"),
                PseudoClass::NthLastOfType(n) => write!(f, ":nth-last-of-type({n})"),
                PseudoClass::Not(l) => {
                    f.write_str(":not(")?;
                    write_list(f, l)?;
                    f.write_str(")")
                }
                PseudoClass::Is(l) => {
                    f.write_str(":is(")?;
                    write_list(f, l)?;
                    f.write_str(")")
                }
                PseudoClass::Where(l) => {
                    f.write_str(":where(")?;
                    write_list(f, l)?;
                    f.write_str(")")
                }
                PseudoClass::Link => f.write_str(":link"),
                PseudoClass::AnyLink => f.write_str(":any-link"),
                PseudoClass::Visited => f.write_str(":visited"),
                PseudoClass::Hover => f.write_str(":hover"),
                PseudoClass::Active => f.write_str(":active"),
                PseudoClass::Focus => f.write_str(":focus"),
                PseudoClass::Enabled => f.write_str(":enabled"),
                PseudoClass::Disabled => f.write_str(":disabled"),
                PseudoClass::Checked => f.write_str(":checked"),
            },
        }
    }
}

impl fmt::Display for Nth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.a, self.b) {
            (0, b) => write!(f, "{b}"),
            (a, 0) => write!(f, "{a}n"),
            (a, b) if b < 0 => write!(f, "{a}n{b}"),
            (a, b) => write!(f, "{a}n+{b}"),
        }
    }
}

impl fmt::Display for PseudoElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PseudoElement::Before => "::before",
            PseudoElement::After => "::after",
            PseudoElement::FirstLine => "::first-line",
            PseudoElement::FirstLetter => "::first-letter",
        })
    }
}

impl fmt::Display for Combinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Combinator::Descendant => " ",
            Combinator::Child => " > ",
            Combinator::NextSibling => " + ",
            Combinator::SubsequentSibling => " ~ ",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_selector_list as parse;

    fn one(src: &str) -> Selector {
        let mut list = parse(src).unwrap_or_else(|| panic!("{src:?} should parse"));
        assert_eq!(list.len(), 1);
        list.remove(0)
    }

    fn roundtrip(src: &str, expected: &str) {
        assert_eq!(one(src).to_string(), expected, "source: {src}");
    }

    #[test]
    fn normalizes_and_roundtrips() {
        roundtrip("DIV", "div");
        roundtrip("div.A#B", "div.A#B");
        roundtrip("a   >  b", "a > b");
        roundtrip("a b", "a b");
        roundtrip("a+b~c", "a + b ~ c");
        roundtrip("*", "*");
        roundtrip("[Data-X]", "[data-x]");
        roundtrip(
            "[a=b][c~='d e'][f|=g][h^=i][j$=k][l*=m i]",
            "[a=\"b\"][c~=\"d e\"][f|=\"g\"][h^=\"i\"][j$=\"k\"][l*=\"m\" i]",
        );
        roundtrip(":root:empty:first-child", ":root:empty:first-child");
        roundtrip("li:nth-child( 2n + 1 )", "li:nth-child(2n+1)");
        roundtrip("li:nth-child(odd)", "li:nth-child(2n+1)");
        roundtrip("li:nth-child(even)", "li:nth-child(2n)");
        roundtrip("li:nth-child(-n+3)", "li:nth-child(-1n+3)");
        roundtrip("li:nth-child(3)", "li:nth-child(3)");
        roundtrip("li:nth-child(n)", "li:nth-child(1n)");
        roundtrip("li:nth-last-of-type(2n-1)", "li:nth-last-of-type(2n-1)");
        roundtrip("p:not(.a, #b)", "p:not(.a, #b)");
        roundtrip("p::before", "p::before");
        roundtrip("p:after", "p::after");
        roundtrip("a:hover", "a:hover");
    }

    #[test]
    fn stored_right_to_left() {
        let s = one("a > b c");
        assert_eq!(s.compounds.len(), 3);
        assert_eq!(s.compounds[0].parts, [SimpleSelector::Type("c".into())]);
        assert_eq!(s.combinators, [Combinator::Descendant, Combinator::Child]);
    }

    #[test]
    fn specificity() {
        assert_eq!(one("*").specificity, Specificity(0, 0, 0));
        assert_eq!(one("li").specificity, Specificity(0, 0, 1));
        assert_eq!(one("ul li").specificity, Specificity(0, 0, 2));
        assert_eq!(one("li.red").specificity, Specificity(0, 1, 1));
        assert_eq!(one("#x").specificity, Specificity(1, 0, 0));
        assert_eq!(
            one("#x .y z[a]:first-child").specificity,
            Specificity(1, 3, 1)
        );
        assert_eq!(one(":not(#a, .b)").specificity, Specificity(1, 0, 0));
        assert_eq!(one(":is(#a, .b)").specificity, Specificity(1, 0, 0));
        assert_eq!(one(":where(#a, .b)").specificity, Specificity(0, 0, 0));
        assert_eq!(one("p::before").specificity, Specificity(0, 0, 2));
        assert!(Specificity(1, 0, 0) > Specificity(0, 99, 99));
    }

    #[test]
    fn invalid_selectors() {
        for src in [
            "",
            ",",
            "a,",
            "a >",
            "> a",
            "a > > b",
            "#1a",
            ".",
            "a.",
            "[",
            "[a=]",
            "[a==b]",
            "[a=b x]",
            ":unknown",
            ":nth-child()",
            ":nth-child(2x)",
            ":nth-child(n 1)",
            "::bogus",
            "a::before b",
            ":not(::before)",
            "a|b",
            "123",
        ] {
            assert!(parse(src).is_none(), "{src:?} should be invalid");
        }
    }

    #[test]
    fn invalid_entry_invalidates_the_list() {
        assert!(parse("a, :bogus").is_none());
    }

    #[test]
    fn forgiving_lists() {
        let s = one(":is(a, :bogus, b)");
        let SimpleSelector::PseudoClass(PseudoClass::Is(list)) = &s.compounds[0].parts[0] else {
            panic!();
        };
        assert_eq!(list.len(), 2);
        assert!(
            parse(":is(:bogus)").is_some(),
            "all-invalid :is() is still valid"
        );
    }

    #[test]
    fn nth_matching() {
        let nth = |a, b| Nth { a, b };
        assert!(nth(0, 3).matches(3) && !nth(0, 3).matches(4));
        assert!(nth(2, 1).matches(1) && nth(2, 1).matches(3) && !nth(2, 1).matches(2));
        assert!(nth(-1, 3).matches(1) && nth(-1, 3).matches(3) && !nth(-1, 3).matches(4));
        assert!(nth(1, 0).matches(1) && nth(1, 0).matches(100));
        assert!(nth(3, -1).matches(2) && nth(3, -1).matches(5) && !nth(3, -1).matches(3));
    }
}
