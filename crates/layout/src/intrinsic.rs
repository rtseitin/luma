//! Intrinsic widths: the min-content and max-content widths of a box
//! ([CSS Sizing 3 §4][sizing]), used for shrink-to-fit sizing of
//! inline-blocks ([CSS 2.1 §10.3.9][stf]).
//!
//! [sizing]: https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes
//! [stf]: https://www.w3.org/TR/CSS21/visudet.html#inlineblock-width
//!
//! Max-content is the width with no soft wraps at all (the longest line
//! between forced breaks); min-content is the width of the widest
//! unbreakable chunk. Percentages have nothing definite to resolve against
//! here and count as zero.

use luma_style::{LengthPct, MaxSize, Size};

use crate::block::Ctx;
use crate::box_tree::BoxIdx;
use crate::geometry::{clamp_min_max, sanitize};
use crate::inline::{ItemKind, flatten, inline_edges};

fn px_only(s: Size) -> Option<f32> {
    match s {
        Size::Length(LengthPct::Px(px)) => Some(sanitize(px).max(0.0)),
        _ => None,
    }
}

fn px_only_max(s: MaxSize) -> Option<f32> {
    match s {
        MaxSize::Length(LengthPct::Px(px)) => Some(sanitize(px).max(0.0)),
        _ => None,
    }
}

impl Ctx<'_> {
    /// `(min-content, max-content)` widths of the content box of `bx`,
    /// memoised.
    pub(crate) fn intrinsic_widths(&mut self, bx: BoxIdx) -> (f32, f32) {
        if let Some(v) = self.intrinsic[bx] {
            return v;
        }
        let bt = self.bt;
        let style = bt.get(bx).style.clone();
        let (min, max) = match px_only(style.width) {
            Some(w) => (w, w),
            None if bt.has_inline_content(bx) => self.inline_intrinsic(bx),
            None => {
                let mut min = 0.0f32;
                let mut max = 0.0f32;
                for &child in &bt.get(bx).children {
                    let (cmin, cmax) = self.intrinsic_widths(child);
                    let extra = self.horizontal_edges_px(child);
                    min = min.max(cmin + extra);
                    max = max.max(cmax + extra);
                }
                (min, max)
            }
        };
        let min_w = px_only(style.min_width);
        let max_w = px_only_max(style.max_width);
        let v = (
            sanitize(clamp_min_max(min, min_w, max_w)),
            sanitize(clamp_min_max(max, min_w, max_w)),
        );
        self.intrinsic[bx] = Some(v);
        v
    }

    /// Fixed horizontal margins, borders, and padding of `bx`; `auto` and
    /// percentages are zero.
    fn horizontal_edges_px(&self, bx: BoxIdx) -> f32 {
        let (p, b, ml, mr) = inline_edges(&self.bt.get(bx).style, None);
        p.left + p.right + b.left + b.right + ml + mr
    }

    fn inline_intrinsic(&mut self, bx: BoxIdx) -> (f32, f32) {
        let items = flatten(self, bx, None);
        let mut min = 0.0f32;
        let mut max = 0.0f32;
        // Running sums for the current unbreakable chunk and the current
        // line, each with the width of its trailing collapsible spaces
        // (which vanish at a break and so don't count).
        let mut chunk = 0.0f32;
        let mut chunk_trailing = 0.0f32;
        let mut line = 0.0f32;
        let mut line_trailing = 0.0f32;
        for item in &items {
            if matches!(item.kind, ItemKind::Break) {
                min = min.max(chunk - chunk_trailing);
                max = max.max(line - line_trailing);
                chunk = 0.0;
                chunk_trailing = 0.0;
                line = 0.0;
                line_trailing = 0.0;
                continue;
            }
            if item.break_before {
                min = min.max(chunk - chunk_trailing);
                chunk = 0.0;
                chunk_trailing = 0.0;
            }
            let (wmin, wmax) = match item.kind {
                ItemKind::Atom(b) => {
                    let (a, z) = self.intrinsic_widths(b);
                    let extra = self.horizontal_edges_px(b);
                    (a + extra, z + extra)
                }
                _ => (item.width, item.width),
            };
            let collapsible = matches!(
                item.kind,
                ItemKind::Text {
                    collapsible: true,
                    ..
                }
            );
            if collapsible {
                chunk_trailing += wmin;
                line_trailing += wmax;
            } else if !matches!(item.kind, ItemKind::Open(_) | ItemKind::Close(_)) {
                chunk_trailing = 0.0;
                line_trailing = 0.0;
            }
            chunk += wmin;
            line += wmax;
        }
        min = min.max(chunk - chunk_trailing);
        max = max.max(line - line_trailing);
        (min.max(0.0), max.max(0.0))
    }
}
