//! Block layout: CSS 2.1 [§10.3.3][w] (widths and margins of block boxes),
//! [§10.6.3][h] (`auto` heights), [§10.4][minmax] (`min`/`max` clamping),
//! and [§8.3.1][mc] (margin collapsing).
//!
//! [w]: https://www.w3.org/TR/CSS21/visudet.html#blockwidth
//! [h]: https://www.w3.org/TR/CSS21/visudet.html#normal-block
//! [minmax]: https://www.w3.org/TR/CSS21/visudet.html#min-max-widths
//! [mc]: https://www.w3.org/TR/CSS21/box.html#collapsing-margins
//!
//! Coordinates produced here are relative to the parent's content origin;
//! [`LayoutTree::absolutize`](crate::LayoutTree) fixes them up at the end.
//! Working relatively is what makes margin collapsing simple: a child is
//! laid out first and positioned afterwards, once its collapsed top margin
//! is known.

use std::collections::HashMap;
use std::sync::Arc;

use luma_style::{ComputedStyle, Edges, Size};
use luma_text::FontContext;

use crate::box_tree::{BoxIdx, BoxTree, Kind};
use crate::geometry::{
    Rect, border_edges, clamp_min_max, resolve_edges, resolve_max, resolve_size,
    resolve_size_indefinite, sanitize,
};
use crate::inline::{self, InlineMetrics};
use crate::tree::{BoxKind, LayoutBox, LayoutBoxId, TextFragment};

/// Layout state: the box tree being laid out and the arena being built.
pub(crate) struct Ctx<'a> {
    pub(crate) bt: &'a BoxTree,
    pub(crate) fonts: &'a FontContext,
    pub(crate) boxes: Vec<LayoutBox>,
    /// Memoised `(min-content, max-content)` widths per box tree node.
    pub(crate) intrinsic: Vec<Option<(f32, f32)>>,
    /// Font and vertical metrics per style, keyed by the `Arc` pointer
    /// (styles live as long as the box tree, so pointers are stable).
    metrics: HashMap<usize, InlineMetrics>,
}

/// A set of adjoining vertical margins, collapsed per §8.3.1: the result
/// is the largest positive margin plus the most negative one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct MarginSet {
    pos: f32,
    neg: f32,
}

impl MarginSet {
    pub(crate) fn from(m: f32) -> Self {
        let mut s = Self::default();
        s.add(m);
        s
    }

    pub(crate) fn add(&mut self, m: f32) {
        if m >= 0.0 {
            self.pos = self.pos.max(m);
        } else {
            self.neg = self.neg.min(m);
        }
    }

    pub(crate) fn union(&mut self, other: MarginSet) {
        self.pos = self.pos.max(other.pos);
        self.neg = self.neg.min(other.neg);
    }

    pub(crate) fn collapsed(self) -> f32 {
        self.pos + self.neg
    }
}

/// What a parent needs to know after laying out a block child.
pub(crate) struct BlockOut {
    pub(crate) id: LayoutBoxId,
    /// Height of the border box.
    pub(crate) border_box_height: f32,
    /// The box's top margin together with any child margins that collapsed
    /// through its top edge.
    pub(crate) top: MarginSet,
    /// Likewise for the bottom edge.
    pub(crate) bottom: MarginSet,
    /// An empty block whose top and bottom margins collapse with each
    /// other (§8.3.1 "may be adjoining").
    pub(crate) self_collapsing: bool,
    /// Baselines of the first and last line boxes inside, relative to the
    /// border-box top.
    pub(crate) first_baseline: Option<f32>,
    pub(crate) last_baseline: Option<f32>,
}

/// Result of laying out a box's children into its content box.
pub(crate) struct ContentOut {
    /// Content height, excluding margins that collapsed through the
    /// container's edges.
    pub(crate) height: f32,
    /// Child margins that collapsed through the top edge.
    pub(crate) top_through: MarginSet,
    /// Child margins that collapsed through the bottom edge.
    pub(crate) bottom_through: MarginSet,
    /// No line boxes and no non-self-collapsing children.
    pub(crate) empty: bool,
    /// Relative to the content-box top.
    pub(crate) first_baseline: Option<f32>,
    pub(crate) last_baseline: Option<f32>,
}

