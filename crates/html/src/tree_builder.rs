//! The tree builder: consumes tokens and constructs a [`Document`].
//!
//! Structure follows the WHATWG "tree construction" section. Each insertion
//! mode is a method; the stack of open elements and the list of active
//! formatting elements hold [`NodeId`]s into the document arena. See the
//! crate docs for the list of simplifications.

use luma_dom::{Attribute, Document, Element, NodeId};

use crate::ParseError;
use crate::tokenizer::{TextMode, Token, Tokenizer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    Text,
    AfterBody,
    AfterAfterBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FmtEntry {
    Marker,
    Element(NodeId),
}

#[derive(Debug, Clone, Copy)]
enum Scope {
    Default,
    ListItem,
    Button,
    Table,
}

pub(crate) struct TreeBuilder {
    tokenizer: Tokenizer,
    doc: Document,
    mode: Mode,
    original_mode: Mode,
    /// Stack of open elements. `open[0]` is the `html` element once it
    /// exists.
    open: Vec<NodeId>,
    /// List of active formatting elements.
    formatting: Vec<FmtEntry>,
    head: Option<NodeId>,
    /// Set after `<pre>`, `<listing>`, `<textarea>`: a directly following
    /// newline is dropped.
    ignore_lf: bool,
    errors: Vec<ParseError>,
}

// ----- element classification -----------------------------------------------

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ')
}

fn split_leading_ws(s: &str) -> (&str, &str) {
    let idx = s.find(|c| !is_ws(c)).unwrap_or(s.len());
    s.split_at(idx)
}

/// The spec's "special" category.
fn is_special(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "applet"
            | "area"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "bgsound"
            | "blockquote"
            | "body"
            | "br"
            | "button"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "embed"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hgroup"
            | "hr"
            | "html"
            | "iframe"
            | "img"
            | "input"
            | "keygen"
            | "li"
            | "link"
            | "listing"
            | "main"
            | "marquee"
            | "menu"
            | "meta"
            | "nav"
            | "noembed"
            | "noframes"
            | "noscript"
            | "object"
            | "ol"
            | "p"
            | "param"
            | "plaintext"
            | "pre"
            | "script"
            | "search"
            | "section"
            | "select"
            | "source"
            | "style"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "template"
            | "textarea"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
            | "wbr"
            | "xmp"
    )
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn is_formatting(name: &str) -> bool {
    matches!(
        name,
        "a" | "b"
            | "big"
            | "code"
            | "em"
            | "font"
            | "i"
            | "nobr"
            | "s"
            | "small"
            | "strike"
            | "strong"
            | "tt"
            | "u"
    )
}

/// Start tags that implicitly close an open `<p>` and open a block.
fn is_block_start(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "center"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "header"
            | "hgroup"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "search"
            | "section"
            | "summary"
            | "ul"
    )
}

/// Elements that are popped by "generate implied end tags".
fn is_implied_end(name: &str) -> bool {
    matches!(
        name,
        "dd" | "dt" | "li" | "optgroup" | "option" | "p" | "rb" | "rp" | "rt" | "rtc"
    )
}

fn is_scope_boundary(name: &str, scope: Scope) -> bool {
    match scope {
        Scope::Table => matches!(name, "html" | "table" | "template"),
        _ => {
            let base = matches!(
                name,
                "applet"
                    | "caption"
                    | "html"
                    | "table"
                    | "td"
                    | "th"
                    | "marquee"
                    | "object"
                    | "template"
            );
            base || match scope {
                Scope::ListItem => matches!(name, "ol" | "ul"),
                Scope::Button => name == "button",
                _ => false,
            }
        }
    }
}

