//! Layout for Luma: from a styled DOM to boxes with absolute rectangles.
//!
//! [`layout`] takes a [`Document`], its [`StyleMap`], and a [`FontContext`]
//! and produces a [`LayoutTree`]: an arena of [`LayoutBox`]es, each with a
//! content rectangle in CSS pixels plus resolved padding, border, and
//! margin widths. Paint reads the tree in order; nothing here touches the
//! DOM or the styles.
//!
//! The implementation follows the normal-flow parts of [CSS 2.1][css21]:
//!
//! 1. **Box generation** ([§9.2][boxgen]): elements become block, inline,
//!    or inline-block boxes by `display`; mixed block and inline children
//!    get anonymous block wrappers; whitespace is collapsed per
//!    `white-space` ([CSS Text 3 §4][ws]).
//! 2. **Block layout** ([§10.3.3][width], [§10.6.3][height]): the width
//!    constraint equation with `auto` margins and widths, `min`/`max`
//!    clamping ([§10.4][minmax]), and vertical margin collapsing between
//!    siblings and through parents' edges ([§8.3.1][collapse]).
//! 3. **Inline layout** ([§9.4.2][ifc], [§10.8][lineheight]): text is
//!    shaped with the selected font, broken greedily into line boxes at
//!    collapsible spaces and forced breaks, aligned on a shared baseline,
//!    and shifted per `text-align`. Inline boxes that span lines produce
//!    one fragment per line. Inline-blocks are sized shrink-to-fit
//!    ([§10.3.9][stf]) and aligned by their last line's baseline.
//!
//! [css21]: https://www.w3.org/TR/CSS21/
//! [boxgen]: https://www.w3.org/TR/CSS21/visuren.html#box-gen
//! [ws]: https://www.w3.org/TR/css-text-3/#white-space-processing
//! [width]: https://www.w3.org/TR/CSS21/visudet.html#blockwidth
//! [height]: https://www.w3.org/TR/CSS21/visudet.html#normal-block
//! [minmax]: https://www.w3.org/TR/CSS21/visudet.html#min-max-widths
//! [collapse]: https://www.w3.org/TR/CSS21/box.html#collapsing-margins
//! [ifc]: https://www.w3.org/TR/CSS21/visuren.html#inline-formatting
//! [lineheight]: https://www.w3.org/TR/CSS21/visudet.html#line-height
//! [stf]: https://www.w3.org/TR/CSS21/visudet.html#inlineblock-width
//!
//! # Simplifications
//!
//! - Normal flow only: no floats, no positioning, no tables (the UA sheet
//!   approximates them with blocks), and `display: flex` is laid out as
//!   `block`.
//! - Left-to-right, horizontal writing mode only; `text-align: start` is
//!   `left` and `end` is `right`.
//! - `vertical-align` is always `baseline`. Inline boxes' vertical
//!   margins are ignored and their padding and borders don't affect line
//!   height, as the spec says.
//! - An inline box containing a block-level box is laid out as a block
//!   instead of being split around it.
//! - Line breaking happens only at collapsible spaces, after `pre-wrap`
//!   spaces, around inline-blocks, and at forced breaks: no hyphenation,
//!   no `word-break`, no break opportunities inside CJK text.
//! - Replaced elements (`<img>`, form controls) have no intrinsic size and
//!   lay out as empty inline-blocks.
//! - Documents nested deeper than 256 boxes are flattened at that depth.
//!
//! Every input, however malformed, produces a tree: lengths are clamped to
//! a finite range and no box ever has a negative size.

mod block;
mod box_tree;
mod geometry;
mod inline;
mod intrinsic;
mod tree;

use luma_dom::Document;
use luma_style::{StyleMap, Viewport};
use luma_text::FontContext;

pub use geometry::Rect;
pub use tree::{BoxKind, LayoutBox, LayoutBoxId, LayoutTree, TextFragment};

use crate::block::Ctx;
use crate::box_tree::BoxTree;
use crate::geometry::sanitize;