/// An inline-block laid out as an atomic inline.
pub(crate) struct AtomOut {
    pub(crate) id: LayoutBoxId,
    pub(crate) margin_width: f32,
    pub(crate) margin_height: f32,
    /// Baseline relative to the margin-box top, if the box has line boxes.
    pub(crate) baseline: Option<f32>,
}

/// Horizontal geometry from §10.3.3.
struct Horizontal {
    padding: Edges<f32>,
    border: Edges<f32>,
    margin_left: f32,
    margin_right: f32,
    width: f32,
}

/// Vertical sizing inputs.
struct Vertical {
    margin_top: f32,
    margin_bottom: f32,
    /// `height` if definite.
    specified: Option<f32>,
    min: f32,
    max: Option<f32>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(bt: &'a BoxTree, fonts: &'a FontContext) -> Self {
        Self {
            bt,
            fonts,
            boxes: Vec::with_capacity(bt.nodes.len() * 2),
            intrinsic: vec![None; bt.nodes.len()],
            metrics: HashMap::new(),
        }
    }

    /// Font and inline metrics for a style, cached.
    pub(crate) fn metrics_for(&mut self, style: &Arc<ComputedStyle>) -> InlineMetrics {
        let key = Arc::as_ptr(style) as usize;
        if let Some(m) = self.metrics.get(&key) {
            return *m;
        }
        let m = inline::inline_metrics(self.fonts, style);
        self.metrics.insert(key, m);
        m
    }

    pub(crate) fn push(&mut self, b: LayoutBox) -> LayoutBoxId {
        let id = LayoutBoxId(u32::try_from(self.boxes.len()).unwrap_or(u32::MAX));
        self.boxes.push(b);
        id
    }

    pub(crate) fn get_mut(&mut self, id: LayoutBoxId) -> &mut LayoutBox {
        &mut self.boxes[id.index()]
    }

    /// Lay out a block-level box whose containing block is `cb_width` wide
    /// and `cb_height` tall (if definite). The box's `content.y` is left at
    /// zero for the caller to set once margins are collapsed; `content.x`
    /// is final (relative to the parent's content origin).
    pub(crate) fn layout_block(
        &mut self,
        bx: BoxIdx,
        cb_width: f32,
        cb_height: Option<f32>,
        is_root: bool,
    ) -> BlockOut {
        let bt = self.bt;
        let node = bt.get(bx);
        let style = node.style.clone();
        let h = block_horizontal(&style, cb_width);
        let v = vertical(&style, cb_width, cb_height);

        let kind = match node.kind {
            Kind::AnonymousBlock => BoxKind::AnonymousBlock,
            _ => BoxKind::Block,
        };
        let id = self.push(LayoutBox {
            kind,
            node: node.node,
            style: style.clone(),
            content: Rect::new(
                h.margin_left + h.border.left + h.padding.left,
                0.0,
                h.width,
                0.0,
            ),
            padding: h.padding,
            border: h.border,
            margin: Edges {
                top: v.margin_top,
                right: h.margin_right,
                bottom: v.margin_bottom,
                left: h.margin_left,
            },
            children: Vec::new(),
        });

        let child_cb_height = v.specified.map(|hh| clamp_min_max(hh, Some(v.min), v.max));
        let can_collapse_top = !is_root && h.border.top == 0.0 && h.padding.top == 0.0;
        let can_collapse_bottom =
            !is_root && h.border.bottom == 0.0 && h.padding.bottom == 0.0 && v.specified.is_none();

        let content = self.layout_children(
            bx,
            id,
            h.width,
            child_cb_height,
            can_collapse_top,
            can_collapse_bottom,
        );

        let height = clamp_min_max(v.specified.unwrap_or(content.height), Some(v.min), v.max);
        self.get_mut(id).content.height = height;

        if let Some(marker) = &node.marker {
            self.add_marker(id, &style, marker, content.first_baseline);
        }

        let mut top = MarginSet::from(v.margin_top);
        top.union(content.top_through);
        let mut bottom = MarginSet::from(v.margin_bottom);
        bottom.union(content.bottom_through);

        let no_edges = h.border.top == 0.0
            && h.border.bottom == 0.0
            && h.padding.top == 0.0
            && h.padding.bottom == 0.0;
        let self_collapsing = !is_root && content.empty && height == 0.0 && no_edges;

        let top_offset = h.border.top + h.padding.top;
        BlockOut {
            id,
            border_box_height: height
                + h.padding.top
                + h.padding.bottom
                + h.border.top
                + h.border.bottom,
            top,
            bottom,
            self_collapsing,
            first_baseline: content.first_baseline.map(|b| b + top_offset),
            last_baseline: content.last_baseline.map(|b| b + top_offset),
        }
    }

