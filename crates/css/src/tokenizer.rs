//! CSS tokenizer, following CSS Syntax Module Level 3 §4.
//!
//! Function names mirror the spec's algorithm names ("consume a name",
//! "consume a numeric token", ...) so the two can be read together.

use std::fmt;

const REPLACEMENT: char = '\u{FFFD}';

/// A CSS token.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// An identifier such as `div` or `--custom`.
    Ident(String),
    /// A function name with its opening paren consumed: `rgb(`.
    Function(String),
    /// `@media`, `@import`, ... (without the `@`).
    AtKeyword(String),
    /// `#abc`. `is_id` is true when the value is a valid identifier, which
    /// is required for an ID selector.
    Hash {
        /// The text after `#`.
        value: String,
        /// Whether the value would be a valid identifier.
        is_id: bool,
    },
    /// A quoted string, with quotes removed and escapes resolved.
    Str(String),
    /// A string terminated by a newline rather than a quote.
    BadString,
    /// An unquoted `url(...)`, with the URL extracted.
    Url(String),
    /// A `url(...)` that could not be parsed.
    BadUrl,
    /// Any single code point that is not part of another token.
    Delim(char),
    /// A number.
    Number {
        /// The numeric value.
        value: f32,
        /// Whether the source had no `.` or exponent.
        is_integer: bool,
    },
    /// A number followed by `%`. The value is the number before the sign.
    Percentage(f32),
    /// A number followed by a unit identifier.
    Dimension {
        /// The numeric value.
        value: f32,
        /// Whether the source had no `.` or exponent.
        is_integer: bool,
        /// The unit, as written (not lowercased).
        unit: String,
    },
    /// One or more whitespace characters.
    Whitespace,
    /// `<!--`.
    Cdo,
    /// `-->`.
    Cdc,
    /// `:`.
    Colon,
    /// `;`.
    Semicolon,
    /// `,`.
    Comma,
    /// `[`.
    OpenSquare,
    /// `]`.
    CloseSquare,
    /// `(`.
    OpenParen,
    /// `)`.
    CloseParen,
    /// `{`.
    OpenCurly,
    /// `}`.
    CloseCurly,
}

fn write_number(f: &mut fmt::Formatter<'_>, value: f32, is_integer: bool) -> fmt::Result {
    if is_integer {
        write!(f, "{}", value as i64)
    } else {
        write!(f, "{value}")
    }
}

/// CSSOM "serialize an identifier": escape so the result re-tokenizes as
/// the same single ident.
pub(crate) fn write_ident(f: &mut impl fmt::Write, s: &str) -> fmt::Result {
    for (i, c) in s.chars().enumerate() {
        match c {
            '\0' => f.write_char(REPLACEMENT)?,
            '\x01'..='\x1F' | '\x7F' => write!(f, "\\{:x} ", c as u32)?,
            '0'..='9' if i == 0 || (i == 1 && s.starts_with('-')) => {
                write!(f, "\\{:x} ", c as u32)?;
            }
            '-' if i == 0 && s.len() == 1 => f.write_str("\\-")?,
            c if is_name(c) => f.write_char(c)?,
            c => {
                f.write_char('\\')?;
                f.write_char(c)?;
            }
        }
    }
    Ok(())
}

/// CSSOM "serialize a string".
pub(crate) fn write_string(f: &mut impl fmt::Write, s: &str) -> fmt::Result {
    f.write_char('"')?;
    for c in s.chars() {
        match c {
            '\0' => f.write_char(REPLACEMENT)?,
            '\x01'..='\x1F' | '\x7F' => write!(f, "\\{:x} ", c as u32)?,
            '"' | '\\' => {
                f.write_char('\\')?;
                f.write_char(c)?;
            }
            c => f.write_char(c)?,
        }
    }
    f.write_char('"')
}

