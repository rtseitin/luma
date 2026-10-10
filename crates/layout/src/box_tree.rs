//! Box tree generation: from the DOM and its computed styles to a tree of
//! boxes with no geometry yet ([CSS 2.1 §9.2][boxgen]).
//!
//! [boxgen]: https://www.w3.org/TR/CSS21/visuren.html#box-gen
//!
//! What happens here:
//!
//! - `display: none` subtrees, comments, and the doctype produce nothing.
//! - Elements become block, inline, or inline-block boxes by `display`.
//!   `list-item` and `flex` are block boxes (list items also remember
//!   their marker text).
//! - Text nodes become text boxes with the whitespace processing of
//!   [CSS Text §4.1][ws] that can be done without knowing line positions:
//!   collapsible whitespace runs become one space and, for `pre-line`,
//!   a newline. Trimming at line edges happens in inline layout.
//! - A block container with both block-level and inline-level children
//!   gets anonymous block boxes around each run of inline-level children
//!   ([§9.2.1.1][anon]). Whitespace-only runs between blocks are dropped.
//! - An inline box containing a block-level box is itself turned into a
//!   block box (a simplification of §9.2.1.1, which would split it).
//!
//! The DOM is walked iteratively and the tree is flattened beyond
//! [`MAX_DEPTH`], so arbitrarily deep documents neither overflow the stack
//! here nor in the recursive layout passes that follow.
//!
//! [ws]: https://www.w3.org/TR/css-text-3/#white-space-phase-1
//! [anon]: https://www.w3.org/TR/CSS21/visuren.html#anonymous-block-level

use std::sync::Arc;

use luma_dom::{Document, NodeData, NodeId};
use luma_style::{ComputedStyle, Display, StyleMap, WhiteSpace};

/// Boxes nested deeper than this are attached to their nearest ancestor
/// at this depth instead, keeping later recursive passes bounded.
pub(crate) const MAX_DEPTH: usize = 256;

/// Index of a node in a [`BoxTree`].
pub(crate) type BoxIdx = usize;

/// What a box tree node is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// A block-level block container.
    Block,
    /// A block container generated to hold inline content next to blocks.
    AnonymousBlock,
    /// An inline box.
    Inline,
    /// An inline-level block container.
    InlineBlock,
    /// A run of text, already whitespace-collapsed per its `white-space`.
    Text(String),
    /// A forced line break (`<br>`).
    Break,
}

impl Kind {
    pub(crate) fn is_block_level(&self) -> bool {
        matches!(self, Kind::Block | Kind::AnonymousBlock)
    }
}

/// A node in the box tree.
#[derive(Debug, Clone)]
pub(crate) struct BoxNode {
    pub(crate) kind: Kind,
    /// The DOM node, or `None` for anonymous boxes.
    pub(crate) node: Option<NodeId>,
    pub(crate) style: Arc<ComputedStyle>,
    pub(crate) children: Vec<BoxIdx>,
    /// Marker text for `display: list-item` boxes (`"• "`, `"3. "`).
    pub(crate) marker: Option<String>,
}

/// A tree of boxes without geometry.
#[derive(Debug, Clone)]
pub(crate) struct BoxTree {
    pub(crate) nodes: Vec<BoxNode>,
    /// The root block (the `<html>` element's box).
    pub(crate) root: BoxIdx,
}

impl BoxTree {
    /// Build the box tree for `doc`.
    ///
    /// The root is always a block box, for the `<html>` element when the
    /// document has one and an empty anonymous block otherwise.
    pub(crate) fn build(doc: &Document, styles: &StyleMap) -> Self {
        let mut tree = BoxTree {
            nodes: Vec::new(),
            root: 0,
        };
        let html = doc
            .children(doc.root())
            .find(|&id| doc.get(id).is_element());
        match html.and_then(|id| styles.get_arc(id).map(|s| (id, s))) {
            Some((id, style)) => {
                let root_style = if style.display == Display::Block {
                    style
                } else {
                    // The root is always blockified (CSS Display §2.7).
                    Arc::new(ComputedStyle {
                        display: Display::Block,
                        ..(*style).clone()
                    })
                };
                tree.root = tree.push(Kind::Block, Some(id), root_style);
                tree.build_subtree(doc, styles, id);
            }
            None => {
                let style = Arc::new(ComputedStyle {
                    display: Display::Block,
                    ..ComputedStyle::initial()
                });
                tree.root = tree.push(Kind::Block, None, style);
            }
        }
        tree.fix_up();
        tree
    }

