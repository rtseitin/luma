//! The HTML tokenizer: a state machine over characters that produces tags,
//! comments, doctypes, and text runs.
//!
//! State names follow the WHATWG spec so the two can be read side by side.
//! Character references are resolved here, so text and attribute values
//! in emitted tokens are already decoded.

use std::collections::VecDeque;

use luma_dom::Attribute;

use crate::ParseError;
use crate::entities;

const REPLACEMENT: char = '\u{FFFD}';

/// A token produced by the [`Tokenizer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// `<!DOCTYPE ...>`.
    Doctype {
        /// Lowercased doctype name; empty if missing.
        name: String,
        /// Set when the doctype is malformed enough to force quirks mode.
        force_quirks: bool,
    },
    /// An opening tag.
    StartTag {
        /// Lowercased tag name.
        name: String,
        /// Attributes in source order, duplicates removed.
        attrs: Vec<Attribute>,
        /// Whether the tag ended with `/>`.
        self_closing: bool,
    },
    /// A closing tag.
    EndTag {
        /// Lowercased tag name.
        name: String,
    },
    /// `<!-- ... -->`, with the delimiters stripped.
    Comment(String),
    /// A run of character data. Adjacent characters are batched into one
    /// token; the tree builder merges adjacent text nodes regardless.
    Text(String),
    /// End of input. Returned forever once reached.
    Eof,
}

/// How the tokenizer should treat the content it is about to see.
///
/// The tree builder switches this after start tags whose content is not
/// markup (`<script>`, `<style>`, `<title>`, `<textarea>`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMode {
    /// Normal markup.
    Data,
    /// Text with character references but no tags (`title`, `textarea`).
    RcData,
    /// Raw text; only the matching end tag is recognized (`script`, `style`).
    RawText,
    /// Everything until end of input is text (`plaintext`).
    PlainText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    RcData,
    RawText,
    PlainText,
    TagOpen,
    EndTagOpen,
    TagName,
    // Shared by RCDATA and RAWTEXT; `raw_kind` says which to return to.
    RawLessThan,
    RawEndTagOpen,
    RawEndTagName,
    BeforeAttrName,
    AttrName,
    AfterAttrName,
    BeforeAttrValue,
    AttrValueDq,
    AttrValueSq,
    AttrValueUnquoted,
    AfterAttrValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    BogusDoctype,
    CharRef,
}

#[derive(Default)]
struct Tag {
    name: String,
    attrs: Vec<Attribute>,
    self_closing: bool,
    is_end: bool,
    attr_name: String,
    attr_value: String,
    in_attr: bool,
}

impl Tag {
    fn start(is_end: bool) -> Self {
        Self {
            is_end,
            ..Self::default()
        }
    }
}

/// Streaming HTML tokenizer.
///
/// Pull tokens with [`next_token`](Self::next_token). Input is fully
/// buffered up front; newlines are normalized to `\n` as the spec requires.
pub struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
    state: State,
    return_state: State,
    raw_kind: State,
    tag: Tag,
    comment: String,
    doctype_name: String,
    doctype_quirks: bool,
    last_start_tag: String,
    temp: String,
    text: String,
    pending: VecDeque<Token>,
    errors: Vec<ParseError>,
    eof_emitted: bool,
}

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | ' ')
}

impl Tokenizer {
    /// Create a tokenizer over `source`.
    pub fn new(source: &str) -> Self {
        // Preprocess: CRLF and lone CR become LF.
        let mut chars = Vec::with_capacity(source.len());
        let mut it = source.chars().peekable();
        while let Some(c) = it.next() {
            if c == '\r' {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                chars.push('\n');
            } else {
                chars.push(c);
            }
        }
        Self {
            chars,
            pos: 0,
            state: State::Data,
            return_state: State::Data,
            raw_kind: State::RawText,
            tag: Tag::default(),
            comment: String::new(),
            doctype_name: String::new(),
            doctype_quirks: false,
            last_start_tag: String::new(),
            temp: String::new(),
            text: String::new(),
            pending: VecDeque::new(),
            errors: Vec::new(),
            eof_emitted: false,
        }
    }