impl fmt::Display for Token {
    /// Serialize the token back to CSS text that re-tokenizes to the same
    /// token. Used for diagnostics and for `an+b` parsing.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Ident(s) => write_ident(f, s),
            Token::Function(s) => {
                write_ident(f, s)?;
                f.write_str("(")
            }
            Token::AtKeyword(s) => {
                f.write_str("@")?;
                write_ident(f, s)
            }
            Token::Hash { value, .. } => {
                f.write_str("#")?;
                write_ident(f, value)
            }
            Token::Str(s) => write_string(f, s),
            Token::BadString => f.write_str("\"\n"),
            Token::Url(s) => {
                f.write_str("url(")?;
                write_string(f, s)?;
                f.write_str(")")
            }
            Token::BadUrl => f.write_str("url(\u{FFFD})"),
            Token::Delim(c) => write!(f, "{c}"),
            Token::Number { value, is_integer } => write_number(f, *value, *is_integer),
            Token::Percentage(v) => write!(f, "{v}%"),
            Token::Dimension {
                value,
                is_integer,
                unit,
            } => {
                write_number(f, *value, *is_integer)?;
                // A unit starting with `e` could be mistaken for an
                // exponent; escape it.
                if unit.starts_with(['e', 'E']) {
                    write!(f, "\\{:x} ", unit.chars().next().unwrap() as u32)?;
                    write_ident(f, &unit[1..])
                } else {
                    write_ident(f, unit)
                }
            }
            Token::Whitespace => f.write_str(" "),
            Token::Cdo => f.write_str("<!--"),
            Token::Cdc => f.write_str("-->"),
            Token::Colon => f.write_str(":"),
            Token::Semicolon => f.write_str(";"),
            Token::Comma => f.write_str(","),
            Token::OpenSquare => f.write_str("["),
            Token::CloseSquare => f.write_str("]"),
            Token::OpenParen => f.write_str("("),
            Token::CloseParen => f.write_str(")"),
            Token::OpenCurly => f.write_str("{"),
            Token::CloseCurly => f.write_str("}"),
        }
    }
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c as u32 >= 0x80
}