    /// Lay out the children of block container `bx` (whose layout box is
    /// `id`) into a content box `width` wide.
    pub(crate) fn layout_children(
        &mut self,
        bx: BoxIdx,
        id: LayoutBoxId,
        width: f32,
        cb_height: Option<f32>,
        can_collapse_top: bool,
        can_collapse_bottom: bool,
    ) -> ContentOut {
        if self.bt.has_inline_content(bx) {
            let out = inline::layout_inline_content(self, bx, id, width, cb_height);
            return ContentOut {
                height: out.height,
                top_through: MarginSet::default(),
                bottom_through: MarginSet::default(),
                empty: out.line_count == 0,
                first_baseline: out.first_baseline,
                last_baseline: out.last_baseline,
            };
        }
        self.layout_block_flow(
            bx,
            id,
            width,
            cb_height,
            can_collapse_top,
            can_collapse_bottom,
        )
    }

    /// Stack block children vertically with margin collapsing.
    fn layout_block_flow(
        &mut self,
        bx: BoxIdx,
        id: LayoutBoxId,
        width: f32,
        cb_height: Option<f32>,
        can_collapse_top: bool,
        can_collapse_bottom: bool,
    ) -> ContentOut {
        let bt = self.bt;
        let mut cursor = 0.0f32;
        // Margins below the last placed child that have not been applied.
        let mut pending = MarginSet::default();
        // Still at the top edge: child margins collapse through it.
        let mut at_top = can_collapse_top;
        let mut top_through = MarginSet::default();
        let mut placed_any = false;
        let mut first_baseline = None;
        let mut last_baseline = None;

        for &child in &bt.get(bx).children {
            let out = self.layout_block(child, width, cb_height, false);
            let child_top_offset = {
                let b = &self.boxes[out.id.index()];
                b.border.top + b.padding.top
            };
            self.get_mut(id).children.push(out.id);

            if out.self_collapsing {
                let mut m = out.top;
                m.union(out.bottom);
                let y = if at_top {
                    top_through.union(m);
                    0.0
                } else {
                    pending.union(m);
                    cursor + pending.collapsed()
                };
                self.get_mut(out.id).content.y = sanitize(y + child_top_offset);
                continue;
            }

            let y = if at_top {
                top_through.union(out.top);
                at_top = false;
                0.0
            } else {
                pending.union(out.top);
                cursor + pending.collapsed()
            };
            self.get_mut(out.id).content.y = sanitize(y + child_top_offset);
            cursor = sanitize(y + out.border_box_height);
            pending = out.bottom;
            placed_any = true;
            if first_baseline.is_none() {
                first_baseline = out.first_baseline.map(|b| y + b);
            }
            if let Some(b) = out.last_baseline {
                last_baseline = Some(y + b);
            }
        }

        let (height, bottom_through) = if !placed_any && at_top {
            // Everything collapsed through the top edge.
            (0.0, MarginSet::default())
        } else if can_collapse_bottom {
            (cursor, pending)
        } else {
            (cursor + pending.collapsed(), MarginSet::default())
        };

        ContentOut {
            height: sanitize(height).max(0.0),
            top_through,
            bottom_through,
            empty: !placed_any,
            first_baseline,
            last_baseline,
        }
    }