    /// Produce the next token. Returns [`Token::Eof`] forever once the
    /// input is exhausted.
    pub fn next_token(&mut self) -> Token {
        loop {
            if let Some(t) = self.pending.pop_front() {
                return t;
            }
            if self.eof_emitted {
                return Token::Eof;
            }
            self.step();
        }
    }

    /// Switch content mode. Called by the tree builder after start tags
    /// like `<script>` whose content is not markup.
    pub fn set_text_mode(&mut self, mode: TextMode) {
        self.state = match mode {
            TextMode::Data => State::Data,
            TextMode::RcData => State::RcData,
            TextMode::RawText => State::RawText,
            TextMode::PlainText => State::PlainText,
        };
    }

    /// Current character offset into the normalized input.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Take the parse errors recorded so far.
    pub fn take_errors(&mut self) -> Vec<ParseError> {
        std::mem::take(&mut self.errors)
    }

    // ----- input helpers ----------------------------------------------------

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn consume(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// Put `c` back (if it was a real character) and switch state.
    fn reconsume(&mut self, c: Option<char>, state: State) {
        if c.is_some() {
            self.pos -= 1;
        }
        self.state = state;
    }

    fn starts_with_ci(&self, s: &str) -> bool {
        let mut i = self.pos;
        for sc in s.chars() {
            match self.chars.get(i) {
                Some(c) if c.eq_ignore_ascii_case(&sc) => i += 1,
                _ => return false,
            }
        }
        true
    }

    fn error(&mut self, message: &'static str) {
        self.errors.push(ParseError {
            offset: self.pos,
            message,
        });
    }

    // ----- output helpers ---------------------------------------------------

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let t = std::mem::take(&mut self.text);
            self.pending.push_back(Token::Text(t));
        }
    }

    fn emit(&mut self, tok: Token) {
        self.flush_text();
        self.pending.push_back(tok);
    }

    fn emit_eof(&mut self) {
        self.flush_text();
        self.pending.push_back(Token::Eof);
        self.eof_emitted = true;
    }

    fn emit_tag(&mut self) {
        self.finish_attr();
        let tag = std::mem::take(&mut self.tag);
        if tag.is_end {
            if !tag.attrs.is_empty() {
                self.error("end-tag-with-attributes");
            }
            if tag.self_closing {
                self.error("end-tag-with-trailing-solidus");
            }
            self.emit(Token::EndTag { name: tag.name });
        } else {
            self.last_start_tag.clone_from(&tag.name);
            self.emit(Token::StartTag {
                name: tag.name,
                attrs: tag.attrs,
                self_closing: tag.self_closing,
            });
        }
    }

    fn start_attr(&mut self) {
        self.finish_attr();
        self.tag.in_attr = true;
    }

    fn finish_attr(&mut self) {
        if !self.tag.in_attr {
            return;
        }
        self.tag.in_attr = false;
        let name = std::mem::take(&mut self.tag.attr_name);
        let value = std::mem::take(&mut self.tag.attr_value);
        if self.tag.attrs.iter().any(|a| a.name == name) {
            self.error("duplicate-attribute");
        } else {
            self.tag.attrs.push(Attribute::new(name, value));
        }
    }

    fn emit_comment(&mut self) {
        let c = std::mem::take(&mut self.comment);
        self.emit(Token::Comment(c));
    }

    fn emit_doctype(&mut self) {
        let name = std::mem::take(&mut self.doctype_name);
        let force_quirks = std::mem::take(&mut self.doctype_quirks);
        self.emit(Token::Doctype { name, force_quirks });
    }

    fn is_appropriate_end_tag(&self) -> bool {
        self.tag.is_end && self.tag.name == self.last_start_tag
    }

    // ----- state dispatch ---------------------------------------------------

    fn step(&mut self) {
        match self.state {
            State::Data => self.data(),
            State::RcData => self.raw(State::RcData),
            State::RawText => self.raw(State::RawText),
            State::PlainText => self.raw(State::PlainText),
            State::TagOpen => self.tag_open(),
            State::EndTagOpen => self.end_tag_open(),
            State::TagName => self.tag_name(),
            State::RawLessThan => self.raw_less_than(),
            State::RawEndTagOpen => self.raw_end_tag_open(),
            State::RawEndTagName => self.raw_end_tag_name(),
            State::BeforeAttrName => self.before_attr_name(),
            State::AttrName => self.attr_name(),
            State::AfterAttrName => self.after_attr_name(),
            State::BeforeAttrValue => self.before_attr_value(),
            State::AttrValueDq => self.attr_value_quoted('"'),
            State::AttrValueSq => self.attr_value_quoted('\''),
            State::AttrValueUnquoted => self.attr_value_unquoted(),
            State::AfterAttrValueQuoted => self.after_attr_value_quoted(),
            State::SelfClosingStartTag => self.self_closing_start_tag(),
            State::BogusComment => self.bogus_comment(),
            State::MarkupDeclarationOpen => self.markup_declaration_open(),
            State::CommentStart => self.comment_start(),
            State::CommentStartDash => self.comment_start_dash(),
            State::Comment => self.comment(),
            State::CommentEndDash => self.comment_end_dash(),
            State::CommentEnd => self.comment_end(),
            State::CommentEndBang => self.comment_end_bang(),
            State::Doctype => self.doctype(),
            State::BeforeDoctypeName => self.before_doctype_name(),
            State::DoctypeName => self.doctype_name(),
            State::AfterDoctypeName => self.after_doctype_name(),
            State::BogusDoctype => self.bogus_doctype(),
            State::CharRef => self.char_ref(),
        }
    }

    // ----- text states ------------------------------------------------------

    fn data(&mut self) {
        match self.consume() {
            Some('&') => {
                self.return_state = State::Data;
                self.state = State::CharRef;
            }
            Some('<') => self.state = State::TagOpen,
            Some('\0') => {
                self.error("unexpected-null-character");
                self.text.push('\0');
            }
            None => self.emit_eof(),
            Some(c) => {
                self.text.push(c);
                // Fast path: swallow the rest of the plain run.
                while let Some(c) = self.peek() {
                    if matches!(c, '&' | '<' | '\0') {
                        break;
                    }
                    self.text.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn raw(&mut self, kind: State) {
        match self.consume() {
            Some('&') if kind == State::RcData => {
                self.return_state = State::RcData;
                self.state = State::CharRef;
            }
            Some('<') if kind != State::PlainText => {
                self.raw_kind = kind;
                self.state = State::RawLessThan;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.text.push(REPLACEMENT);
            }
            None => self.emit_eof(),
            Some(c) => self.text.push(c),
        }
    }

    fn raw_less_than(&mut self) {
        match self.consume() {
            Some('/') => {
                self.temp.clear();
                self.state = State::RawEndTagOpen;
            }
            c => {
                self.text.push('<');
                self.reconsume(c, self.raw_kind);
            }
        }
    }

    fn raw_end_tag_open(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.tag = Tag::start(true);
                self.reconsume(Some(c), State::RawEndTagName);
            }
            c => {
                self.text.push_str("</");
                self.reconsume(c, self.raw_kind);
            }
        }
    }

    fn raw_end_tag_name(&mut self) {
        let c = self.consume();
        match c {
            Some(ch) if is_ws(ch) && self.is_appropriate_end_tag() => {
                self.state = State::BeforeAttrName;
            }
            Some('/') if self.is_appropriate_end_tag() => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') if self.is_appropriate_end_tag() => {
                self.emit_tag();
                self.state = State::Data;
            }
            Some(ch) if ch.is_ascii_alphabetic() => {
                self.tag.name.push(ch.to_ascii_lowercase());
                self.temp.push(ch);
            }
            _ => {
                self.text.push_str("</");
                let temp = std::mem::take(&mut self.temp);
                self.text.push_str(&temp);
                self.reconsume(c, self.raw_kind);
            }
        }
    }

    // ----- tag states -------------------------------------------------------

    fn tag_open(&mut self) {
        match self.consume() {
            Some('!') => self.state = State::MarkupDeclarationOpen,
            Some('/') => self.state = State::EndTagOpen,
            Some(c) if c.is_ascii_alphabetic() => {
                self.tag = Tag::start(false);
                self.reconsume(Some(c), State::TagName);
            }
            Some('?') => {
                self.error("unexpected-question-mark-instead-of-tag-name");
                self.comment.clear();
                self.reconsume(Some('?'), State::BogusComment);
            }
            None => {
                self.error("eof-before-tag-name");
                self.text.push('<');
                self.emit_eof();
            }
            c => {
                self.error("invalid-first-character-of-tag-name");
                self.text.push('<');
                self.reconsume(c, State::Data);
            }
        }
    }

    fn end_tag_open(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.tag = Tag::start(true);
                self.reconsume(Some(c), State::TagName);
            }
            Some('>') => {
                self.error("missing-end-tag-name");
                self.state = State::Data;
            }
            None => {
                self.error("eof-before-tag-name");
                self.text.push_str("</");
                self.emit_eof();
            }
            c => {
                self.error("invalid-first-character-of-tag-name");
                self.comment.clear();
                self.reconsume(c, State::BogusComment);
            }
        }
    }

    fn tag_name(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => self.state = State::BeforeAttrName,
            Some('/') => self.state = State::SelfClosingStartTag,
            Some('>') => {
                self.emit_tag();
                self.state = State::Data;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.tag.name.push(REPLACEMENT);
            }
            None => {
                self.error("eof-in-tag");
                self.emit_eof();
            }
            Some(c) => self.tag.name.push(c.to_ascii_lowercase()),
        }
    }

    fn before_attr_name(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => {}
            c @ (Some('/') | Some('>') | None) => self.reconsume(c, State::AfterAttrName),
            Some('=') => {
                self.error("unexpected-equals-sign-before-attribute-name");
                self.start_attr();
                self.tag.attr_name.push('=');
                self.state = State::AttrName;
            }
            c => {
                self.start_attr();
                self.reconsume(c, State::AttrName);
            }
        }
    }

    fn attr_name(&mut self) {
        match self.consume() {
            c @ (Some('/') | Some('>') | None) => self.reconsume(c, State::AfterAttrName),
            Some(c) if is_ws(c) => self.reconsume(Some(c), State::AfterAttrName),
            Some('=') => self.state = State::BeforeAttrValue,
            Some('\0') => {
                self.error("unexpected-null-character");
                self.tag.attr_name.push(REPLACEMENT);
            }
            Some(c @ ('"' | '\'' | '<')) => {
                self.error("unexpected-character-in-attribute-name");
                self.tag.attr_name.push(c);
            }
            Some(c) => self.tag.attr_name.push(c.to_ascii_lowercase()),
        }
    }

    fn after_attr_name(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => {}
            Some('/') => self.state = State::SelfClosingStartTag,
            Some('=') => self.state = State::BeforeAttrValue,
            Some('>') => {
                self.emit_tag();
                self.state = State::Data;
            }
            None => {
                self.error("eof-in-tag");
                self.emit_eof();
            }
            c => {
                self.start_attr();
                self.reconsume(c, State::AttrName);
            }
        }
    }

    fn before_attr_value(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => {}
            Some('"') => self.state = State::AttrValueDq,
            Some('\'') => self.state = State::AttrValueSq,
            Some('>') => {
                self.error("missing-attribute-value");
                self.emit_tag();
                self.state = State::Data;
            }
            c => self.reconsume(c, State::AttrValueUnquoted),
        }
    }

    fn attr_value_quoted(&mut self, quote: char) {
        match self.consume() {
            Some(c) if c == quote => self.state = State::AfterAttrValueQuoted,
            Some('&') => {
                self.return_state = self.state;
                self.state = State::CharRef;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.tag.attr_value.push(REPLACEMENT);
            }
            None => {
                self.error("eof-in-tag");
                self.emit_eof();
            }
            Some(c) => self.tag.attr_value.push(c),
        }
    }

    fn attr_value_unquoted(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => self.state = State::BeforeAttrName,
            Some('&') => {
                self.return_state = State::AttrValueUnquoted;
                self.state = State::CharRef;
            }
            Some('>') => {
                self.emit_tag();
                self.state = State::Data;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.tag.attr_value.push(REPLACEMENT);
            }
            Some(c @ ('"' | '\'' | '<' | '=' | '`')) => {
                self.error("unexpected-character-in-unquoted-attribute-value");
                self.tag.attr_value.push(c);
            }
            None => {
                self.error("eof-in-tag");
                self.emit_eof();
            }
            Some(c) => self.tag.attr_value.push(c),
        }
    }

    fn after_attr_value_quoted(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => self.state = State::BeforeAttrName,
            Some('/') => self.state = State::SelfClosingStartTag,
            Some('>') => {
                self.emit_tag();
                self.state = State::Data;
            }
            None => {
                self.error("eof-in-tag");
                self.emit_eof();
            }
            c => {
                self.error("missing-whitespace-between-attributes");
                self.reconsume(c, State::BeforeAttrName);
            }
        }
    }

    fn self_closing_start_tag(&mut self) {
        match self.consume() {
            Some('>') => {
                self.tag.self_closing = true;
                self.emit_tag();
                self.state = State::Data;
            }
            None => {
                self.error("eof-in-tag");
                self.emit_eof();
            }
            c => {
                self.error("unexpected-solidus-in-tag");
                self.reconsume(c, State::BeforeAttrName);
            }
        }
    }

    // ----- comment and doctype states ---------------------------------------

    fn bogus_comment(&mut self) {
        match self.consume() {
            Some('>') => {
                self.emit_comment();
                self.state = State::Data;
            }
            None => {
                self.emit_comment();
                self.emit_eof();
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.comment.push(REPLACEMENT);
            }
            Some(c) => self.comment.push(c),
        }
    }

    fn markup_declaration_open(&mut self) {
        if self.starts_with_ci("--") {
            self.pos += 2;
            self.comment.clear();
            self.state = State::CommentStart;
        } else if self.starts_with_ci("DOCTYPE") {
            self.pos += 7;
            self.state = State::Doctype;
        } else if self.starts_with_ci("[CDATA[") {
            // Only valid in foreign content, which we do not model.
            self.error("cdata-in-html-content");
            self.pos += 7;
            self.comment = "[CDATA[".to_owned();
            self.state = State::BogusComment;
        } else {
            self.error("incorrectly-opened-comment");
            self.comment.clear();
            self.state = State::BogusComment;
        }
    }

    fn comment_start(&mut self) {
        match self.consume() {
            Some('-') => self.state = State::CommentStartDash,
            Some('>') => {
                self.error("abrupt-closing-of-empty-comment");
                self.emit_comment();
                self.state = State::Data;
            }
            c => self.reconsume(c, State::Comment),
        }
    }

    fn comment_start_dash(&mut self) {
        match self.consume() {
            Some('-') => self.state = State::CommentEnd,
            Some('>') => {
                self.error("abrupt-closing-of-empty-comment");
                self.emit_comment();
                self.state = State::Data;
            }
            None => {
                self.error("eof-in-comment");
                self.emit_comment();
                self.emit_eof();
            }
            c => {
                self.comment.push('-');
                self.reconsume(c, State::Comment);
            }
        }
    }

    fn comment(&mut self) {
        match self.consume() {
            Some('-') => self.state = State::CommentEndDash,
            Some('\0') => {
                self.error("unexpected-null-character");
                self.comment.push(REPLACEMENT);
            }
            None => {
                self.error("eof-in-comment");
                self.emit_comment();
                self.emit_eof();
            }
            Some(c) => self.comment.push(c),
        }
    }

    fn comment_end_dash(&mut self) {
        match self.consume() {
            Some('-') => self.state = State::CommentEnd,
            None => {
                self.error("eof-in-comment");
                self.emit_comment();
                self.emit_eof();
            }
            c => {
                self.comment.push('-');
                self.reconsume(c, State::Comment);
            }
        }
    }

    fn comment_end(&mut self) {
        match self.consume() {
            Some('>') => {
                self.emit_comment();
                self.state = State::Data;
            }
            Some('!') => self.state = State::CommentEndBang,
            Some('-') => self.comment.push('-'),
            None => {
                self.error("eof-in-comment");
                self.emit_comment();
                self.emit_eof();
            }
            c => {
                self.comment.push_str("--");
                self.reconsume(c, State::Comment);
            }
        }
    }

    fn comment_end_bang(&mut self) {
        match self.consume() {
            Some('-') => {
                self.comment.push_str("--!");
                self.state = State::CommentEndDash;
            }
            Some('>') => {
                self.error("incorrectly-closed-comment");
                self.emit_comment();
                self.state = State::Data;
            }
            None => {
                self.error("eof-in-comment");
                self.emit_comment();
                self.emit_eof();
            }
            c => {
                self.comment.push_str("--!");
                self.reconsume(c, State::Comment);
            }
        }
    }

    fn doctype(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => self.state = State::BeforeDoctypeName,
            Some('>') => self.reconsume(Some('>'), State::BeforeDoctypeName),
            None => {
                self.error("eof-in-doctype");
                self.doctype_quirks = true;
                self.emit_doctype();
                self.emit_eof();
            }
            c => {
                self.error("missing-whitespace-before-doctype-name");
                self.reconsume(c, State::BeforeDoctypeName);
            }
        }
    }

    fn before_doctype_name(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => {}
            Some('\0') => {
                self.error("unexpected-null-character");
                self.doctype_name.clear();
                self.doctype_name.push(REPLACEMENT);
                self.state = State::DoctypeName;
            }
            Some('>') => {
                self.error("missing-doctype-name");
                self.doctype_quirks = true;
                self.emit_doctype();
                self.state = State::Data;
            }
            None => {
                self.error("eof-in-doctype");
                self.doctype_quirks = true;
                self.emit_doctype();
                self.emit_eof();
            }
            Some(c) => {
                self.doctype_name.clear();
                self.doctype_name.push(c.to_ascii_lowercase());
                self.state = State::DoctypeName;
            }
        }
    }

    fn doctype_name(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => self.state = State::AfterDoctypeName,
            Some('>') => {
                self.emit_doctype();
                self.state = State::Data;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.doctype_name.push(REPLACEMENT);
            }
            None => {
                self.error("eof-in-doctype");
                self.doctype_quirks = true;
                self.emit_doctype();
                self.emit_eof();
            }
            Some(c) => self.doctype_name.push(c.to_ascii_lowercase()),
        }
    }

    fn after_doctype_name(&mut self) {
        match self.consume() {
            Some(c) if is_ws(c) => {}
            Some('>') => {
                self.emit_doctype();
                self.state = State::Data;
            }
            None => {
                self.error("eof-in-doctype");
                self.doctype_quirks = true;
                self.emit_doctype();
                self.emit_eof();
            }
            c => {
                // PUBLIC/SYSTEM identifiers are not modelled: skip to `>`.
                // Anything else is junk that forces quirks mode.
                self.reconsume(c, State::BogusDoctype);
                if self.starts_with_ci("PUBLIC") || self.starts_with_ci("SYSTEM") {
                    self.pos += 6;
                } else {
                    self.error("invalid-character-sequence-after-doctype-name");
                    self.doctype_quirks = true;
                }
            }
        }
    }

    fn bogus_doctype(&mut self) {
        match self.consume() {
            Some('>') => {
                self.emit_doctype();
                self.state = State::Data;
            }
            None => {
                self.emit_doctype();
                self.emit_eof();
            }
            Some(_) => {}
        }
    }

    // ----- character references ---------------------------------------------

    fn in_attr_ref(&self) -> bool {
        matches!(
            self.return_state,
            State::AttrValueDq | State::AttrValueSq | State::AttrValueUnquoted
        )
    }

    fn push_ref(&mut self, s: &str) {
        if self.in_attr_ref() {
            self.tag.attr_value.push_str(s);
        } else {
            self.text.push_str(s);
        }
    }

    fn char_ref(&mut self) {
        // We are positioned just after the `&`.
        self.state = self.return_state;
        match self.peek() {
            Some('#') => self.numeric_ref(),
            Some(c) if c.is_ascii_alphanumeric() => self.named_ref(),
            _ => self.push_ref("&"),
        }
    }

    fn named_ref(&mut self) {
        let Some((len, value)) = entities::lookup(&self.chars[self.pos..]) else {
            // Unknown reference: leave the `&` and let the following
            // characters be processed as ordinary text.
            self.push_ref("&");
            return;
        };
        let ends_with_semi = self.chars[self.pos + len - 1] == ';';
        if self.in_attr_ref() && !ends_with_semi {
            // Historical quirk: `&copy=1` inside an attribute is a literal
            // `&copy=1`, so query strings survive.
            let next = self.chars.get(self.pos + len).copied();
            if next == Some('=') || next.is_some_and(|c| c.is_ascii_alphanumeric()) {
                self.push_ref("&");
                return;
            }
        }
        if !ends_with_semi {
            self.error("missing-semicolon-after-character-reference");
        }
        self.pos += len;
        self.push_ref(value);
    }

    fn numeric_ref(&mut self) {
        let start = self.pos; // at '#'
        self.pos += 1;
        let hex = matches!(self.peek(), Some('x' | 'X'));
        if hex {
            self.pos += 1;
        }
        let radix = if hex { 16 } else { 10 };

        let mut value: u32 = 0;
        let mut digits = 0;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(radix)) {
            value = value.saturating_mul(radix).saturating_add(d);
            digits += 1;
            self.pos += 1;
        }

        if digits == 0 {
            self.error("absence-of-digits-in-numeric-character-reference");
            let literal: String = std::iter::once('&')
                .chain(self.chars[start..self.pos].iter().copied())
                .collect();
            self.push_ref(&literal);
            return;
        }

        if self.peek() == Some(';') {
            self.pos += 1;
        } else {
            self.error("missing-semicolon-after-character-reference");
        }

        let ch = match value {
            0 => {
                self.error("null-character-reference");
                REPLACEMENT
            }
            0xD800..=0xDFFF => {
                self.error("surrogate-character-reference");
                REPLACEMENT
            }
            v if v > 0x10FFFF => {
                self.error("character-reference-outside-unicode-range");
                REPLACEMENT
            }
            0x80..=0x9F => {
                self.error("control-character-reference");
                entities::c1_remap(value).unwrap_or_else(|| char::from_u32(value).unwrap())
            }
            v => char::from_u32(v).unwrap_or(REPLACEMENT),
        };
        let mut buf = [0u8; 4];
        self.push_ref(ch.encode_utf8(&mut buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &str) -> Vec<Token> {
        let mut t = Tokenizer::new(src);
        let mut out = vec![];
        loop {
            let tok = t.next_token();
            let eof = tok == Token::Eof;
            out.push(tok);
            if eof {
                break;
            }
        }
        out
    }

    fn start(name: &str, attrs: &[(&str, &str)]) -> Token {
        Token::StartTag {
            name: name.into(),
            attrs: attrs.iter().map(|(n, v)| Attribute::new(*n, *v)).collect(),
            self_closing: false,
        }
    }

    fn end(name: &str) -> Token {
        Token::EndTag { name: name.into() }
    }

    fn text(s: &str) -> Token {
        Token::Text(s.into())
    }

    #[test]
    fn simple_tags_and_text() {
        assert_eq!(
            tokens("<p>Hi</p>"),
            [start("p", &[]), text("Hi"), end("p"), Token::Eof]
        );
    }

    #[test]
    fn tag_names_and_attr_names_are_lowercased() {
        assert_eq!(
            tokens("<DIV ID=a Class='b c' data-X=\"d\">"),
            [
                start("div", &[("id", "a"), ("class", "b c"), ("data-x", "d")]),
                Token::Eof
            ]
        );
    }

    #[test]
    fn boolean_and_duplicate_attributes() {
        let mut t = Tokenizer::new("<input disabled id=a id=b>");
        assert_eq!(
            t.next_token(),
            start("input", &[("disabled", ""), ("id", "a")])
        );
        assert_eq!(t.take_errors().len(), 1);
    }

    #[test]
    fn self_closing_flag() {
        assert_eq!(
            tokens("<br/><img src=x />"),
            [
                Token::StartTag {
                    name: "br".into(),
                    attrs: vec![],
                    self_closing: true
                },
                Token::StartTag {
                    name: "img".into(),
                    attrs: vec![Attribute::new("src", "x")],
                    self_closing: true
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn stray_less_than_is_text() {
        assert_eq!(tokens("a < b <> c"), [text("a < b <> c"), Token::Eof]);
    }

    #[test]
    fn empty_end_tag_is_dropped() {
        assert_eq!(tokens("a</>b"), [text("ab"), Token::Eof]);
    }

    #[test]
    fn comments() {
        assert_eq!(
            tokens("<!-- a -- b --><!---->x<!--->"),
            [
                Token::Comment(" a -- b ".into()),
                Token::Comment(String::new()),
                text("x"),
                Token::Comment(String::new()),
                Token::Eof
            ]
        );
    }

    #[test]
    fn bogus_comments() {
        assert_eq!(
            tokens("<?xml version?><!ELEMENT x></#junk>"),
            [
                Token::Comment("?xml version?".into()),
                Token::Comment("ELEMENT x".into()),
                Token::Comment("#junk".into()),
                Token::Eof
            ]
        );
    }

    #[test]
    fn unterminated_comment_at_eof() {
        assert_eq!(
            tokens("<p><!-- oops"),
            [start("p", &[]), Token::Comment(" oops".into()), Token::Eof]
        );
    }

    #[test]
    fn eof_in_tag_drops_the_tag() {
        assert_eq!(tokens("x<a href=\"y"), [text("x"), Token::Eof]);
        assert_eq!(tokens("x<a hr"), [text("x"), Token::Eof]);
        assert_eq!(tokens("x<"), [text("x<"), Token::Eof]);
        assert_eq!(tokens("x</"), [text("x</"), Token::Eof]);
    }

    #[test]
    fn doctype() {
        assert_eq!(
            tokens("<!DOCTYPE html>"),
            [
                Token::Doctype {
                    name: "html".into(),
                    force_quirks: false
                },
                Token::Eof
            ]
        );
        assert_eq!(
            tokens("<!doctype HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\">"),
            [
                Token::Doctype {
                    name: "html".into(),
                    force_quirks: false
                },
                Token::Eof
            ]
        );
        assert_eq!(
            tokens("<!DOCTYPE>"),
            [
                Token::Doctype {
                    name: String::new(),
                    force_quirks: true
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn character_references_in_text() {
        assert_eq!(
            tokens("&lt;&amp;&gt; &copy &#65;&#x42;&#67 &bogus; &"),
            [text("<&> \u{A9} ABC &bogus; &"), Token::Eof]
        );
    }

    #[test]
    fn numeric_reference_edge_cases() {
        assert_eq!(
            tokens("&#0;&#xD800;&#x110000;&#x80;&#;&#x;"),
            [text("\u{FFFD}\u{FFFD}\u{FFFD}\u{20AC}&#;&#x;"), Token::Eof]
        );
    }

    #[test]
    fn character_references_in_attributes() {
        assert_eq!(
            tokens("<a href=\"?a=1&copy=2&amp;b=3&lt\">"),
            [start("a", &[("href", "?a=1&copy=2&b=3<")]), Token::Eof]
        );
    }

    #[test]
    fn rawtext_ignores_markup_until_matching_end_tag() {
        let mut t = Tokenizer::new("<script>if (a < b) { x = \"</p>\"; }</script>after");
        assert_eq!(t.next_token(), start("script", &[]));
        t.set_text_mode(TextMode::RawText);
        assert_eq!(t.next_token(), text("if (a < b) { x = \"</p>\"; }"));
        assert_eq!(t.next_token(), end("script"));
        assert_eq!(t.next_token(), text("after"));
        assert_eq!(t.next_token(), Token::Eof);
    }

    #[test]
    fn rcdata_decodes_references_but_not_tags() {
        let mut t = Tokenizer::new("<title>A &amp; <b>B</b></title>");
        assert_eq!(t.next_token(), start("title", &[]));
        t.set_text_mode(TextMode::RcData);
        assert_eq!(t.next_token(), text("A & <b>B</b>"));
        assert_eq!(t.next_token(), end("title"));
    }

    #[test]
    fn rawtext_end_tag_must_match() {
        let mut t = Tokenizer::new("<style></script></style>");
        assert_eq!(t.next_token(), start("style", &[]));
        t.set_text_mode(TextMode::RawText);
        assert_eq!(t.next_token(), text("</script>"));
        assert_eq!(t.next_token(), end("style"));
    }

    #[test]
    fn plaintext_runs_to_eof() {
        let mut t = Tokenizer::new("<plaintext></plaintext><b>");
        assert_eq!(t.next_token(), start("plaintext", &[]));
        t.set_text_mode(TextMode::PlainText);
        assert_eq!(t.next_token(), text("</plaintext><b>"));
        assert_eq!(t.next_token(), Token::Eof);
    }

    #[test]
    fn newlines_are_normalized() {
        assert_eq!(tokens("a\r\nb\rc\n"), [text("a\nb\nc\n"), Token::Eof]);
    }

    #[test]
    fn eof_is_sticky() {
        let mut t = Tokenizer::new("");
        assert_eq!(t.next_token(), Token::Eof);
        assert_eq!(t.next_token(), Token::Eof);
    }
}