    pub(crate) fn get(&self, idx: BoxIdx) -> &BoxNode {
        &self.nodes[idx]
    }

    fn push(&mut self, kind: Kind, node: Option<NodeId>, style: Arc<ComputedStyle>) -> BoxIdx {
        self.nodes.push(BoxNode {
            kind,
            node,
            style,
            children: Vec::new(),
            marker: None,
        });
        self.nodes.len() - 1
    }

    /// Create boxes for everything beneath `root` (whose box is
    /// `self.root`), iteratively, in document order.
    fn build_subtree(&mut self, doc: &Document, styles: &StyleMap, root: NodeId) {
        // (DOM node, box that receives its box, depth)
        let mut stack: Vec<(NodeId, BoxIdx, usize)> = Vec::new();
        let root_box = self.root;
        let kids: Vec<NodeId> = doc.children(root).collect();
        stack.extend(kids.into_iter().rev().map(|c| (c, root_box, 1)));

        while let Some((id, parent, depth)) = stack.pop() {
            let node = doc.get(id);
            let Some(style) = styles.get_arc(id) else {
                continue; // doctype, comment
            };
            let kind = match &node.data {
                NodeData::Text(text) => {
                    let collapsed = collapse_whitespace(text, style.white_space);
                    if collapsed.is_empty() {
                        continue;
                    }
                    Kind::Text(collapsed)
                }
                NodeData::Element(el) => match style.display {
                    Display::None => continue,
                    Display::Block | Display::ListItem | Display::Flex => Kind::Block,
                    Display::Inline if el.name == "br" => Kind::Break,
                    Display::Inline => Kind::Inline,
                    Display::InlineBlock => Kind::InlineBlock,
                },
                _ => continue,
            };
            let is_text = matches!(kind, Kind::Text(_) | Kind::Break);
            let is_list_item = style.display == Display::ListItem;
            let idx = self.push(kind, Some(id), style);
            self.nodes[parent].children.push(idx);
            if is_list_item {
                self.nodes[idx].marker = Some(marker_text(doc, id));
            }
            if is_text {
                continue;
            }
            // Children of boxes at the depth limit are flattened into the
            // same parent, as browsers' parsers do for very deep trees.
            let (child_parent, child_depth) = if depth < MAX_DEPTH {
                (idx, depth + 1)
            } else {
                (parent, depth)
            };
            let kids: Vec<NodeId> = doc.children(id).collect();
            stack.extend(
                kids.into_iter()
                    .rev()
                    .map(|c| (c, child_parent, child_depth)),
            );
        }
    }

    /// Post-order fix-ups: inline boxes with block children become blocks,
    /// and block containers with mixed children get anonymous blocks.
    ///
    /// Children always have larger indices than their parents, so walking
    /// the arena backwards visits every box after all of its children.
    fn fix_up(&mut self) {
        for idx in (0..self.nodes.len()).rev() {
            if self.nodes[idx].children.is_empty() {
                continue;
            }
            let has_block_child = self.nodes[idx]
                .children
                .iter()
                .any(|&c| self.nodes[c].kind.is_block_level());
            if !has_block_child {
                continue;
            }
            if self.nodes[idx].kind == Kind::Inline {
                self.nodes[idx].kind = Kind::Block;
            }
            self.wrap_inline_runs(idx);
        }
    }