    /// Lay out an inline-block (§10.3.9 shrink-to-fit width, §10.6.6
    /// height). Its position is set by inline layout afterwards; the box
    /// establishes a new block formatting context, so child margins never
    /// collapse through its edges.
    pub(crate) fn layout_inline_block(
        &mut self,
        bx: BoxIdx,
        cb_width: f32,
        cb_height: Option<f32>,
    ) -> AtomOut {
        let bt = self.bt;
        let node = bt.get(bx);
        let style = node.style.clone();
        let padding = resolve_edges(&style.padding, cb_width);
        let border = border_edges(&style.border_width);
        let margin = Edges {
            top: resolve_size(style.margin.top, cb_width).unwrap_or(0.0),
            right: resolve_size(style.margin.right, cb_width).unwrap_or(0.0),
            bottom: resolve_size(style.margin.bottom, cb_width).unwrap_or(0.0),
            left: resolve_size(style.margin.left, cb_width).unwrap_or(0.0),
        };
        let edges = padding.left + padding.right + border.left + border.right;
        let min_w = resolve_size(style.min_width, cb_width).unwrap_or(0.0);
        let max_w = resolve_max(style.max_width, Some(cb_width));

        let width = match resolve_size(style.width, cb_width) {
            Some(w) => w,
            None => {
                let (min_c, max_c) = self.intrinsic_widths(bx);
                let available = (cb_width - margin.left - margin.right - edges).max(0.0);
                min_c.max(available).min(max_c)
            }
        };
        let width = clamp_min_max(width, Some(min_w), max_w);
        let v = vertical(&style, cb_width, cb_height);

        let id = self.push(LayoutBox {
            kind: BoxKind::InlineBlock,
            node: node.node,
            style: style.clone(),
            content: Rect::new(0.0, 0.0, width, 0.0),
            padding,
            border,
            margin,
            children: Vec::new(),
        });

        let child_cb_height = v.specified.map(|hh| clamp_min_max(hh, Some(v.min), v.max));
        let content = self.layout_children(bx, id, width, child_cb_height, false, false);
        let height = clamp_min_max(v.specified.unwrap_or(content.height), Some(v.min), v.max);
        self.get_mut(id).content.height = height;
        if let Some(marker) = &node.marker {
            self.add_marker(id, &style, marker, content.first_baseline);
        }

        let top_offset = margin.top + border.top + padding.top;
        AtomOut {
            id,
            margin_width: sanitize(margin.left + edges + width + margin.right),
            margin_height: sanitize(
                top_offset + height + padding.bottom + border.bottom + margin.bottom,
            ),
            baseline: content.last_baseline.map(|b| b + top_offset),
        }
    }

    /// Add a list marker box outside the content box of `id`, aligned with
    /// the first line's baseline (or where the first line would be).
    fn add_marker(
        &mut self,
        id: LayoutBoxId,
        style: &Arc<ComputedStyle>,
        text: &str,
        first_baseline: Option<f32>,
    ) {
        let m = self.metrics_for(style);
        let run = self.fonts.shape(m.font, style.font_size, text);
        let baseline = first_baseline.unwrap_or(m.above);
        let marker = self.push(LayoutBox {
            kind: BoxKind::ListMarker(TextFragment {
                text: text.to_owned(),
                run: run.clone(),
                font: m.font,
                baseline,
            }),
            node: None,
            style: style.clone(),
            content: Rect::new(
                -run.advance,
                baseline - m.ascent,
                run.advance,
                m.ascent + m.descent,
            ),
            padding: Edges::all(0.0),
            border: Edges::all(0.0),
            margin: Edges::all(0.0),
            children: Vec::new(),
        });
        self.get_mut(id).children.insert(0, marker);
    }
}