fn same_attrs(a: &[Attribute], b: &[Attribute]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

// ----- the builder ------------------------------------------------------------

impl TreeBuilder {
    pub(crate) fn new(tokenizer: Tokenizer) -> Self {
        Self {
            tokenizer,
            doc: Document::new(),
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            open: Vec::new(),
            formatting: Vec::new(),
            head: None,
            ignore_lf: false,
            errors: Vec::new(),
        }
    }

    pub(crate) fn run(mut self) -> (Document, Vec<ParseError>) {
        loop {
            let tok = self.tokenizer.next_token();
            let eof = tok == Token::Eof;
            self.process(tok);
            if eof {
                break;
            }
        }
        let mut errors = self.tokenizer.take_errors();
        errors.append(&mut self.errors);
        errors.sort_by_key(|e| e.offset);
        (self.doc, errors)
    }

    fn error(&mut self, message: &'static str) {
        self.errors.push(ParseError {
            offset: self.tokenizer.offset(),
            message,
        });
    }

    // ----- stack helpers ----------------------------------------------------

    fn name_of(&self, id: NodeId) -> &str {
        self.doc.element(id).map_or("", |e| e.name.as_str())
    }

    fn current(&self) -> NodeId {
        self.open.last().copied().unwrap_or_else(|| self.doc.root())
    }

    fn current_name(&self) -> &str {
        self.open.last().map_or("", |&id| self.name_of(id))
    }

    fn in_scope_by(&self, pred: impl Fn(&str) -> bool, scope: Scope) -> bool {
        for &id in self.open.iter().rev() {
            let n = self.name_of(id);
            if pred(n) {
                return true;
            }
            if is_scope_boundary(n, scope) {
                return false;
            }
        }
        false
    }

    fn in_scope(&self, target: &str, scope: Scope) -> bool {
        self.in_scope_by(|n| n == target, scope)
    }

    fn element_in_scope(&self, target: NodeId, scope: Scope) -> bool {
        for &id in self.open.iter().rev() {
            if id == target {
                return true;
            }
            if is_scope_boundary(self.name_of(id), scope) {
                return false;
            }
        }
        false
    }

    /// Pop elements up to and including the first one named `name`.
    fn pop_until(&mut self, name: &str) {
        while let Some(id) = self.open.pop() {
            if self.name_of(id) == name {
                break;
            }
        }
    }

    fn pop_until_by(&mut self, pred: impl Fn(&str) -> bool) {
        while let Some(id) = self.open.pop() {
            if pred(self.name_of(id)) {
                break;
            }
        }
    }

    /// Pop elements until the current node satisfies `pred` (not popped).
    /// The `html` element is never popped.
    fn pop_while_not(&mut self, pred: impl Fn(&str) -> bool) {
        while self.open.len() > 1 && !pred(self.current_name()) {
            self.open.pop();
        }
    }

    fn generate_implied_end_tags(&mut self, except: Option<&str>) {
        loop {
            let n = self.current_name();
            if is_implied_end(n) && Some(n) != except {
                self.open.pop();
            } else {
                break;
            }
        }
    }

    fn close_p(&mut self) {
        self.generate_implied_end_tags(Some("p"));
        if self.current_name() != "p" {
            self.error("unexpected-end-tag");
        }
        self.pop_until("p");
    }

    // ----- insertion helpers --------------------------------------------------

    fn insert_element(&mut self, name: &str, attrs: Vec<Attribute>) -> NodeId {
        let id = self.doc.create_element(name, attrs);
        let parent = self.current();
        self.doc.append_child(parent, id);
        self.open.push(id);
        id
    }

    fn insert_void(&mut self, name: &str, attrs: Vec<Attribute>) {
        self.insert_element(name, attrs);
        self.open.pop();
    }

    fn insert_text(&mut self, text: &str) {
        let parent = self.current();
        self.doc.append_text(parent, text);
    }

    fn insert_comment(&mut self, text: &str) {
        let parent = self.current();
        self.append_comment_to(parent, text);
    }

    fn append_comment_to(&mut self, parent: NodeId, text: &str) {
        let id = self.doc.create_comment(text);
        self.doc.append_child(parent, id);
    }

    fn merge_attrs(&mut self, id: NodeId, attrs: Vec<Attribute>) {
        if let Some(el) = self.doc.element_mut(id) {
            for a in attrs {
                if !el.has_attr(&a.name) {
                    el.attrs.push(a);
                }
            }
        }
    }

    fn clone_element(&self, id: NodeId) -> Element {
        self.doc
            .element(id)
            .cloned()
            .unwrap_or_else(|| Element::new("", vec![]))
    }

    /// Insert an element whose content is raw text, and switch to text mode.
    fn generic_raw(&mut self, name: &str, attrs: Vec<Attribute>, mode: TextMode) {
        self.insert_element(name, attrs);
        self.tokenizer.set_text_mode(mode);
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    fn in_foreign_content(&self) -> bool {
        self.open
            .iter()
            .any(|&id| matches!(self.name_of(id), "svg" | "math"))
    }

    // ----- active formatting elements -----------------------------------------

    fn push_formatting(&mut self, id: NodeId) {
        // Noah's Ark clause: at most three identical entries since the
        // last marker.
        let el = self.clone_element(id);
        let mut earliest = None;
        let mut count = 0;
        for (i, entry) in self.formatting.iter().enumerate().rev() {
            match entry {
                FmtEntry::Marker => break,
                FmtEntry::Element(other) => {
                    let o = self.clone_element(*other);
                    if o.name == el.name && same_attrs(&o.attrs, &el.attrs) {
                        count += 1;
                        earliest = Some(i);
                    }
                }
            }
        }
        if count >= 3
            && let Some(i) = earliest
        {
            self.formatting.remove(i);
        }
        self.formatting.push(FmtEntry::Element(id));
    }

    fn formatting_index(&self, id: NodeId) -> Option<usize> {
        self.formatting
            .iter()
            .position(|e| *e == FmtEntry::Element(id))
    }

    /// Last entry after the last marker whose element has `name`.
    fn formatting_find(&self, name: &str) -> Option<(usize, NodeId)> {
        for (i, entry) in self.formatting.iter().enumerate().rev() {
            match entry {
                FmtEntry::Marker => return None,
                FmtEntry::Element(id) if self.name_of(*id) == name => return Some((i, *id)),
                FmtEntry::Element(_) => {}
            }
        }
        None
    }

    fn clear_formatting_to_marker(&mut self) {
        while let Some(e) = self.formatting.pop() {
            if e == FmtEntry::Marker {
                break;
            }
        }
    }

    fn reconstruct_formatting(&mut self) {
        let Some(last) = self.formatting.last().copied() else {
            return;
        };
        match last {
            FmtEntry::Marker => return,
            FmtEntry::Element(id) if self.open.contains(&id) => return,
            FmtEntry::Element(_) => {}
        }

        // Rewind to the earliest entry (after a marker or an open element)
        // that needs recreating.
        let mut i = self.formatting.len() - 1;
        loop {
            if i == 0 {
                break;
            }
            i -= 1;
            match self.formatting[i] {
                FmtEntry::Marker => {
                    i += 1;
                    break;
                }
                FmtEntry::Element(id) if self.open.contains(&id) => {
                    i += 1;
                    break;
                }
                FmtEntry::Element(_) => {}
            }
        }

        // Recreate from `i` to the end.
        for j in i..self.formatting.len() {
            let FmtEntry::Element(old) = self.formatting[j] else {
                continue;
            };
            let el = self.clone_element(old);
            let new = self.insert_element(&el.name, el.attrs);
            self.formatting[j] = FmtEntry::Element(new);
        }
    }

    /// The adoption agency algorithm, for end tags of formatting elements.
    fn adoption_agency(&mut self, subject: &str) {
        // Step 1: the easy case.
        if self.current_name() == subject && self.formatting_index(self.current()).is_none() {
            self.open.pop();
            return;
        }

        for _ in 0..8 {
            let Some((fmt_idx, fmt_el)) = self.formatting_find(subject) else {
                self.any_other_end_tag(subject);
                return;
            };
            let Some(stack_idx) = self.open.iter().position(|&id| id == fmt_el) else {
                self.error("unexpected-end-tag");
                self.formatting.remove(fmt_idx);
                return;
            };
            if !self.element_in_scope(fmt_el, Scope::Default) {
                self.error("unexpected-end-tag");
                return;
            }
            if fmt_el != self.current() {
                self.error("unexpected-end-tag");
            }

            // Furthest block: the first special element above the
            // formatting element in the stack.
            let furthest = self.open[stack_idx + 1..]
                .iter()
                .position(|&id| is_special(self.name_of(id)))
                .map(|p| p + stack_idx + 1);
            let Some(fb_idx) = furthest else {
                self.open.truncate(stack_idx);
                self.formatting.remove(fmt_idx);
                return;
            };
            let furthest_block = self.open[fb_idx];
            let common_ancestor = self.open[stack_idx - 1];
            let mut bookmark = fmt_idx;

            let mut node_idx = fb_idx;
            let mut node;
            let mut last_node = furthest_block;
            let mut inner = 0;
            loop {
                inner += 1;
                node_idx -= 1;
                node = self.open[node_idx];
                if node == fmt_el {
                    break;
                }
                let mut node_fmt = self.formatting_index(node);
                if inner > 3
                    && let Some(p) = node_fmt
                {
                    self.formatting.remove(p);
                    if p < bookmark {
                        bookmark -= 1;
                    }
                    node_fmt = None;
                }
                let Some(node_fmt) = node_fmt else {
                    self.open.remove(node_idx);
                    continue;
                };

                let el = self.clone_element(node);
                let new = self.doc.create_element(&el.name, el.attrs);
                self.formatting[node_fmt] = FmtEntry::Element(new);
                self.open[node_idx] = new;
                node = new;

                if last_node == furthest_block {
                    bookmark = node_fmt + 1;
                }
                self.doc.append_child(node, last_node);
                last_node = node;
            }

            self.doc.append_child(common_ancestor, last_node);

            let el = self.clone_element(fmt_el);
            let new_fmt = self.doc.create_element(&el.name, el.attrs);
            let kids: Vec<NodeId> = self.doc.children(furthest_block).collect();
            for k in kids {
                self.doc.append_child(new_fmt, k);
            }
            self.doc.append_child(furthest_block, new_fmt);

            if let Some(p) = self.formatting_index(fmt_el) {
                self.formatting.remove(p);
                if p < bookmark {
                    bookmark -= 1;
                }
            }
            let bookmark = bookmark.min(self.formatting.len());
            self.formatting.insert(bookmark, FmtEntry::Element(new_fmt));

            if let Some(p) = self.open.iter().position(|&id| id == fmt_el) {
                self.open.remove(p);
            }
            if let Some(p) = self.open.iter().position(|&id| id == furthest_block) {
                self.open.insert(p + 1, new_fmt);
            }
        }
    }

    // ----- token dispatch -----------------------------------------------------

    fn process(&mut self, mut tok: Token) {
        if self.ignore_lf {
            self.ignore_lf = false;
            if let Token::Text(t) = &mut tok
                && t.starts_with('\n')
            {
                t.remove(0);
                if t.is_empty() {
                    return;
                }
            }
        }
        match self.mode {
            Mode::Initial => self.initial(tok),
            Mode::BeforeHtml => self.before_html(tok),
            Mode::BeforeHead => self.before_head(tok),
            Mode::InHead => self.in_head(tok),
            Mode::AfterHead => self.after_head(tok),
            Mode::InBody => self.in_body(tok),
            Mode::Text => self.text(tok),
            Mode::AfterBody => self.after_body(tok),
            Mode::AfterAfterBody => self.after_after_body(tok),
        }
    }

    fn initial(&mut self, tok: Token) {
        match tok {
            Token::Text(t) => {
                let (_, rest) = split_leading_ws(&t);
                if !rest.is_empty() {
                    let rest = rest.to_owned();
                    self.error("missing-doctype");
                    self.mode = Mode::BeforeHtml;
                    self.process(Token::Text(rest));
                }
            }
            Token::Comment(c) => {
                let root = self.doc.root();
                self.append_comment_to(root, &c);
            }
            Token::Doctype { name, force_quirks } => {
                if force_quirks || name != "html" {
                    self.error("unexpected-doctype");
                }
                let id = self.doc.create_doctype(&name);
                let root = self.doc.root();
                self.doc.append_child(root, id);
                self.mode = Mode::BeforeHtml;
            }
            other => {
                self.error("missing-doctype");
                self.mode = Mode::BeforeHtml;
                self.process(other);
            }
        }
    }

    fn before_html(&mut self, tok: Token) {
        match tok {
            Token::Doctype { .. } => self.error("unexpected-doctype"),
            Token::Comment(c) => {
                let root = self.doc.root();
                self.append_comment_to(root, &c);
            }
            Token::Text(t) => {
                let (_, rest) = split_leading_ws(&t);
                if !rest.is_empty() {
                    let rest = rest.to_owned();
                    self.before_html_anything_else(Token::Text(rest));
                }
            }
            Token::StartTag { name, attrs, .. } if name == "html" => {
                self.insert_element("html", attrs);
                self.mode = Mode::BeforeHead;
            }
            Token::EndTag { ref name }
                if !matches!(name.as_str(), "head" | "body" | "html" | "br") =>
            {
                self.error("unexpected-end-tag");
            }
            other => self.before_html_anything_else(other),
        }
    }

    fn before_html_anything_else(&mut self, tok: Token) {
        self.insert_element("html", vec![]);
        self.mode = Mode::BeforeHead;
        self.process(tok);
    }

    fn before_head(&mut self, tok: Token) {
        match tok {
            Token::Text(t) => {
                let (_, rest) = split_leading_ws(&t);
                if !rest.is_empty() {
                    let rest = rest.to_owned();
                    self.before_head_anything_else(Token::Text(rest));
                }
            }
            Token::Comment(c) => self.insert_comment(&c),
            Token::Doctype { .. } => self.error("unexpected-doctype"),
            Token::StartTag { ref name, .. } if name == "html" => self.in_body(tok),
            Token::StartTag { name, attrs, .. } if name == "head" => {
                let id = self.insert_element("head", attrs);
                self.head = Some(id);
                self.mode = Mode::InHead;
            }
            Token::EndTag { ref name }
                if !matches!(name.as_str(), "head" | "body" | "html" | "br") =>
            {
                self.error("unexpected-end-tag");
            }
            other => self.before_head_anything_else(other),
        }
    }

    fn before_head_anything_else(&mut self, tok: Token) {
        let id = self.insert_element("head", vec![]);
        self.head = Some(id);
        self.mode = Mode::InHead;
        self.process(tok);
    }

    fn in_head(&mut self, tok: Token) {
        match tok {
            Token::Text(t) => {
                let (ws, rest) = split_leading_ws(&t);
                if !ws.is_empty() {
                    self.insert_text(ws);
                }
                if !rest.is_empty() {
                    let rest = rest.to_owned();
                    self.in_head_anything_else(Token::Text(rest));
                }
            }
            Token::Comment(c) => self.insert_comment(&c),
            Token::Doctype { .. } => self.error("unexpected-doctype"),
            Token::StartTag { ref name, .. } if name == "html" => self.in_body(tok),
            Token::StartTag { name, attrs, .. }
                if matches!(
                    name.as_str(),
                    "base" | "basefont" | "bgsound" | "link" | "meta"
                ) =>
            {
                self.insert_void(&name, attrs);
            }
            Token::StartTag { name, attrs, .. } if name == "title" => {
                self.generic_raw(&name, attrs, TextMode::RcData);
            }
            Token::StartTag { name, attrs, .. }
                if matches!(name.as_str(), "noframes" | "style" | "script") =>
            {
                self.generic_raw(&name, attrs, TextMode::RawText);
            }
            Token::StartTag { ref name, .. } if name == "head" => {
                self.error("unexpected-start-tag");
            }
            Token::EndTag { ref name } if name == "head" => {
                self.open.pop();
                self.mode = Mode::AfterHead;
            }
            Token::EndTag { ref name } if !matches!(name.as_str(), "body" | "html" | "br") => {
                self.error("unexpected-end-tag");
            }
            other => self.in_head_anything_else(other),
        }
    }

    fn in_head_anything_else(&mut self, tok: Token) {
        self.open.pop();
        self.mode = Mode::AfterHead;
        self.process(tok);
    }

    fn after_head(&mut self, tok: Token) {
        match tok {
            Token::Text(t) => {
                let (ws, rest) = split_leading_ws(&t);
                if !ws.is_empty() {
                    self.insert_text(ws);
                }
                if !rest.is_empty() {
                    let rest = rest.to_owned();
                    self.after_head_anything_else(Token::Text(rest));
                }
            }
            Token::Comment(c) => self.insert_comment(&c),
            Token::Doctype { .. } => self.error("unexpected-doctype"),
            Token::StartTag { ref name, .. } if name == "html" => self.in_body(tok),
            Token::StartTag { name, attrs, .. } if name == "body" => {
                self.insert_element("body", attrs);
                self.mode = Mode::InBody;
            }
            Token::StartTag { ref name, .. } if name == "frameset" => {
                // Framesets are not supported; drop the tag, keep content.
                self.error("unexpected-start-tag");
            }
            Token::StartTag { ref name, .. }
                if matches!(
                    name.as_str(),
                    "base"
                        | "basefont"
                        | "bgsound"
                        | "link"
                        | "meta"
                        | "noframes"
                        | "script"
                        | "style"
                        | "title"
                ) =>
            {
                self.error("unexpected-start-tag");
                if let Some(head) = self.head {
                    self.open.push(head);
                    self.in_head(tok);
                    self.open.retain(|&id| id != head);
                }
            }
            Token::StartTag { ref name, .. } if name == "head" => {
                self.error("unexpected-start-tag");
            }
            Token::EndTag { ref name } if !matches!(name.as_str(), "body" | "html" | "br") => {
                self.error("unexpected-end-tag");
            }
            other => self.after_head_anything_else(other),
        }
    }

    fn after_head_anything_else(&mut self, tok: Token) {
        self.insert_element("body", vec![]);
        self.mode = Mode::InBody;
        self.process(tok);
    }

    fn text(&mut self, tok: Token) {
        match tok {
            Token::Text(t) => self.insert_text(&t),
            Token::Eof => {
                self.error("eof-in-element-that-can-contain-only-text");
                self.open.pop();
                self.mode = self.original_mode;
                self.process(Token::Eof);
            }
            Token::EndTag { .. } => {
                self.open.pop();
                self.mode = self.original_mode;
            }
            // The tokenizer only produces text and the matching end tag in
            // these modes.
            _ => {}
        }
    }

    fn after_body(&mut self, tok: Token) {
        match tok {
            Token::Text(t) => {
                let (ws, rest) = split_leading_ws(&t);
                if !ws.is_empty() {
                    self.in_body(Token::Text(ws.to_owned()));
                }
                if !rest.is_empty() {
                    let rest = rest.to_owned();
                    self.error("unexpected-token-after-body");
                    self.mode = Mode::InBody;
                    self.process(Token::Text(rest));
                }
            }
            Token::Comment(c) => {
                let html = self.open[0];
                self.append_comment_to(html, &c);
            }
            Token::Doctype { .. } => self.error("unexpected-doctype"),
            Token::StartTag { ref name, .. } if name == "html" => self.in_body(tok),
            Token::EndTag { ref name } if name == "html" => self.mode = Mode::AfterAfterBody,
            Token::Eof => {}
            other => {
                self.error("unexpected-token-after-body");
                self.mode = Mode::InBody;
                self.process(other);
            }
        }
    }

    fn after_after_body(&mut self, tok: Token) {
        match tok {
            Token::Comment(c) => {
                let root = self.doc.root();
                self.append_comment_to(root, &c);
            }
            Token::Text(t) => {
                let (ws, rest) = split_leading_ws(&t);
                if !ws.is_empty() {
                    self.in_body(Token::Text(ws.to_owned()));
                }
                if !rest.is_empty() {
                    let rest = rest.to_owned();
                    self.error("unexpected-token-after-body");
                    self.mode = Mode::InBody;
                    self.process(Token::Text(rest));
                }
            }
            Token::Doctype { .. } => self.in_body(tok),
            Token::StartTag { ref name, .. } if name == "html" => self.in_body(tok),
            Token::Eof => {}
            other => {
                self.error("unexpected-token-after-body");
                self.mode = Mode::InBody;
                self.process(other);
            }
        }
    }

    // ----- in body ------------------------------------------------------------

    fn in_body(&mut self, tok: Token) {
        match tok {
            Token::Text(t) => {
                let t = if t.contains('\0') {
                    self.error("unexpected-null-character");
                    t.replace('\0', "")
                } else {
                    t
                };
                if !t.is_empty() {
                    self.reconstruct_formatting();
                    self.insert_text(&t);
                }
            }
            Token::Comment(c) => self.insert_comment(&c),
            Token::Doctype { .. } => self.error("unexpected-doctype"),
            Token::Eof => {
                let ok = self.open.iter().all(|&id| {
                    matches!(
                        self.name_of(id),
                        "dd" | "dt"
                            | "li"
                            | "optgroup"
                            | "option"
                            | "p"
                            | "rb"
                            | "rp"
                            | "rt"
                            | "rtc"
                            | "tbody"
                            | "td"
                            | "tfoot"
                            | "th"
                            | "thead"
                            | "tr"
                            | "body"
                            | "html"
                    )
                });
                if !ok {
                    self.error("expected-closing-tag-but-got-eof");
                }
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => self.in_body_start(name, attrs, self_closing),
            Token::EndTag { name } => self.in_body_end(&name),
        }
    }

    fn in_body_start(&mut self, name: String, attrs: Vec<Attribute>, self_closing: bool) {
        match name.as_str() {
            "html" => {
                self.error("unexpected-start-tag");
                let html = self.open[0];
                self.merge_attrs(html, attrs);
            }
            // Only tags that `in_head` handles directly may be delegated;
            // anything else would bounce between modes forever.
            "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style"
            | "title" => {
                self.in_head(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                });
            }
            "body" => {
                self.error("unexpected-start-tag");
                if self.open.len() > 1 && self.name_of(self.open[1]) == "body" {
                    let body = self.open[1];
                    self.merge_attrs(body, attrs);
                }
            }
            "frameset" | "frame" | "head" => self.error("unexpected-start-tag"),

            n if is_block_start(n) => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_element(&name, attrs);
            }
            n if is_heading(n) => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                if is_heading(self.current_name()) {
                    self.error("unexpected-start-tag");
                    self.open.pop();
                }
                self.insert_element(&name, attrs);
            }
            "pre" | "listing" => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_element(&name, attrs);
                self.ignore_lf = true;
            }
            "form" => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_element(&name, attrs);
            }
            "li" => {
                self.close_list_item(&["li"]);
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_element(&name, attrs);
            }
            "dd" | "dt" => {
                self.close_list_item(&["dd", "dt"]);
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_element(&name, attrs);
            }
            "plaintext" => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_element(&name, attrs);
                self.tokenizer.set_text_mode(TextMode::PlainText);
            }
            "button" => {
                if self.in_scope("button", Scope::Default) {
                    self.error("unexpected-start-tag");
                    self.generate_implied_end_tags(None);
                    self.pop_until("button");
                }
                self.reconstruct_formatting();
                self.insert_element(&name, attrs);
            }

            "a" => {
                if let Some((_, old)) = self.formatting_find("a") {
                    self.error("unexpected-start-tag");
                    self.adoption_agency("a");
                    if let Some(p) = self.formatting_index(old) {
                        self.formatting.remove(p);
                    }
                    self.open.retain(|&id| id != old);
                }
                self.reconstruct_formatting();
                let id = self.insert_element(&name, attrs);
                self.push_formatting(id);
            }
            "nobr" => {
                self.reconstruct_formatting();
                if self.in_scope("nobr", Scope::Default) {
                    self.error("unexpected-start-tag");
                    self.adoption_agency("nobr");
                    self.reconstruct_formatting();
                }
                let id = self.insert_element(&name, attrs);
                self.push_formatting(id);
            }
            n if is_formatting(n) => {
                self.reconstruct_formatting();
                let id = self.insert_element(&name, attrs);
                self.push_formatting(id);
            }
            "applet" | "marquee" | "object" => {
                self.reconstruct_formatting();
                self.insert_element(&name, attrs);
                self.formatting.push(FmtEntry::Marker);
            }

            // Tables (simplified: see crate docs).
            "table" => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_element(&name, attrs);
            }
            "caption" | "colgroup" | "tbody" | "thead" | "tfoot" => {
                if !self.in_scope("table", Scope::Table) {
                    self.error("unexpected-start-tag");
                    return;
                }
                self.close_cell();
                self.pop_while_not(|n| n == "table");
                self.insert_element(&name, attrs);
                if name == "caption" {
                    self.formatting.push(FmtEntry::Marker);
                }
            }
            "col" => {
                if !self.in_scope("table", Scope::Table) {
                    self.error("unexpected-start-tag");
                    return;
                }
                self.pop_while_not(|n| matches!(n, "colgroup" | "table"));
                if self.current_name() == "table" {
                    self.insert_element("colgroup", vec![]);
                }
                self.insert_void(&name, attrs);
            }
            "tr" => {
                if !self.in_scope("table", Scope::Table) {
                    self.error("unexpected-start-tag");
                    return;
                }
                self.close_cell();
                self.pop_while_not(|n| matches!(n, "tbody" | "thead" | "tfoot" | "table"));
                if self.current_name() == "table" {
                    self.insert_element("tbody", vec![]);
                }
                self.insert_element(&name, attrs);
            }
            "td" | "th" => {
                if !self.in_scope("table", Scope::Table) {
                    self.error("unexpected-start-tag");
                    return;
                }
                self.close_cell();
                self.pop_while_not(|n| matches!(n, "tr" | "tbody" | "thead" | "tfoot" | "table"));
                if self.current_name() == "table" {
                    self.insert_element("tbody", vec![]);
                }
                if matches!(self.current_name(), "tbody" | "thead" | "tfoot") {
                    self.insert_element("tr", vec![]);
                }
                self.insert_element(&name, attrs);
                self.formatting.push(FmtEntry::Marker);
            }

            "area" | "br" | "embed" | "img" | "keygen" | "wbr" | "input" => {
                self.reconstruct_formatting();
                self.insert_void(&name, attrs);
            }
            "param" | "source" | "track" => self.insert_void(&name, attrs),
            "hr" => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert_void(&name, attrs);
            }
            "image" => {
                self.error("unexpected-start-tag");
                self.in_body_start("img".to_owned(), attrs, self_closing);
            }
            "textarea" => {
                self.insert_element(&name, attrs);
                self.ignore_lf = true;
                self.tokenizer.set_text_mode(TextMode::RcData);
                self.original_mode = self.mode;
                self.mode = Mode::Text;
            }
            "xmp" => {
                if self.in_scope("p", Scope::Button) {
                    self.close_p();
                }
                self.reconstruct_formatting();
                self.generic_raw(&name, attrs, TextMode::RawText);
            }
            "iframe" | "noembed" => self.generic_raw(&name, attrs, TextMode::RawText),

            "select" => {
                self.reconstruct_formatting();
                self.insert_element(&name, attrs);
            }
            "option" | "optgroup" => {
                if self.current_name() == "option" {
                    self.open.pop();
                }
                if name == "optgroup" && self.current_name() == "optgroup" {
                    self.open.pop();
                }
                self.reconstruct_formatting();
                self.insert_element(&name, attrs);
            }

            "math" | "svg" => {
                self.reconstruct_formatting();
                self.insert_element(&name, attrs);
                if self_closing {
                    self.open.pop();
                }
            }
            _ => {
                self.reconstruct_formatting();
                self.insert_element(&name, attrs);
                if self_closing && self.in_foreign_content() {
                    self.open.pop();
                }
            }
        }
    }

    fn close_list_item(&mut self, kinds: &[&str]) {
        for i in (0..self.open.len()).rev() {
            let n = self.name_of(self.open[i]).to_owned();
            if kinds.contains(&n.as_str()) {
                self.generate_implied_end_tags(Some(&n));
                if self.current_name() != n {
                    self.error("unexpected-start-tag");
                }
                self.pop_until(&n);
                return;
            }
            if is_special(&n) && !matches!(n.as_str(), "address" | "div" | "p") {
                return;
            }
        }
    }

    /// If a table cell is open, close it (used by the simplified table
    /// handling in place of the "in cell" insertion mode).
    fn close_cell(&mut self) {
        if self.in_scope_by(|n| matches!(n, "td" | "th"), Scope::Table) {
            self.generate_implied_end_tags(None);
            self.pop_until_by(|n| matches!(n, "td" | "th"));
            self.clear_formatting_to_marker();
        }
    }

    fn in_body_end(&mut self, name: &str) {
        match name {
            "body" => {
                if !self.in_scope("body", Scope::Default) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.mode = Mode::AfterBody;
            }
            "html" => {
                if !self.in_scope("body", Scope::Default) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.mode = Mode::AfterBody;
                self.process(Token::EndTag {
                    name: name.to_owned(),
                });
            }
            n if is_block_start(n) && n != "p" => self.close_block(name),
            "button" | "listing" | "pre" | "form" => self.close_block(name),
            "p" => {
                if !self.in_scope("p", Scope::Button) {
                    self.error("unexpected-end-tag");
                    self.insert_element("p", vec![]);
                }
                self.close_p();
            }
            "li" => {
                if !self.in_scope("li", Scope::ListItem) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.generate_implied_end_tags(Some("li"));
                if self.current_name() != "li" {
                    self.error("unexpected-end-tag");
                }
                self.pop_until("li");
            }
            "dd" | "dt" => {
                if !self.in_scope(name, Scope::Default) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.generate_implied_end_tags(Some(name));
                if self.current_name() != name {
                    self.error("unexpected-end-tag");
                }
                self.pop_until(name);
            }
            n if is_heading(n) => {
                if !self.in_scope_by(is_heading, Scope::Default) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.generate_implied_end_tags(None);
                if self.current_name() != name {
                    self.error("unexpected-end-tag");
                }
                self.pop_until_by(is_heading);
            }
            n if is_formatting(n) => self.adoption_agency(name),
            "applet" | "marquee" | "object" => {
                if !self.in_scope(name, Scope::Default) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.generate_implied_end_tags(None);
                if self.current_name() != name {
                    self.error("unexpected-end-tag");
                }
                self.pop_until(name);
                self.clear_formatting_to_marker();
            }
            "br" => {
                self.error("unexpected-end-tag");
                self.in_body_start("br".to_owned(), vec![], false);
            }

            // Tables (simplified).
            "table" => {
                if !self.in_scope("table", Scope::Table) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.close_cell();
                self.pop_until("table");
            }
            "tbody" | "thead" | "tfoot" | "tr" => {
                if !self.in_scope(name, Scope::Table) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.close_cell();
                self.pop_until(name);
            }
            "td" | "th" | "caption" => {
                if !self.in_scope(name, Scope::Table) {
                    self.error("unexpected-end-tag");
                    return;
                }
                self.generate_implied_end_tags(None);
                self.pop_until(name);
                self.clear_formatting_to_marker();
            }
            "colgroup" => {
                if self.current_name() == "colgroup" {
                    self.open.pop();
                } else {
                    self.error("unexpected-end-tag");
                }
            }
            "col" => self.error("unexpected-end-tag"),

            // `template` is an ordinary element here (no template mode), so
            // it falls through to the generic handling.
            _ => self.any_other_end_tag(name),
        }
    }

    fn close_block(&mut self, name: &str) {
        if !self.in_scope(name, Scope::Default) {
            self.error("unexpected-end-tag");
            return;
        }
        self.generate_implied_end_tags(None);
        if self.current_name() != name {
            self.error("unexpected-end-tag");
        }
        self.pop_until(name);
    }

    fn any_other_end_tag(&mut self, name: &str) {
        for i in (0..self.open.len()).rev() {
            let node = self.open[i];
            if self.name_of(node) == name {
                self.generate_implied_end_tags(Some(name));
                if self.current() != node {
                    self.error("unexpected-end-tag");
                }
                // The node may have been popped by implied end tags; find
                // it again rather than trusting `i`.
                if let Some(p) = self.open.iter().position(|&id| id == node) {
                    self.open.truncate(p);
                }
                return;
            }
            if is_special(self.name_of(node)) {
                self.error("unexpected-end-tag");
                return;
            }
        }
    }
}