/// Lay out `doc` for a viewport of the given size.
///
/// `styles` must come from [`luma_style::style_document`] for the same
/// document. The root box's containing block is the viewport; its height
/// follows its content, and [`LayoutTree::document_height`] reports the
/// larger of the content and viewport heights.
pub fn layout(
    doc: &Document,
    styles: &StyleMap,
    fonts: &FontContext,
    viewport: Viewport,
) -> LayoutTree {
    let vw = sanitize(viewport.width).max(0.0);
    let vh = sanitize(viewport.height).max(0.0);
    let viewport = Viewport {
        width: vw,
        height: vh,
    };

    let bt = BoxTree::build(doc, styles);
    let mut ctx = Ctx::new(&bt, fonts);
    let out = ctx.layout_block(bt.root, vw, Some(vh), true);
    {
        let root = ctx.get_mut(out.id);
        root.content.y = sanitize(out.top.collapsed() + root.border.top + root.padding.top);
    }

    let mut tree = LayoutTree {
        boxes: ctx.boxes,
        root: out.id,
        viewport,
        document_height: vh,
    };
    tree.absolutize();
    let content_bottom = tree
        .boxes
        .iter()
        .map(|b| b.margin_rect().bottom())
        .fold(0.0f32, f32::max);
    tree.document_height = sanitize(content_bottom.max(vh));
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use luma_dom::NodeId;
    use luma_style::{FontStyle, style_document};

    /// A laid-out document with lookup helpers.
    pub(crate) struct Laid {
        pub(crate) doc: Document,
        pub(crate) tree: LayoutTree,
        pub(crate) fonts: FontContext,
    }

    impl Laid {
        pub(crate) fn new(html: &str) -> Self {
            Self::with_viewport(html, 1024.0, 768.0)
        }

        pub(crate) fn with_viewport(html: &str, w: f32, h: f32) -> Self {
            let doc = luma_html::parse(html);
            let viewport = Viewport {
                width: w,
                height: h,
            };
            let styles = style_document(&doc, &[], viewport);
            let fonts = FontContext::new();
            let tree = layout(&doc, &styles, &fonts, viewport);
            Self { doc, tree, fonts }
        }

        pub(crate) fn node(&self, id: &str) -> NodeId {
            self.doc
                .descendants(self.doc.root())
                .find(|&n| self.doc.element(n).is_some_and(|e| e.id() == Some(id)))
                .unwrap_or_else(|| panic!("no element #{id}"))
        }

        /// The single box of element `#id`.
        pub(crate) fn bx(&self, id: &str) -> &LayoutBox {
            let boxes = self.tree.boxes_for(self.node(id));
            assert_eq!(boxes.len(), 1, "#{id} has {} boxes", boxes.len());
            &self.tree[boxes[0]]
        }

        /// All fragments of element `#id`, in tree order.
        pub(crate) fn fragments(&self, id: &str) -> Vec<&LayoutBox> {
            self.tree
                .boxes_for(self.node(id))
                .into_iter()
                .map(|b| &self.tree[b])
                .collect()
        }

        pub(crate) fn tag(&self, name: &str) -> &LayoutBox {
            let n = self
                .doc
                .descendants(self.doc.root())
                .find(|&n| self.doc.element(n).is_some_and(|e| e.name == name))
                .unwrap_or_else(|| panic!("no <{name}>"));
            let boxes = self.tree.boxes_for(n);
            &self.tree[boxes[0]]
        }

        /// Text fragments beneath `#id`, in order: `(text, content rect)`.
        pub(crate) fn texts(&self, id: &str) -> Vec<(String, Rect)> {
            let root = self.tree.boxes_for(self.node(id))[0];
            self.tree
                .pre_order(root)
                .into_iter()
                .filter_map(|b| {
                    let b = &self.tree[b];
                    match &b.kind {
                        BoxKind::Text(t) => Some((t.text.clone(), b.content)),
                        _ => None,
                    }
                })
                .collect()
        }

        /// The line boxes that are direct children of `#id`'s box, in
        /// order (lines inside nested inline-blocks are not included).
        pub(crate) fn lines(&self, id: &str) -> Vec<&LayoutBox> {
            let root = self.tree.boxes_for(self.node(id))[0];
            self.tree[root]
                .children
                .iter()
                .map(|&b| &self.tree[b])
                .filter(|b| b.kind == BoxKind::LineBox)
                .collect()
        }

        pub(crate) fn dump(&self) -> String {
            self.tree.dump(Some(&self.doc))
        }

        /// Advance of one glyph of the bundled monospace font at `size`.
        pub(crate) fn mono_advance(&self, size: f32) -> f32 {
            let f = self
                .fonts
                .select(&["monospace".to_owned()], 400, FontStyle::Normal);
            self.fonts.shape(f, size, "x").advance
        }
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    // ----- block layout (3b) ----------------------------------------------------

    #[test]
    fn root_and_body_defaults() {
        let l = Laid::new("<body></body>");
        let html = l.tag("html");
        // An empty body is self-collapsing: its 8px top and bottom margins
        // collapse into one 8px gap inside html (html, the root, never
        // collapses with it).
        assert_eq!(html.content, rect(0.0, 0.0, 1024.0, 8.0), "{}", l.dump());
        let body = l.tag("body");
        assert_eq!(body.margin, luma_style::Edges::all(8.0));
        assert_eq!(body.content, rect(8.0, 8.0, 1008.0, 0.0));
        assert_eq!(l.tree.document_height(), 768.0);

        // With content, body is 8px in on every side.
        let l = Laid::new("<body><div style='height:10px'></div></body>");
        assert_eq!(l.tag("body").content, rect(8.0, 8.0, 1008.0, 10.0));
        assert_eq!(l.tag("html").content.height, 26.0);
    }

    #[test]
    fn padding_border_margin_sums() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='margin:10px; border:2px solid; padding:5px'>
                 <div id=b style='height:20px'></div>
               </div>
             </body>",
        );
        let a = l.bx("a");
        assert_eq!(
            a.margin_rect(),
            rect(0.0, 0.0, 1024.0, 54.0),
            "{}",
            l.dump()
        );
        assert_eq!(a.border_rect(), rect(10.0, 10.0, 1004.0, 34.0));
        assert_eq!(a.padding_rect(), rect(12.0, 12.0, 1000.0, 30.0));
        assert_eq!(a.content, rect(17.0, 17.0, 990.0, 20.0));
        assert_eq!(l.bx("b").content, rect(17.0, 17.0, 990.0, 20.0));
        assert_eq!(l.tag("html").content.height, 54.0);
    }

    #[test]
    fn percent_width_and_padding_resolve_against_containing_width() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='width:50%; padding:10%; height:10px'></div>
             </body>",
        );
        let a = l.bx("a");
        assert_eq!(a.content.width, 512.0);
        assert_eq!(a.padding, luma_style::Edges::all(102.4));
        // Vertical padding percentages also use the width.
        assert_eq!(a.border_rect().height, 10.0 + 2.0 * 102.4);
    }

    #[test]
    fn auto_margins_centre_a_fixed_width() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='width:200px; margin:0 auto; height:10px'></div>
               <div id=b style='width:200px; margin-left:auto; height:10px'></div>
               <div id=c style='width:200px; margin-right:auto; margin-left:50px; height:10px'></div>
             </body>",
        );
        assert_eq!(l.bx("a").content.x, 412.0);
        assert_eq!(l.bx("a").margin.left, 412.0);
        assert_eq!(l.bx("b").content.x, 824.0);
        assert_eq!(l.bx("c").content.x, 50.0);
        assert_eq!(l.bx("c").margin.right, 774.0);
    }

    #[test]
    fn over_constrained_width_adjusts_right_margin() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='width:900px; margin:0 100px; height:10px'></div>
               <div id=b style='width:2000px; margin:0 auto; height:10px'></div>
             </body>",
        );
        assert_eq!(l.bx("a").content.x, 100.0);
        assert_eq!(l.bx("a").margin.right, 1024.0 - 900.0 - 100.0);
        // Too wide to centre: auto margins become 0, overflow to the right.
        assert_eq!(l.bx("b").content.x, 0.0);
        assert_eq!(l.bx("b").content.width, 2000.0);
        assert_eq!(l.bx("b").margin.right, -976.0);
    }

    #[test]
    fn min_and_max_width_clamp_and_recentre() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='width:90%; max-width:300px; margin:0 auto; height:10px'></div>
               <div id=b style='width:10px; min-width:500px; height:10px'></div>
               <div id=c style='max-width:100px; height:10px'></div>
               <div id=d style='min-width:200px; max-width:100px; height:10px'></div>
             </body>",
        );
        assert_eq!(l.bx("a").content.width, 300.0);
        assert_eq!(l.bx("a").content.x, 362.0, "re-centred after clamping");
        assert_eq!(l.bx("b").content.width, 500.0);
        assert_eq!(l.bx("c").content.width, 100.0, "auto width is clamped too");
        assert_eq!(l.bx("d").content.width, 200.0, "min wins over max");
    }

    #[test]
    fn heights_specified_percent_and_clamped() {
        let l = Laid::with_viewport(
            "<body style='margin:0'>
               <div id=a style='height:100px'>
                 <div id=b style='height:50%'></div>
                 <div id=c style='height:300px; max-height:20px'></div>
               </div>
               <div id=d><div id=e style='height:50%'></div></div>
               <div id=f style='min-height:40px'></div>
               <html-ignored></html-ignored>
             </body>",
            800.0,
            600.0,
        );
        assert_eq!(l.bx("a").content.height, 100.0, "fixed, children overflow");
        assert_eq!(l.bx("b").content.height, 50.0);
        assert_eq!(l.bx("c").content.height, 20.0);
        assert_eq!(l.bx("e").content.height, 0.0, "percent of auto is auto");
        assert_eq!(l.bx("d").content.height, 0.0);
        assert_eq!(l.bx("f").content.height, 40.0);
        // html's containing block is the viewport, so 100% works there.
        let l2 = Laid::with_viewport("<html style='height:100%'><body>", 800.0, 600.0);
        assert_eq!(l2.tag("html").content.height, 600.0);
    }

    #[test]
    fn sibling_margins_collapse() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='height:10px; margin-bottom:16px'></div>
               <div id=b style='height:10px; margin-top:16px; margin-bottom:20px'></div>
               <div id=c style='height:10px; margin-top:10px; margin-bottom:20px'></div>
               <div id=d style='height:10px; margin-top:-5px'></div>
               <div id=e style='height:10px; margin-top:-5px; margin-bottom:-10px'></div>
               <div id=f style='height:10px; margin-top:-20px'></div>
             </body>",
        );
        assert_eq!(l.bx("a").content.y, 0.0);
        assert_eq!(l.bx("b").content.y, 26.0, "16 + 16 -> 16");
        assert_eq!(l.bx("c").content.y, 56.0, "20 + 10 -> 20");
        assert_eq!(l.bx("d").content.y, 81.0, "20 + -5 -> 15");
        assert_eq!(l.bx("e").content.y, 86.0, "0 + -5 -> -5");
        assert_eq!(l.bx("f").content.y, 76.0, "-10 + -20 -> -20");
        assert_eq!(l.tag("body").content.height, 86.0, "{}", l.dump());
    }

    #[test]
    fn parent_and_first_child_margins_collapse_unless_separated() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='margin-top:10px'>
                 <div id=a1 style='margin-top:30px; height:10px'></div>
               </div>
               <div id=b style='margin-top:10px; padding-top:1px'>
                 <div id=b1 style='margin-top:30px; height:10px'></div>
               </div>
               <div id=c style='margin-bottom:10px'>
                 <div id=c1 style='margin-bottom:30px; height:10px'></div>
               </div>
               <div id=d style='height:10px'></div>
             </body>",
        );
        // a's 10 and a1's 30 collapse to 30 and move both; a1 is flush
        // with a's top.
        assert_eq!(l.bx("a").content.y, 30.0, "{}", l.dump());
        assert_eq!(l.bx("a1").content.y, 30.0);
        assert_eq!(l.bx("a").content.height, 10.0);
        // b has padding: its own margin is 10 (collapsing with a's bottom
        // 0 and a1's 0), and b1's 30 stays inside b.
        assert_eq!(l.bx("b").content.y, 40.0 + 10.0 + 1.0);
        assert_eq!(l.bx("b1").content.y, 51.0 + 30.0);
        assert_eq!(l.bx("b").content.height, 40.0);
        // c's bottom collapses with c1's: 30 between c and d, and c's
        // height excludes it.
        let c = l.bx("c");
        assert_eq!(c.content.height, 10.0);
        assert_eq!(l.bx("d").content.y, c.content.bottom() + 30.0);
    }

    #[test]
    fn root_margins_do_not_collapse_and_body_collapses_with_heading() {
        let l = Laid::new("<h1 id=h style='margin:20px 0; height:10px'>x</h1>");
        // body's 8px collapses with h1's 20px -> 20px; html (root) keeps
        // its (zero) margin separate.
        assert_eq!(l.tag("body").content.y, 20.0, "{}", l.dump());
        assert_eq!(l.bx("h").content.y, 20.0);
        assert_eq!(l.tag("body").content.height, 10.0);
        assert_eq!(l.tag("html").content.height, 50.0);
    }

    #[test]
    fn empty_blocks_self_collapse() {
        let l = Laid::new(
            "<body style='margin:0'>
               <div id=a style='margin:10px 0'></div>
               <div id=b style='margin:15px 0'></div>
               <div id=c style='height:10px'></div>
             </body>",
        );
        // a (10/10) and b (15/15) are empty: all four margins collapse to
        // one 15px gap, which also collapses through body's top edge.
        assert_eq!(l.bx("c").content.y, 15.0, "{}", l.dump());
        assert_eq!(l.tag("body").content.y, 15.0);
        assert_eq!(l.tag("body").content.height, 10.0);
    }

    #[test]
    fn document_height_covers_overflow() {
        let l = Laid::with_viewport(
            "<body style='margin:0'><div style='height:900px; margin-bottom:50px'></div></body>",
            800.0,
            600.0,
        );
        assert_eq!(l.tree.document_height(), 950.0);
        let short = Laid::with_viewport("<body style='margin:0'></body>", 800.0, 600.0);
        assert_eq!(short.tree.document_height(), 600.0);
    }

    // ----- inline layout (3c) ---------------------------------------------------

    /// A monospace paragraph `chars` glyphs wide, with a 20px line height
    /// so vertical positions are exact.
    fn mono(l: &Laid, chars: f32) -> f32 {
        chars * l.mono_advance(16.0)
    }

    fn mono_page(content: &str, chars: f32) -> Laid {
        let adv = Laid::new("").mono_advance(16.0);
        Laid::new(&format!(
            "<body style='margin:0; font-family:monospace; font-size:16px; line-height:20px'>
               <p id=p style='margin:0; width:{}px'>{content}</p>
             </body>",
            chars * adv
        ))
    }

    #[test]
    fn greedy_wrapping_at_spaces() {
        let l = mono_page("aaaa bbbb cccc", 10.0);
        let texts = l.texts("p");
        assert_eq!(
            texts.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            ["aaaa bbbb", "cccc"],
            "{}",
            l.dump()
        );
        assert_eq!(texts[0].1.width, mono(&l, 9.0));
        assert_eq!(texts[1].1.width, mono(&l, 4.0));
        let lines = l.lines("p");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].content, rect(0.0, 0.0, mono(&l, 10.0), 20.0));
        assert_eq!(lines[1].content, rect(0.0, 20.0, mono(&l, 10.0), 20.0));
        assert_eq!(l.bx("p").content.height, 40.0);
    }

    #[test]
    fn exact_fit_does_not_wrap() {
        let l = mono_page("aaaa bbbb", 9.0);
        assert_eq!(l.lines("p").len(), 1, "{}", l.dump());
    }

    #[test]
    fn long_word_overflows_on_its_own_line() {
        let l = mono_page("ab cdefghijklmnop qr", 10.0);
        let texts: Vec<String> = l.texts("p").into_iter().map(|(t, _)| t).collect();
        assert_eq!(texts, ["ab", "cdefghijklmnop", "qr"], "{}", l.dump());
        assert_eq!(l.lines("p").len(), 3);
    }

    #[test]
    fn br_forces_breaks_and_empty_lines() {
        let l = mono_page("a<br>b<br><br>c<br>", 10.0);
        let lines = l.lines("p");
        assert_eq!(lines.len(), 4, "{}", l.dump());
        let texts = l.texts("p");
        assert_eq!(
            texts.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        // "c" is on the fourth line (y = 60); the empty third line still
        // takes a full line height.
        assert_eq!(lines[3].content.y, 60.0);
        assert!(texts[2].1.y >= 60.0 && texts[2].1.y < 80.0);
        assert_eq!(l.bx("p").content.height, 80.0);
    }

    #[test]
    fn white_space_pre_and_nowrap() {
        let l = mono_page(
            "<span id=pre style='white-space:pre'>a  b\n  c</span>",
            10.0,
        );
        let texts: Vec<String> = l.texts("p").into_iter().map(|(t, _)| t).collect();
        assert_eq!(texts, ["a  b", "  c"], "{}", l.dump());
        assert_eq!(l.fragments("pre").len(), 2, "one inline fragment per line");

        let l = mono_page(
            "<span id=nw style='white-space:nowrap'>aaaa bbbb cccc</span>",
            10.0,
        );
        assert_eq!(l.lines("p").len(), 1, "{}", l.dump());
        assert_eq!(l.texts("p")[0].1.width, mono(&l, 14.0));

        let l = mono_page("<span style='white-space:pre-line'>a\n  b   c</span>", 10.0);
        let texts: Vec<String> = l.texts("p").into_iter().map(|(t, _)| t).collect();
        assert_eq!(texts, ["a", "b c"]);
    }

    #[test]
    fn pre_wrap_keeps_spaces_and_wraps() {
        let l = mono_page(
            "<span style='white-space:pre-wrap'>aa   bb cccccccc</span>",
            10.0,
        );
        let texts: Vec<String> = l.texts("p").into_iter().map(|(t, _)| t).collect();
        assert_eq!(texts, ["aa   bb ", "cccccccc"], "{}", l.dump());
    }

    #[test]
    fn text_align_shifts_lines() {
        let l = mono_page(
            "<span style='display:block; text-align:center'>aaaa bbbb cccc</span>",
            10.0,
        );
        let t = l.texts("p");
        assert_eq!(t[0].1.x, mono(&l, 0.5), "{}", l.dump());
        assert_eq!(t[1].1.x, mono(&l, 3.0));

        let l = mono_page(
            "<span style='display:block; text-align:right'>aaaa bbbb cccc</span>",
            10.0,
        );
        let t = l.texts("p");
        assert_eq!(t[0].1.x, mono(&l, 1.0));
        assert_eq!(t[1].1.x, mono(&l, 6.0));

        let l = mono_page(
            "<span style='display:block; text-align:justify'>aa bb cc dddddd ee</span>",
            10.0,
        );
        let t = l.texts("p");
        // "aa bb cc" is 8 wide with 2 gaps; 2px of slack per gap.
        assert_eq!(t[0].1.width, mono(&l, 10.0), "{}", l.dump());
        assert_eq!(t[1].1.width, mono(&l, 9.0), "last line is not justified");
    }

    #[test]
    fn baselines_and_line_height_with_mixed_sizes() {
        let l = Laid::new(
            "<body style='margin:0; font-family:monospace; font-size:16px; line-height:20px'>
               <p id=p style='margin:0'>small <span id=big style='font-size:32px; line-height:40px'>BIG</span> small</p>
             </body>",
        );
        let lines = l.lines("p");
        assert_eq!(lines.len(), 1, "{}", l.dump());
        let line = lines[0].content;
        let texts = l.texts("p");
        let (small, big) = (texts[0].1, texts[1].1);
        // All three fragments share one baseline, so the big text starts
        // higher and is taller.
        let baselines: Vec<f32> = l
            .tree
            .pre_order(l.tree.root())
            .into_iter()
            .filter_map(|id| l.tree[id].text().map(|t| t.baseline))
            .collect();
        assert_eq!(baselines.len(), 3);
        assert!(
            baselines.iter().all(|&b| b == baselines[0]),
            "{baselines:?}"
        );
        assert!(big.y < small.y);
        assert!(big.height > small.height);
        // The line is at least as tall as the big span's line-height and
        // taller than the small one's.
        assert!(line.height >= 40.0 && line.height < 60.0, "{}", line.height);
        assert_eq!(l.fragments("big").len(), 1);
        assert_eq!(l.fragments("big")[0].content.width, big.width);
    }

    #[test]
    fn inline_fragments_carry_edges_only_at_their_ends() {
        let adv = Laid::new("").mono_advance(16.0);
        let l = Laid::new(&format!(
            "<body style='margin:0; font-family:monospace; font-size:16px; line-height:20px'>
               <p id=p style='margin:0; width:{}px'>aa <span id=s style='padding:0 {adv}px; border:1px solid; margin:0 {adv}px'>bbbb cccc</span> dd</p>
             </body>",
            12.0 * adv
        ));
        let frags = l.fragments("s");
        assert_eq!(frags.len(), 2, "{}", l.dump());
        let (first, last) = (frags[0], frags[1]);
        assert_eq!(first.margin.left, adv);
        assert_eq!(first.padding.left, adv);
        assert_eq!(first.border.left, 1.0);
        assert_eq!(first.margin.right, 0.0);
        assert_eq!(first.padding.right, 0.0);
        assert_eq!(first.border.right, 0.0);
        assert_eq!(last.margin.left, 0.0);
        assert_eq!(last.padding.left, 0.0);
        assert_eq!(last.margin.right, adv);
        assert_eq!(last.padding.right, adv);
        assert_eq!(last.border.right, 1.0);
        // "aa " (3 chars) + margin (1) + border (1px) + padding (1).
        assert_eq!(first.content.x, 5.0 * adv + 1.0);
        assert_eq!(first.content.width, 4.0 * adv);
        assert_eq!(last.content.x, 0.0);
        assert_eq!(last.content.width, 4.0 * adv);
        let texts: Vec<String> = l.texts("p").into_iter().map(|(t, _)| t).collect();
        // Spaces after "aa" and before "dd" sit between in-flow content on
        // their lines, so they are not trimmed as line-edge spaces.
        assert_eq!(texts, ["aa ", "bbbb", "cccc", " dd"]);
    }

    #[test]
    fn inline_block_shrinks_to_fit_and_sits_on_baseline() {
        let adv = Laid::new("").mono_advance(16.0);
        let l = Laid::new(&format!(
            "<body style='margin:0; font-family:monospace; font-size:16px; line-height:20px'>
               <p id=p style='margin:0; width:{}px'>x <span id=ib style='display:inline-block; padding:2px'>ab cd</span> y
               <span id=wide style='display:inline-block'>aaaa bbbb cccc dddd eeee ffff</span></p>
             </body>",
            20.0 * adv
        ));
        let ib = l.bx("ib");
        assert_eq!(
            ib.content.width,
            5.0 * adv,
            "max-content fits: {}",
            l.dump()
        );
        assert_eq!(ib.content.height, 20.0);
        assert_eq!(ib.content.x, 2.0 * adv + 2.0);
        // Its single line's baseline aligns with the paragraph's.
        let texts = l.texts("p");
        let x_frag = &texts[0];
        let inner = texts.iter().find(|(t, _)| t == "ab cd").unwrap();
        assert_eq!(inner.1.y, x_frag.1.y);
        // Wider than the line: shrinks to the available width and wraps.
        let wide = l.bx("wide");
        assert!(wide.content.width <= 20.0 * adv);
        assert_eq!(l.lines("wide").len(), 2);
        // First line of p holds "x", ib, "y"; second holds the wide atom.
        assert_eq!(l.lines("p").len(), 2);
    }

    #[test]
    fn empty_inline_block_sits_on_its_bottom_edge() {
        let l = Laid::new(
            "<body style='margin:0; line-height:20px'>
               <p id=p style='margin:0'>x<span id=ib style='display:inline-block; width:10px; height:50px'></span></p>
             </body>",
        );
        let ib = l.bx("ib");
        let line = l.lines("p")[0];
        assert_eq!(ib.content.y, line.content.y, "{}", l.dump());
        // The 50px box pushes the baseline down; the line is taller than
        // the strut alone.
        assert!(line.content.height > 50.0);
        let text_y = l.texts("p")[0].1.y;
        assert!(text_y > 30.0, "text sits near the bottom: {text_y}");
    }

    #[test]
    fn list_markers_align_with_the_first_line() {
        let l = Laid::new(
            "<body style='margin:0; line-height:20px'>
               <ul style='margin:0'><li id=a>one</li><li id=b><p style='margin:0'>para</p></li></ul>
               <ol style='margin:0'><li id=c>three</li></ol>
             </body>",
        );
        let a = l.bx("a");
        let marker = &l.tree[a.children[0]];
        let BoxKind::ListMarker(m) = &marker.kind else {
            panic!("{}", l.dump());
        };
        assert_eq!(m.text, "\u{2022} ");
        assert_eq!(marker.content.right(), a.content.x);
        let first_text = l.texts("a")[0].1;
        assert_eq!(marker.content.y, first_text.y);
        // A list item whose first line is inside a child block still
        // aligns its marker with that line.
        let b = l.bx("b");
        let marker_b = &l.tree[b.children[0]];
        assert_eq!(marker_b.content.y, l.texts("b")[0].1.y);
        let c = l.bx("c");
        assert_eq!(l.tree[c.children[0]].text().unwrap().text, "1. ");
    }

    #[test]
    fn whitespace_only_blocks_have_no_lines() {
        let l = Laid::new("<body style='margin:0'><div id=a> \n </div><div id=b>x</div></body>");
        assert_eq!(l.bx("a").content.height, 0.0);
        assert!(l.lines("a").is_empty());
        assert_eq!(l.bx("b").content.y, 0.0);
    }

    #[test]
    fn cross_node_spaces_collapse() {
        let l = mono_page("a <b id=b> b </b> c", 20.0);
        let texts: Vec<String> = l.texts("p").into_iter().map(|(t, _)| t).collect();
        assert_eq!(texts, ["a ", "b ", "c"], "{}", l.dump());
        assert_eq!(l.texts("p")[2].1.x, mono(&l, 4.0));
    }

    #[test]
    fn anonymous_blocks_lay_out_inline_runs() {
        let l = Laid::new(
            "<body style='margin:0; line-height:20px'>
               <div id=d>text<p id=p style='margin:0; height:30px'></p>more</div>
             </body>",
        );
        let d = l.bx("d");
        assert_eq!(d.children.len(), 3, "{}", l.dump());
        assert_eq!(l.tree[d.children[0]].kind, BoxKind::AnonymousBlock);
        assert_eq!(l.tree[d.children[0]].content.height, 20.0);
        assert_eq!(l.bx("p").content.y, 20.0);
        assert_eq!(l.tree[d.children[2]].content.y, 50.0);
        assert_eq!(d.content.height, 70.0);
    }

    #[test]
    fn dump_format() {
        let l =
            Laid::new("<body style='margin:0'><p id=x class='a b' style='margin:0'>Hi</p></body>");
        let dump = l.dump();
        assert!(
            dump.starts_with("Block <html> at (0,0) size 1024x"),
            "{dump}"
        );
        assert!(
            dump.contains("\n  Block <body> at (0,0) size 1024x"),
            "{dump}"
        );
        assert!(
            dump.contains("\n    Block <p#x.a.b> at (0,0) size 1024x"),
            "{dump}"
        );
        assert!(dump.contains("\n      Line at (0,0) size 1024x"), "{dump}");
        assert!(dump.contains("\n        Text \"Hi\" at (0,"), "{dump}");
        assert!(dump.contains(" baseline="), "{dump}");
        let plain = l.tree.dump(None);
        assert!(plain.starts_with("Block at (0,0)"), "{plain}");
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        for html in [
            "",
            "<html>",
            "<div style='width:-5px; height:-5px; padding:-1px; margin:1e30px'>x</div>",
            "<div style='font-size:0'>zero</div>",
            "<div style='width:0'>a b c</div>",
            "<div style='line-height:0'>a<br>b</div>",
            "<span style='display:inline-block; width:50%'><div style='display:inline-block'>x</div></span>",
            "<p>\u{200B}\u{FEFF}\0</p>",
            "<pre>\t\ta\n\n\n</pre>",
            "<ul><li><li><ul><li></ul></ul>",
        ] {
            let l = Laid::new(html);
            for (_, b) in l.tree.iter() {
                let r = b.content;
                assert!(
                    r.x.is_finite()
                        && r.y.is_finite()
                        && r.width.is_finite()
                        && r.height.is_finite(),
                    "{html}: {r:?}"
                );
                assert!(r.width >= 0.0 && r.height >= 0.0, "{html}: {r:?}");
            }
            assert!(l.tree.document_height().is_finite());
        }
        // Deep nesting of every box kind must not overflow the stack.
        let deep = Laid::new(&"<div>".repeat(5000));
        assert!(deep.tree.len() > 1000);
        // Beyond the depth limit boxes are flattened, but the text is
        // still there.
        let deep_inline = Laid::new(&("<span>".repeat(3000) + "<i>x</i>"));
        let xs = deep_inline
            .tree
            .iter()
            .filter(|(_, b)| b.text().is_some_and(|t| t.text == "x"))
            .count();
        assert_eq!(xs, 1);
        let _ = Laid::new(&"<div style='display:inline-block'>".repeat(400));
        let _ = Laid::new(&"<ul><li>".repeat(400));
    }
}