    /// CSS 2.1 §9.2.1.1: wrap each maximal run of inline-level children of
    /// `idx` in an anonymous block box. Runs that contain only collapsible
    /// whitespace are dropped.
    fn wrap_inline_runs(&mut self, idx: BoxIdx) {
        let children = std::mem::take(&mut self.nodes[idx].children);
        let anon_style = Arc::new(anonymous_style(&self.nodes[idx].style));
        let mut out = Vec::with_capacity(children.len());
        let mut run: Vec<BoxIdx> = Vec::new();

        let flush = |tree: &mut BoxTree, run: &mut Vec<BoxIdx>, out: &mut Vec<BoxIdx>| {
            if run.is_empty() {
                return;
            }
            let only_space = run.iter().all(|&c| match &tree.nodes[c].kind {
                Kind::Text(t) => t.trim_matches(' ').is_empty(),
                _ => false,
            });
            if !only_space {
                let anon = tree.push(Kind::AnonymousBlock, None, anon_style.clone());
                tree.nodes[anon].children = std::mem::take(run);
                out.push(anon);
            } else {
                run.clear();
            }
        };

        for child in children {
            if self.nodes[child].kind.is_block_level() {
                flush(self, &mut run, &mut out);
                out.push(child);
            } else {
                run.push(child);
            }
        }
        flush(self, &mut run, &mut out);
        self.nodes[idx].children = out;
    }

    /// Whether every child of `idx` is inline-level (so the box lays out
    /// its content as lines). A box with no children counts as `true`.
    pub(crate) fn has_inline_content(&self, idx: BoxIdx) -> bool {
        self.nodes[idx]
            .children
            .iter()
            .all(|&c| !self.nodes[c].kind.is_block_level())
    }
}

/// The style of an anonymous box: inherited properties from the parent,
/// everything else initial, and `display: block`.
pub(crate) fn anonymous_style(parent: &ComputedStyle) -> ComputedStyle {
    ComputedStyle {
        display: Display::Block,
        ..ComputedStyle::inherit_from(parent)
    }
}

/// Marker text for a list item: `"N. "` inside an `<ol>`, `"• "` otherwise.
fn marker_text(doc: &Document, li: NodeId) -> String {
    let parent = doc.get(li).parent();
    let in_ol = parent.is_some_and(|p| doc.element(p).is_some_and(|e| e.name == "ol"));
    if !in_ol {
        return "\u{2022} ".to_owned();
    }
    let parent = parent.expect("checked above");
    let start: i64 = doc
        .element(parent)
        .and_then(|e| e.attr("start"))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);
    let mut n = start;
    for sibling in doc.children(parent) {
        if sibling == li {
            break;
        }
        if doc.element(sibling).is_some_and(|e| e.name == "li") {
            n = n.saturating_add(1);
        }
    }
    format!("{n}. ")
}

/// Whether `c` is document whitespace for the purposes of collapsing.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

