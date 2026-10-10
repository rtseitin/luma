//! The layout tree: an arena of positioned boxes.

use std::fmt::Write as _;
use std::ops::Index;
use std::sync::Arc;

use luma_dom::{Document, NodeId};
use luma_style::{ComputedStyle, Edges, Viewport};
use luma_text::{FontId, ShapedRun};

use crate::geometry::Rect;

/// Handle to a box in a [`LayoutTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayoutBoxId(pub(crate) u32);

impl LayoutBoxId {
    /// Position of the box in the tree's arena.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One line's worth of text from a single text node, shaped.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFragment {
    /// The characters on this line, after whitespace processing.
    pub text: String,
    /// The glyphs for `text`. Its `advance` equals the box's content
    /// width; spaces on justified lines have widened advances.
    pub run: ShapedRun,
    /// The face the text was shaped with. The size is the style's
    /// `font_size`.
    pub font: FontId,
    /// Absolute y coordinate of the baseline the glyphs sit on.
    pub baseline: f32,
}

/// What a [`LayoutBox`] is.
#[derive(Debug, Clone, PartialEq)]
pub enum BoxKind {
    /// A block-level block container.
    Block,
    /// A block container generated to hold inline content that sits next
    /// to block-level siblings (CSS 2.1 §9.2.1.1).
    AnonymousBlock,
    /// One line's fragment of an inline box (`<span>`, `<a>`, ...). An
    /// inline that spans several lines produces one `Inline` box per line;
    /// only the first carries left edges and only the last right edges.
    Inline,
    /// An inline-level block container (`display: inline-block`).
    InlineBlock,
    /// One line's worth of a text node.
    Text(TextFragment),
    /// A line box: holds the fragments of one line of inline content.
    LineBox,
    /// A list item's marker (`• `, `3. `), positioned outside the content
    /// box, aligned with the first line.
    ListMarker(TextFragment),
}

impl BoxKind {
    /// Short name used by [`LayoutTree::dump`].
    pub fn name(&self) -> &'static str {
        match self {
            BoxKind::Block => "Block",
            BoxKind::AnonymousBlock => "AnonymousBlock",
            BoxKind::Inline => "Inline",
            BoxKind::InlineBlock => "InlineBlock",
            BoxKind::Text(_) => "Text",
            BoxKind::LineBox => "Line",
            BoxKind::ListMarker(_) => "Marker",
        }
    }
}

/// A box with resolved geometry.
///
/// All rectangles are in absolute CSS pixels relative to the top-left of
/// the document (not the viewport; scrolling is the caller's business).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    /// What kind of box this is.
    pub kind: BoxKind,
    /// The DOM node that generated the box; `None` for anonymous boxes
    /// (anonymous blocks, line boxes, markers).
    pub node: Option<NodeId>,
    /// The computed style the box was laid out with. Anonymous boxes get a
    /// style inherited from their parent.
    pub style: Arc<ComputedStyle>,
    /// The content box.
    pub content: Rect,
    /// Resolved padding, in pixels.
    pub padding: Edges<f32>,
    /// Resolved border widths, in pixels.
    pub border: Edges<f32>,
    /// Resolved margins, in pixels. For blocks these are the used values
    /// before collapsing; the collapsed result is reflected in positions.
    pub margin: Edges<f32>,
    /// Child boxes in tree (paint) order.
    pub children: Vec<LayoutBoxId>,
}

impl LayoutBox {
    /// Content box plus padding.
    pub fn padding_rect(&self) -> Rect {
        self.content.outset(&self.padding)
    }

    /// Padding box plus borders.
    pub fn border_rect(&self) -> Rect {
        self.padding_rect().outset(&self.border)
    }

    /// Border box plus margins.
    pub fn margin_rect(&self) -> Rect {
        self.border_rect().outset(&self.margin)
    }

    /// The text of a `Text` or `ListMarker` box.
    pub fn text(&self) -> Option<&TextFragment> {
        match &self.kind {
            BoxKind::Text(t) | BoxKind::ListMarker(t) => Some(t),
            _ => None,
        }
    }
}

/// The result of layout: positioned boxes in an arena.
#[derive(Debug, Clone)]
pub struct LayoutTree {
    pub(crate) boxes: Vec<LayoutBox>,
    pub(crate) root: LayoutBoxId,
    pub(crate) viewport: Viewport,
    pub(crate) document_height: f32,
}

impl LayoutTree {
    /// The root block (the `<html>` element's box).
    pub fn root(&self) -> LayoutBoxId {
        self.root
    }

    /// Borrow a box. Also available as `tree[id]`.
    pub fn get(&self, id: LayoutBoxId) -> &LayoutBox {
        &self.boxes[id.index()]
    }

    /// Number of boxes.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// Whether there are no boxes (never, after [`crate::layout`]).
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// The viewport the tree was laid out for.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Height of the whole document: the bottom of the lowest margin box,
    /// or the viewport height if the content is shorter. This is the
    /// scrollable height.
    pub fn document_height(&self) -> f32 {
        self.document_height
    }