/// §10.3.3: resolve padding, borders, `width`, and horizontal margins of
/// a block-level box against its containing block width, then clamp the
/// width by `min-width`/`max-width` (§10.4) and redo the margins.
fn block_horizontal(style: &ComputedStyle, cb_width: f32) -> Horizontal {
    let padding = resolve_edges(&style.padding, cb_width);
    let border = border_edges(&style.border_width);
    let edges = padding.left + padding.right + border.left + border.right;
    let ml = resolve_size(style.margin.left, cb_width);
    let mr = resolve_size(style.margin.right, cb_width);
    let min_w = resolve_size(style.min_width, cb_width).unwrap_or(0.0);
    let max_w = resolve_max(style.max_width, Some(cb_width));

    let solve = |width: Option<f32>| -> (f32, f32, f32) {
        match width {
            None => {
                // Auto width absorbs everything; auto margins become 0.
                let ml = ml.unwrap_or(0.0);
                let mr = mr.unwrap_or(0.0);
                let w = (cb_width - ml - mr - edges).max(0.0);
                (ml, mr, w)
            }
            Some(w) => {
                let fixed = edges + w + ml.unwrap_or(0.0) + mr.unwrap_or(0.0);
                let (ml, mr) = if fixed > cb_width {
                    // Over-constrained: auto margins are 0 and the right
                    // margin takes the (negative) remainder.
                    let ml = ml.unwrap_or(0.0);
                    (ml, cb_width - edges - w - ml)
                } else {
                    match (ml, mr) {
                        (None, None) => {
                            let half = (cb_width - edges - w) / 2.0;
                            (half, half)
                        }
                        (None, Some(mr)) => (cb_width - edges - w - mr, mr),
                        (Some(ml), None) => (ml, cb_width - edges - w - ml),
                        (Some(ml), Some(_)) => (ml, cb_width - edges - w - ml),
                    }
                };
                (ml, mr, w)
            }
        }
    };

    let specified = resolve_size(style.width, cb_width);
    let (mut margin_left, mut margin_right, mut width) = solve(specified);
    let clamped = clamp_min_max(width, Some(min_w), max_w);
    if clamped != width {
        (margin_left, margin_right, width) = solve(Some(clamped));
    }
    Horizontal {
        padding,
        border,
        margin_left: sanitize(margin_left),
        margin_right: sanitize(margin_right),
        width: sanitize(width).max(0.0),
    }
}

/// Vertical margins (percentages against the containing block *width*,
/// §8.3) and the height constraints (§10.5, §10.7).
fn vertical(style: &ComputedStyle, cb_width: f32, cb_height: Option<f32>) -> Vertical {
    let margin_of = |s: Size| resolve_size(s, cb_width).unwrap_or(0.0);
    Vertical {
        margin_top: margin_of(style.margin.top),
        margin_bottom: margin_of(style.margin.bottom),
        specified: resolve_size_indefinite(style.height, cb_height).map(|h| h.max(0.0)),
        min: resolve_size_indefinite(style.min_height, cb_height)
            .unwrap_or(0.0)
            .max(0.0),
        max: resolve_max(style.max_height, cb_height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_sets_collapse_per_spec() {
        let mut s = MarginSet::from(16.0);
        s.add(16.0);
        assert_eq!(s.collapsed(), 16.0);
        let mut s = MarginSet::from(20.0);
        s.add(10.0);
        assert_eq!(s.collapsed(), 20.0);
        let mut s = MarginSet::from(20.0);
        s.add(-5.0);
        assert_eq!(s.collapsed(), 15.0);
        let mut s = MarginSet::from(-5.0);
        s.add(-10.0);
        assert_eq!(s.collapsed(), -10.0);
        let mut s = MarginSet::from(8.0);
        s.union(MarginSet::from(21.0));
        s.union(MarginSet::from(-3.0));
        assert_eq!(s.collapsed(), 18.0);
        assert_eq!(MarginSet::default().collapsed(), 0.0);
    }
}