/// Within-node whitespace processing (CSS Text §4.1.1).
///
/// - `normal`/`nowrap`: every run of whitespace becomes one space.
/// - `pre-line`: runs containing a newline become one newline, others one
///   space.
/// - `pre`/`pre-wrap`: preserved, except that tabs are expanded to the
///   next multiple of 8 columns (the default tab size) and `\r` is dropped.
pub(crate) fn collapse_whitespace(text: &str, ws: WhiteSpace) -> String {
    match ws {
        WhiteSpace::Normal | WhiteSpace::Nowrap | WhiteSpace::PreLine => {
            let mut out = String::with_capacity(text.len());
            let mut run: Option<bool> = None; // Some(has_newline) while in a run
            for c in text.chars() {
                if is_space(c) {
                    let nl = run.unwrap_or(false) || c == '\n';
                    run = Some(nl);
                } else {
                    if let Some(nl) = run.take() {
                        out.push(if nl && ws == WhiteSpace::PreLine {
                            '\n'
                        } else {
                            ' '
                        });
                    }
                    out.push(c);
                }
            }
            if let Some(nl) = run {
                out.push(if nl && ws == WhiteSpace::PreLine {
                    '\n'
                } else {
                    ' '
                });
            }
            out
        }
        WhiteSpace::Pre | WhiteSpace::PreWrap => {
            let mut out = String::with_capacity(text.len());
            let mut column = 0usize;
            for c in text.chars() {
                match c {
                    '\t' => {
                        let n = 8 - column % 8;
                        out.extend(std::iter::repeat_n(' ', n));
                        column += n;
                    }
                    '\n' => {
                        out.push('\n');
                        column = 0;
                    }
                    '\r' => {}
                    c => {
                        out.push(c);
                        column += 1;
                    }
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use luma_style::{Viewport, style_document};

    /// Build the tree for `html`, then rewrite the dump so element boxes
    /// show their tag name (the dump itself has no DOM access).
    fn dump(html: &str) -> String {
        let doc = luma_html::parse(html);
        let styles = style_document(&doc, &[], Viewport::default());
        let tree = BoxTree::build(&doc, &styles);
        let mut out = String::new();
        let mut stack = vec![(tree.root, 0usize)];
        while let Some((idx, depth)) = stack.pop() {
            let node = &tree.nodes[idx];
            out.push_str(&"  ".repeat(depth));
            match &node.kind {
                Kind::Text(t) => {
                    let _ = write!(out, "Text {t:?}");
                }
                Kind::Break => out.push_str("Break"),
                kind => {
                    let _ = write!(out, "{kind:?}");
                    if let Some(el) = node.node.and_then(|id| doc.element(id)) {
                        let _ = write!(out, " <{}>", el.name);
                    }
                    if let Some(m) = &node.marker {
                        let _ = write!(out, " marker={m:?}");
                    }
                }
            }
            out.push('\n');
            stack.extend(node.children.iter().rev().map(|&c| (c, depth + 1)));
        }
        out
    }

    #[test]
    fn simple_blocks_and_inlines() {
        let out = dump("<p>Hello <b>bold</b> world</p>");
        assert_eq!(
            out,
            "\
Block <html>
  Block <body>
    Block <p>
      Text \"Hello \"
      Inline <b>
        Text \"bold\"
      Text \" world\"
"
        );
    }

    #[test]
    fn hidden_and_non_rendered_nodes_are_skipped() {
        let out = dump(
            "<!DOCTYPE html><head><title>t</title><style>p{}</style></head>
             <body><!-- c --><p hidden>x</p><p style='display:none'>y</p><p>z</p></body>",
        );
        assert_eq!(
            out,
            "\
Block <html>
  Block <body>
    Block <p>
      Text \"z\"
"
        );
    }

    #[test]
    fn anonymous_blocks_wrap_inline_runs() {
        let out = dump("<div>text <i>a</i><p>block</p> <span>tail</span><p>b</p>  </div>");
        assert_eq!(
            out,
            "\
Block <html>
  Block <body>
    Block <div>
      AnonymousBlock
        Text \"text \"
        Inline <i>
          Text \"a\"
      Block <p>
        Text \"block\"
      AnonymousBlock
        Text \" \"
        Inline <span>
          Text \"tail\"
      Block <p>
        Text \"b\"
"
        );
    }

    #[test]
    fn whitespace_only_runs_between_blocks_are_dropped() {
        let out = dump("<div>\n  <p>a</p>\n  <p>b</p>\n</div>");
        assert!(!out.contains("Anonymous"), "{out}");
        assert_eq!(out.matches("Block <p>").count(), 2);
    }

    #[test]
    fn inline_with_block_child_becomes_block() {
        // (Not <p>: the HTML parser closes a <p> when a <div> starts.)
        let out = dump("<div>x <span>a <div>block</div> b</span> y</div>");
        assert_eq!(
            out,
            "\
Block <html>
  Block <body>
    Block <div>
      AnonymousBlock
        Text \"x \"
      Block <span>
        AnonymousBlock
          Text \"a \"
        Block <div>
          Text \"block\"
        AnonymousBlock
          Text \" b\"
      AnonymousBlock
        Text \" y\"
"
        );
    }

    #[test]
    fn nested_inlines_around_a_block_all_become_blocks() {
        let out = dump("<span><i><div>x</div></i></span>");
        assert!(
            out.contains("Block <span>\n      Block <i>\n        Block <div>"),
            "{out}"
        );
    }

    #[test]
    fn list_items_get_markers() {
        let out =
            dump("<ul><li>a</li><li>b</li></ul><ol start=3><li>c</li><p>not li</p><li>d</li></ol>");
        assert!(out.contains("Block <li> marker=\"\u{2022} \"\n"), "{out}");
        assert!(out.contains("Block <li> marker=\"3. \"\n"), "{out}");
        assert!(out.contains("Block <li> marker=\"4. \"\n"), "{out}");
    }

    #[test]
    fn br_and_inline_block() {
        let out = dump("<p>a<br>b <span style='display:inline-block'>c</span></p>");
        assert_eq!(
            out,
            "\
Block <html>
  Block <body>
    Block <p>
      Text \"a\"
      Break
      Text \"b \"
      InlineBlock <span>
        Text \"c\"
"
        );
    }

    #[test]
    fn flex_is_block_for_now() {
        let out = dump("<div style='display:flex'><span>x</span></div>");
        assert!(out.contains("Block <div>\n      Inline <span>"), "{out}");
    }

    #[test]
    fn root_is_always_a_block() {
        let out = dump("<html style='display:inline'><body>x</body></html>");
        assert!(out.starts_with("Block <html>\n"), "{out}");
        let empty = {
            let doc = Document::new();
            let styles = style_document(&doc, &[], Viewport::default());
            BoxTree::build(&doc, &styles)
        };
        assert_eq!(empty.nodes.len(), 1);
        assert_eq!(empty.nodes[0].kind, Kind::Block);
    }

    #[test]
    fn collapse_normal() {
        assert_eq!(
            collapse_whitespace("a  b\n\tc", WhiteSpace::Normal),
            "a b c"
        );
        assert_eq!(collapse_whitespace("  a  ", WhiteSpace::Nowrap), " a ");
        assert_eq!(collapse_whitespace("\n\n", WhiteSpace::Normal), " ");
        assert_eq!(collapse_whitespace("", WhiteSpace::Normal), "");
        assert_eq!(
            collapse_whitespace("a\u{a0}b", WhiteSpace::Normal),
            "a\u{a0}b"
        );
    }

    #[test]
    fn collapse_pre_line() {
        assert_eq!(
            collapse_whitespace("a  \n  b   c\n\n d", WhiteSpace::PreLine),
            "a\nb c\nd"
        );
    }

    #[test]
    fn preserve_pre_and_expand_tabs() {
        assert_eq!(collapse_whitespace("a\tb", WhiteSpace::Pre), "a       b");
        assert_eq!(
            collapse_whitespace("12345678\tx\n\ty", WhiteSpace::PreWrap),
            "12345678        x\n        y"
        );
        assert_eq!(collapse_whitespace("a\r\nb", WhiteSpace::Pre), "a\nb");
    }

    #[test]
    fn empty_text_nodes_produce_no_boxes() {
        let out = dump("<p></p><p> </p>");
        assert_eq!(out.matches("Text").count(), 1, "{out}");
        assert!(out.contains("Text \" \""), "{out}");
    }

    #[test]
    fn deep_trees_are_flattened_at_the_depth_limit() {
        let html = "<div>".repeat(MAX_DEPTH * 4) + "deep";
        let doc = luma_html::parse(&html);
        let styles = style_document(&doc, &[], Viewport::default());
        let tree = BoxTree::build(&doc, &styles);
        assert!(
            tree.nodes.len() >= MAX_DEPTH * 4 + 3,
            "html, body, divs, text"
        );
        let mut depth = 0;
        let mut idx = tree.root;
        while let Some(&first) = tree.nodes[idx].children.first() {
            depth += 1;
            idx = first;
        }
        assert!(depth <= MAX_DEPTH + 1, "depth {depth}");
        // Flattening attaches overflow children as siblings of the box at
        // the depth limit, so the first-child spine ends on an empty
        // block; the text is still in the tree.
        assert!(
            tree.nodes
                .iter()
                .any(|n| matches!(&n.kind, Kind::Text(t) if t.contains("deep"))),
            "text survived flattening"
        );
    }
}