    /// Every box with its id, in arena order (parents before children).
    pub fn iter(&self) -> impl Iterator<Item = (LayoutBoxId, &LayoutBox)> {
        self.boxes
            .iter()
            .enumerate()
            .map(|(i, b)| (LayoutBoxId(i as u32), b))
    }

    /// Ids of `id` and all of its descendants in tree (paint) order.
    pub fn pre_order(&self, id: LayoutBoxId) -> Vec<LayoutBoxId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            out.push(id);
            stack.extend(self.get(id).children.iter().rev());
        }
        out
    }

    /// All boxes generated by DOM node `node`, in tree order. Blocks have
    /// exactly one; inlines and text nodes have one per line they occupy.
    pub fn boxes_for(&self, node: NodeId) -> Vec<LayoutBoxId> {
        self.pre_order(self.root)
            .into_iter()
            .filter(|&id| self.get(id).node == Some(node))
            .collect()
    }

    /// An indented, one-line-per-box description, e.g.
    ///
    /// ```text
    /// Block <html> at (0,0) size 1024x768
    ///   Block <body> at (8,8) size 1008x37
    ///     Block <p#intro.note> at (8,8) size 1008x37
    ///       Line at (8,8) size 1008x18.4
    ///         Text "Hello" at (8,9.6) size 41.2x14.9 baseline=21.2
    /// ```
    ///
    /// Numbers are rounded to two decimals. Pass the document to label
    /// element boxes with their tag, id, and classes; without it only kinds
    /// are shown.
    pub fn dump(&self, doc: Option<&Document>) -> String {
        let mut out = String::new();
        let mut stack = vec![(self.root, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            let b = self.get(id);
            out.push_str(&"  ".repeat(depth));
            out.push_str(b.kind.name());
            match &b.kind {
                BoxKind::Text(t) | BoxKind::ListMarker(t) => {
                    let _ = write!(out, " {:?}", t.text);
                }
                _ => {
                    if let Some(el) = doc.and_then(|d| b.node.and_then(|n| d.element(n))) {
                        out.push_str(" <");
                        out.push_str(&el.name);
                        if let Some(id) = el.id() {
                            let _ = write!(out, "#{id}");
                        }
                        for class in el.classes() {
                            let _ = write!(out, ".{class}");
                        }
                        out.push('>');
                    }
                }
            }
            let _ = write!(
                out,
                " at ({},{}) size {}x{}",
                num(b.content.x),
                num(b.content.y),
                num(b.content.width),
                num(b.content.height)
            );
            if let Some(t) = b.text() {
                let _ = write!(out, " baseline={}", num(t.baseline));
            }
            out.push('\n');
            stack.extend(b.children.iter().rev().map(|&c| (c, depth + 1)));
        }
        out
    }

    /// Convert parent-relative coordinates to absolute ones.
    ///
    /// During layout every box's `content` rectangle (and text baseline)
    /// is relative to its parent's content origin; this single pass makes
    /// them absolute, top-down.
    pub(crate) fn absolutize(&mut self) {
        let mut stack = vec![(self.root, 0.0f32, 0.0f32)];
        while let Some((id, dx, dy)) = stack.pop() {
            let b = &mut self.boxes[id.index()];
            b.content.x += dx;
            b.content.y += dy;
            if let BoxKind::Text(t) | BoxKind::ListMarker(t) = &mut b.kind {
                t.baseline += dy;
            }
            let (cx, cy) = (b.content.x, b.content.y);
            stack.extend(b.children.iter().map(|&c| (c, cx, cy)));
        }
    }
}

impl Index<LayoutBoxId> for LayoutTree {
    type Output = LayoutBox;

    fn index(&self, id: LayoutBoxId) -> &LayoutBox {
        self.get(id)
    }
}

/// A number with at most two decimals and no trailing zeros.
pub(crate) fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting() {
        assert_eq!(num(16.0), "16");
        assert_eq!(num(13.333_333), "13.33");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(-2.25), "-2.25");
        assert_eq!(num(1e7), "10000000");
    }

    #[test]
    fn rects_nest() {
        let b = LayoutBox {
            kind: BoxKind::Block,
            node: None,
            style: Arc::new(ComputedStyle::initial()),
            content: Rect::new(100.0, 100.0, 50.0, 20.0),
            padding: Edges::all(5.0),
            border: Edges::all(1.0),
            margin: Edges {
                top: 10.0,
                right: 0.0,
                bottom: 10.0,
                left: 0.0,
            },
            children: vec![],
        };
        assert_eq!(b.padding_rect(), Rect::new(95.0, 95.0, 60.0, 30.0));
        assert_eq!(b.border_rect(), Rect::new(94.0, 94.0, 62.0, 32.0));
        assert_eq!(b.margin_rect(), Rect::new(94.0, 84.0, 62.0, 52.0));
    }
}