fn is_name(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

fn is_ws(c: char) -> bool {
    matches!(c, '\n' | '\t' | ' ')
}

fn is_non_printable(c: char) -> bool {
    matches!(c as u32, 0..=8 | 0xB | 0xE..=0x1F | 0x7F)
}

/// Streaming CSS tokenizer.
pub struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    /// Create a tokenizer over `source`.
    pub fn new(source: &str) -> Self {
        // §3.3 preprocessing: CR, FF, CRLF -> LF; NUL -> U+FFFD.
        let mut chars = Vec::with_capacity(source.len());
        let mut it = source.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '\r' => {
                    if it.peek() == Some(&'\n') {
                        it.next();
                    }
                    chars.push('\n');
                }
                '\x0C' => chars.push('\n'),
                '\0' => chars.push(REPLACEMENT),
                c => chars.push(c),
            }
        }
        Self { chars, pos: 0 }
    }

    /// Produce the next token, or `None` at end of input.
    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_comments();
        let c = self.peek(0)?;
        Some(match c {
            c if is_ws(c) => {
                while self.peek(0).is_some_and(is_ws) {
                    self.pos += 1;
                }
                Token::Whitespace
            }
            '"' | '\'' => {
                self.pos += 1;
                self.consume_string(c)
            }
            '#' => {
                if self.peek(1).is_some_and(is_name) || self.valid_escape_at(1) {
                    let is_id = self.would_start_ident_at(1);
                    self.pos += 1;
                    let value = self.consume_name();
                    Token::Hash { value, is_id }
                } else {
                    self.pos += 1;
                    Token::Delim('#')
                }
            }
            '(' => self.single(Token::OpenParen),
            ')' => self.single(Token::CloseParen),
            ',' => self.single(Token::Comma),
            ':' => self.single(Token::Colon),
            ';' => self.single(Token::Semicolon),
            '[' => self.single(Token::OpenSquare),
            ']' => self.single(Token::CloseSquare),
            '{' => self.single(Token::OpenCurly),
            '}' => self.single(Token::CloseCurly),
            '+' | '.' => {
                if self.starts_number_at(0) {
                    self.consume_numeric()
                } else {
                    self.single(Token::Delim(c))
                }
            }
            '-' => {
                if self.starts_number_at(0) {
                    self.consume_numeric()
                } else if self.peek(1) == Some('-') && self.peek(2) == Some('>') {
                    self.pos += 3;
                    Token::Cdc
                } else if self.would_start_ident_at(0) {
                    self.consume_ident_like()
                } else {
                    self.single(Token::Delim('-'))
                }
            }
            '<' => {
                if self.peek(1) == Some('!')
                    && self.peek(2) == Some('-')
                    && self.peek(3) == Some('-')
                {
                    self.pos += 4;
                    Token::Cdo
                } else {
                    self.single(Token::Delim('<'))
                }
            }
            '@' => {
                if self.would_start_ident_at(1) {
                    self.pos += 1;
                    Token::AtKeyword(self.consume_name())
                } else {
                    self.single(Token::Delim('@'))
                }
            }
            '\\' => {
                if self.valid_escape_at(0) {
                    self.consume_ident_like()
                } else {
                    self.single(Token::Delim('\\'))
                }
            }
            c if c.is_ascii_digit() => self.consume_numeric(),
            c if is_name_start(c) => self.consume_ident_like(),
            c => self.single(Token::Delim(c)),
        })
    }

    // ----- helpers --------------------------------------------------------

    fn peek(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn single(&mut self, tok: Token) -> Token {
        self.pos += 1;
        tok
    }

    fn skip_comments(&mut self) {
        while self.peek(0) == Some('/') && self.peek(1) == Some('*') {
            self.pos += 2;
            loop {
                match self.peek(0) {
                    None => return,
                    Some('*') if self.peek(1) == Some('/') => {
                        self.pos += 2;
                        break;
                    }
                    _ => self.pos += 1,
                }
            }
        }
    }

    /// §4.3.8: two code points starting at offset `n` are a valid escape.
    fn valid_escape_at(&self, n: usize) -> bool {
        self.peek(n) == Some('\\') && self.peek(n + 1) != Some('\n')
    }

    /// §4.3.9: three code points starting at `n` would start an identifier.
    fn would_start_ident_at(&self, n: usize) -> bool {
        match self.peek(n) {
            Some('-') => {
                self.peek(n + 1)
                    .is_some_and(|c| is_name_start(c) || c == '-')
                    || self.valid_escape_at(n + 1)
            }
            Some(c) if is_name_start(c) => true,
            Some('\\') => self.valid_escape_at(n),
            _ => false,
        }
    }

    /// §4.3.10: three code points starting at `n` would start a number.
    fn starts_number_at(&self, n: usize) -> bool {
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
        match self.peek(n) {
            Some('+' | '-') => {
                digit(self.peek(n + 1))
                    || (self.peek(n + 1) == Some('.') && digit(self.peek(n + 2)))
            }
            Some('.') => digit(self.peek(n + 1)),
            c => digit(c),
        }
    }

    /// §4.3.11.
    fn consume_name(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                Some(c) if is_name(c) => {
                    out.push(c);
                    self.pos += 1;
                }
                Some('\\') if self.valid_escape_at(0) => {
                    self.pos += 1;
                    out.push(self.consume_escape());
                }
                _ => return out,
            }
        }
    }

    /// §4.3.7. Called after the `\` has been consumed.
    fn consume_escape(&mut self) -> char {
        match self.peek(0) {
            Some(c) if c.is_ascii_hexdigit() => {
                let mut value: u32 = 0;
                let mut n = 0;
                while n < 6
                    && let Some(d) = self.peek(0).and_then(|c| c.to_digit(16))
                {
                    value = value * 16 + d;
                    n += 1;
                    self.pos += 1;
                }
                if self.peek(0).is_some_and(is_ws) {
                    self.pos += 1;
                }
                match value {
                    0 | 0xD800..=0xDFFF => REPLACEMENT,
                    v => char::from_u32(v).unwrap_or(REPLACEMENT),
                }
            }
            None => REPLACEMENT,
            Some(c) => {
                self.pos += 1;
                c
            }
        }
    }

    /// §4.3.12.
    fn consume_number(&mut self) -> (f32, bool) {
        let start = self.pos;
        let mut is_integer = true;
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());

        if matches!(self.peek(0), Some('+' | '-')) {
            self.pos += 1;
        }
        while digit(self.peek(0)) {
            self.pos += 1;
        }
        if self.peek(0) == Some('.') && digit(self.peek(1)) {
            self.pos += 1;
            while digit(self.peek(0)) {
                self.pos += 1;
            }
            is_integer = false;
        }
        if matches!(self.peek(0), Some('e' | 'E')) {
            let n = if matches!(self.peek(1), Some('+' | '-')) {
                2
            } else {
                1
            };
            if digit(self.peek(n)) {
                self.pos += n;
                while digit(self.peek(0)) {
                    self.pos += 1;
                }
                is_integer = false;
            }
        }

        let text: String = self.chars[start..self.pos].iter().collect();
        (text.parse().unwrap_or(0.0), is_integer)
    }

    /// §4.3.3.
    fn consume_numeric(&mut self) -> Token {
        let (value, is_integer) = self.consume_number();
        if self.would_start_ident_at(0) {
            let unit = self.consume_name();
            Token::Dimension {
                value,
                is_integer,
                unit,
            }
        } else if self.peek(0) == Some('%') {
            self.pos += 1;
            Token::Percentage(value)
        } else {
            Token::Number { value, is_integer }
        }
    }

    /// §4.3.4.
    fn consume_ident_like(&mut self) -> Token {
        let name = self.consume_name();
        if self.peek(0) != Some('(') {
            return Token::Ident(name);
        }
        self.pos += 1;
        if name.eq_ignore_ascii_case("url") {
            let mut n = 0;
            while self.peek(n).is_some_and(is_ws) {
                n += 1;
            }
            if matches!(self.peek(n), Some('"' | '\'')) {
                // `url("...")` is an ordinary function whose argument is a
                // string token.
                return Token::Function(name);
            }
            return self.consume_url();
        }
        Token::Function(name)
    }

    /// §4.3.6. Called after `url(` has been consumed.
    fn consume_url(&mut self) -> Token {
        while self.peek(0).is_some_and(is_ws) {
            self.pos += 1;
        }
        let mut out = String::new();
        loop {
            match self.peek(0) {
                Some(')') => {
                    self.pos += 1;
                    return Token::Url(out);
                }
                None => return Token::Url(out),
                Some(c) if is_ws(c) => {
                    while self.peek(0).is_some_and(is_ws) {
                        self.pos += 1;
                    }
                    match self.peek(0) {
                        Some(')') => {
                            self.pos += 1;
                            return Token::Url(out);
                        }
                        None => return Token::Url(out),
                        _ => {
                            self.consume_bad_url_remnants();
                            return Token::BadUrl;
                        }
                    }
                }
                Some('"' | '\'' | '(') => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some(c) if is_non_printable(c) => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some('\\') => {
                    if self.valid_escape_at(0) {
                        self.pos += 1;
                        out.push(self.consume_escape());
                    } else {
                        self.consume_bad_url_remnants();
                        return Token::BadUrl;
                    }
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    /// §4.3.14.
    fn consume_bad_url_remnants(&mut self) {
        loop {
            match self.peek(0) {
                Some(')') => {
                    self.pos += 1;
                    return;
                }
                None => return,
                Some('\\') if self.valid_escape_at(0) => {
                    self.pos += 1;
                    self.consume_escape();
                }
                _ => self.pos += 1,
            }
        }
    }

    /// §4.3.5. Called after the opening quote has been consumed.
    fn consume_string(&mut self, quote: char) -> Token {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                None => return Token::Str(out),
                Some(c) if c == quote => {
                    self.pos += 1;
                    return Token::Str(out);
                }
                Some('\n') => return Token::BadString,
                Some('\\') => match self.peek(1) {
                    None => self.pos += 1,
                    Some('\n') => self.pos += 2,
                    _ => {
                        self.pos += 1;
                        out.push(self.consume_escape());
                    }
                },
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }
}

/// Tokenize a whole string.
pub fn tokenize(source: &str) -> Vec<Token> {
    let mut t = Tokenizer::new(source);
    let mut out = Vec::new();
    while let Some(tok) = t.next_token() {
        out.push(tok);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(s: &str) -> Token {
        Token::Ident(s.into())
    }
    fn num(v: f32) -> Token {
        Token::Number {
            value: v,
            is_integer: v.fract() == 0.0,
        }
    }
    fn dim(v: f32, u: &str) -> Token {
        Token::Dimension {
            value: v,
            is_integer: v.fract() == 0.0,
            unit: u.into(),
        }
    }

    #[test]
    fn basic_rule() {
        assert_eq!(
            tokenize("p{color:red}"),
            [
                ident("p"),
                Token::OpenCurly,
                ident("color"),
                Token::Colon,
                ident("red"),
                Token::CloseCurly
            ]
        );
    }

    #[test]
    fn numbers_and_dimensions() {
        assert_eq!(
            tokenize("1 -2.5 +.5 1e3 10px 50% 2n -n"),
            [
                num(1.0),
                Token::Whitespace,
                num(-2.5),
                Token::Whitespace,
                num(0.5),
                Token::Whitespace,
                Token::Number {
                    value: 1000.0,
                    is_integer: false
                },
                Token::Whitespace,
                dim(10.0, "px"),
                Token::Whitespace,
                Token::Percentage(50.0),
                Token::Whitespace,
                dim(2.0, "n"),
                Token::Whitespace,
                ident("-n"),
            ]
        );
    }

    #[test]
    fn hashes() {
        assert_eq!(
            tokenize("#main #1a #-x #"),
            [
                Token::Hash {
                    value: "main".into(),
                    is_id: true
                },
                Token::Whitespace,
                Token::Hash {
                    value: "1a".into(),
                    is_id: false
                },
                Token::Whitespace,
                Token::Hash {
                    value: "-x".into(),
                    is_id: true
                },
                Token::Whitespace,
                Token::Delim('#'),
            ]
        );
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(
            tokenize(
                r#""a\"b" 'c\
d' "e\41 f" "bad"#
            ),
            [
                Token::Str("a\"b".into()),
                Token::Whitespace,
                Token::Str("cd".into()),
                Token::Whitespace,
                Token::Str("eAf".into()),
                Token::Whitespace,
                Token::Str("bad".into()),
            ]
        );
        assert_eq!(
            tokenize("\"line\nbreak\""),
            [
                Token::BadString,
                Token::Whitespace,
                ident("break"),
                Token::Str(String::new())
            ]
        );
    }

    #[test]
    fn urls() {
        assert_eq!(
            tokenize("url(a.png) url( b.png ) url(\"c.png\") url(d e)"),
            [
                Token::Url("a.png".into()),
                Token::Whitespace,
                Token::Url("b.png".into()),
                Token::Whitespace,
                Token::Function("url".into()),
                Token::Str("c.png".into()),
                Token::CloseParen,
                Token::Whitespace,
                Token::BadUrl,
            ]
        );
    }

    #[test]
    fn functions_at_keywords_and_misc() {
        assert_eq!(
            tokenize("@media rgb(1,2,3) <!-- --> a>b"),
            [
                Token::AtKeyword("media".into()),
                Token::Whitespace,
                Token::Function("rgb".into()),
                num(1.0),
                Token::Comma,
                num(2.0),
                Token::Comma,
                num(3.0),
                Token::CloseParen,
                Token::Whitespace,
                Token::Cdo,
                Token::Whitespace,
                Token::Cdc,
                Token::Whitespace,
                ident("a"),
                Token::Delim('>'),
                ident("b"),
            ]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            tokenize("a/* x */b/* unterminated"),
            [ident("a"), ident("b")]
        );
    }

    #[test]
    fn custom_property_and_escaped_ident() {
        assert_eq!(
            tokenize("--x \\31 23 \\@"),
            [
                ident("--x"),
                Token::Whitespace,
                ident("123"),
                Token::Whitespace,
                ident("@")
            ]
        );
    }

    #[test]
    fn display_roundtrip() {
        // Note: an explicit `+` on a number is not preserved.
        let src = "a > b:nth-child(2n-1) { margin: -1.5px 50% }";
        let out: String = tokenize(src).iter().map(ToString::to_string).collect();
        assert_eq!(out, src);
    }

    #[test]
    fn display_escapes_so_tokens_reparse() {
        // Idents containing characters that would otherwise split or
        // change the token must serialize with escapes.
        for src in [
            "block\\a nd", // contains a newline
            "\\31 23",     // starts with a digit
            "-\\31 ",      // `-` then digit
            "a\\.b",       // contains `.`
            "\\-",         // lone hyphen
            "x\\ y",       // contains a space
            "\\@",         // single `@`
            "#\\31 23",    // hash starting with digit
            "\"a\\\"b\\\\c\"",
            "1\\65 m", // dimension with unit starting with `e`
        ] {
            let tokens = tokenize(src);
            let out: String = tokens.iter().map(ToString::to_string).collect();
            assert_eq!(
                tokenize(&out),
                tokens,
                "source {src:?} serialized to {out:?}"
            );
        }
    }
}
