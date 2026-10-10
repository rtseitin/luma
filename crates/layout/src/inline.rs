//! Inline layout: line boxes, line breaking, and vertical alignment
//! ([CSS 2.1 §9.4.2][ifc], [§10.8][lh]; whitespace and wrapping per
//! [CSS Text 3 §4][ws]).
//!
//! [ifc]: https://www.w3.org/TR/CSS21/visuren.html#inline-formatting
//! [lh]: https://www.w3.org/TR/CSS21/visudet.html#line-height
//! [ws]: https://www.w3.org/TR/css-text-3/#white-space-processing
//!
//! The inline content of a block container is first flattened into a
//! sequence of [`Item`]s: words, collapsible spaces, open/close markers
//! for inline boxes, atomic inline-blocks, and forced breaks. Items are
//! grouped into unbreakable chunks at soft wrap opportunities and placed
//! greedily on lines; then each line is built into a `LineBox` with one
//! fragment per text node, inline box, and inline-block on that line.
//!
//! Only `vertical-align: baseline` exists: every fragment's baseline sits
//! on the line's baseline, and inline-blocks use the baseline of their
//! last line box (or their bottom margin edge if they have none).

use std::sync::Arc;

use luma_style::{ComputedStyle, Edges, LengthPct, TextAlign, WhiteSpace};
use luma_text::{FontContext, FontId, Glyph, ShapedRun};

use crate::block::{AtomOut, Ctx};
use crate::box_tree::{BoxIdx, Kind, anonymous_style};
use crate::geometry::{Rect, border_edges, sanitize};
use crate::tree::{BoxKind, LayoutBox, LayoutBoxId, TextFragment};

/// Slack when testing whether a chunk fits, so that content measured to
/// exactly the available width is not wrapped by float noise.
const EPS: f32 = 1e-3;

/// Vertical metrics of an inline box's font plus its `line-height`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InlineMetrics {
    pub(crate) font: FontId,
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    /// Distance from the baseline to the top of the inline box: ascent
    /// plus half-leading.
    pub(crate) above: f32,
    /// Distance from the baseline to the bottom of the inline box.
    pub(crate) below: f32,
}

/// Select the font for `style` and compute its inline box metrics.
pub(crate) fn inline_metrics(fonts: &FontContext, style: &ComputedStyle) -> InlineMetrics {
    let font = fonts.select(&style.font_family, style.font_weight, style.font_style);
    let size = sanitize(style.font_size).max(0.0);
    let m = fonts.metrics(font, size);
    let ascent = sanitize(m.ascent);
    let descent = sanitize(m.descent);
    let line_height = sanitize(style.line_height_px());
    let half_leading = (line_height - (ascent + descent)) / 2.0;
    InlineMetrics {
        font,
        ascent,
        descent,
        above: ascent + half_leading,
        below: descent + half_leading,
    }
}

/// One element of flattened inline content.
#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub(crate) kind: ItemKind,
    /// Horizontal space the item takes: a run's advance, an inline box's
    /// left or right edges, or an inline-block's margin-box width.
    pub(crate) width: f32,
    /// A soft wrap opportunity exists immediately before this item.
    pub(crate) break_before: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ItemKind {
    /// A word, a preserved run, or (when `collapsible`) a single
    /// collapsible space that disappears at a line edge.
    Text {
        bx: BoxIdx,
        text: String,
        run: ShapedRun,
        collapsible: bool,
    },
    /// Start of an inline box.
    Open(BoxIdx),
    /// End of an inline box.
    Close(BoxIdx),
    /// An inline-block.
    Atom(BoxIdx),
    /// A forced line break.
    Break,
}

impl Item {
    fn is_break(&self) -> bool {
        matches!(self.kind, ItemKind::Break)
    }

    fn is_collapsible_space(&self) -> bool {
        matches!(
            self.kind,
            ItemKind::Text {
                collapsible: true,
                ..
            }
        )
    }

    fn is_marker(&self) -> bool {
        matches!(self.kind, ItemKind::Open(_) | ItemKind::Close(_))
    }

    /// Whether the item makes a line non-empty (§9.4.2: text, atoms, or
    /// inline boxes with non-zero edges do; collapsible spaces don't).
    fn has_content(&self) -> bool {
        match self.kind {
            ItemKind::Text { collapsible, .. } => !collapsible,
            ItemKind::Atom(_) => true,
            ItemKind::Open(_) | ItemKind::Close(_) => self.width > 0.0,
            ItemKind::Break => false,
        }
    }
}

fn allows_wrap(ws: WhiteSpace) -> bool {
    !matches!(ws, WhiteSpace::Nowrap | WhiteSpace::Pre)
}

fn resolve_opt(v: LengthPct, cb: Option<f32>) -> f32 {
    match (v, cb) {
        (LengthPct::Px(px), _) => sanitize(px),
        (LengthPct::Percent(_), Some(cb)) => sanitize(v.resolve(cb)),
        (LengthPct::Percent(_), None) => 0.0,
    }
}

/// Padding, border, and horizontal margins of an inline box. Vertical
/// margins of inlines have no effect and are dropped. With no containing
/// block width (intrinsic sizing), percentages count as zero.
pub(crate) fn inline_edges(
    style: &ComputedStyle,
    cb: Option<f32>,
) -> (Edges<f32>, Edges<f32>, f32, f32) {
    let p = &style.padding;
    let padding = Edges {
        top: resolve_opt(p.top, cb).max(0.0),
        right: resolve_opt(p.right, cb).max(0.0),
        bottom: resolve_opt(p.bottom, cb).max(0.0),
        left: resolve_opt(p.left, cb).max(0.0),
    };
    let border = border_edges(&style.border_width);
    let margin = |s| match s {
        luma_style::Size::Auto => 0.0,
        luma_style::Size::Length(l) => resolve_opt(l, cb),
    };
    (
        padding,
        border,
        margin(style.margin.left),
        margin(style.margin.right),
    )
}

// ----- flattening ------------------------------------------------------------

struct Flattener<'c, 'a> {
    ctx: &'c mut Ctx<'a>,
    cb_width: Option<f32>,
    items: Vec<Item>,
    /// At the start of a line or right after a collapsible space, so the
    /// next collapsible space is dropped.
    at_start: bool,
    /// A soft wrap opportunity is waiting for the next item.
    break_pending: bool,
}

/// Flatten the inline content of block container `bx` into items. With
/// `cb_width` the inline boxes' percentage edges are resolved; without it
/// (intrinsic sizing) they are zero. Inline-block items get width 0; the
/// caller sizes them.
pub(crate) fn flatten(ctx: &mut Ctx<'_>, bx: BoxIdx, cb_width: Option<f32>) -> Vec<Item> {
    let mut f = Flattener {
        ctx,
        cb_width,
        items: Vec::new(),
        at_start: true,
        break_pending: false,
    };
    f.walk(bx);
    f.items
}

impl Flattener<'_, '_> {
    fn walk(&mut self, bx: BoxIdx) {
        let bt = self.ctx.bt;
        for &child in &bt.get(bx).children {
            let node = bt.get(child);
            match &node.kind {
                Kind::Text(text) => self.text(child, text, &node.style),
                Kind::Break => self.forced_break(),
                Kind::Inline => {
                    let (p, b, ml, mr) = inline_edges(&node.style, self.cb_width);
                    let wrap = allows_wrap(node.style.white_space);
                    let break_before = std::mem::take(&mut self.break_pending) && wrap;
                    self.items.push(Item {
                        kind: ItemKind::Open(child),
                        width: ml + b.left + p.left,
                        break_before,
                    });
                    self.walk(child);
                    self.items.push(Item {
                        kind: ItemKind::Close(child),
                        width: p.right + b.right + mr,
                        break_before: false,
                    });
                }
                Kind::InlineBlock => {
                    let wrap = allows_wrap(node.style.white_space);
                    let pending = std::mem::take(&mut self.break_pending);
                    self.items.push(Item {
                        kind: ItemKind::Atom(child),
                        width: 0.0,
                        break_before: (pending || !self.at_start) && wrap,
                    });
                    self.at_start = false;
                    self.break_pending = wrap;
                }
                // The box tree never puts block-level boxes in inline
                // content.
                Kind::Block | Kind::AnonymousBlock => {}
            }
        }
    }

    fn text(&mut self, bx: BoxIdx, text: &str, style: &Arc<ComputedStyle>) {
        let font = self.ctx.metrics_for(style).font;
        let size = style.font_size;
        let ws = style.white_space;
        let wrap = allows_wrap(ws);
        match ws {
            WhiteSpace::Normal | WhiteSpace::Nowrap | WhiteSpace::PreLine => {
                let mut word = String::new();
                for c in text.chars() {
                    match c {
                        ' ' => {
                            self.flush_word(bx, &mut word, font, size, wrap);
                            self.space(bx, font, size, wrap);
                        }
                        '\n' => {
                            self.flush_word(bx, &mut word, font, size, wrap);
                            self.forced_break();
                        }
                        c => word.push(c),
                    }
                }
                self.flush_word(bx, &mut word, font, size, wrap);
            }
            WhiteSpace::Pre => {
                for (i, segment) in text.split('\n').enumerate() {
                    if i > 0 {
                        self.forced_break();
                    }
                    if !segment.is_empty() {
                        self.push_text(bx, segment, font, size, false, false);
                        self.at_start = false;
                        self.break_pending = false;
                    }
                }
            }
            WhiteSpace::PreWrap => {
                for (i, segment) in text.split('\n').enumerate() {
                    if i > 0 {
                        self.forced_break();
                    }
                    let mut token = String::new();
                    let mut token_is_space = false;
                    for c in segment.chars() {
                        let is_space = c == ' ';
                        if !token.is_empty() && is_space != token_is_space {
                            self.pre_wrap_token(bx, &mut token, token_is_space, font, size);
                        }
                        token_is_space = is_space;
                        token.push(c);
                    }
                    if !token.is_empty() {
                        self.pre_wrap_token(bx, &mut token, token_is_space, font, size);
                    }
                }
            }
        }
    }

    /// A `pre-wrap` token: preserved spaces are kept (and allow a break
    /// after them); words break before them when a space preceded.
    fn pre_wrap_token(
        &mut self,
        bx: BoxIdx,
        token: &mut String,
        is_space: bool,
        font: FontId,
        size: f32,
    ) {
        let text = std::mem::take(token);
        if is_space {
            self.push_text(bx, &text, font, size, false, false);
            self.break_pending = true;
        } else {
            let break_before = std::mem::take(&mut self.break_pending);
            self.push_text(bx, &text, font, size, false, break_before);
        }
        self.at_start = false;
    }

    fn flush_word(&mut self, bx: BoxIdx, word: &mut String, font: FontId, size: f32, wrap: bool) {
        if word.is_empty() {
            return;
        }
        let text = std::mem::take(word);
        let break_before = std::mem::take(&mut self.break_pending) && wrap;
        self.push_text(bx, &text, font, size, false, break_before);
        self.at_start = false;
    }

    fn space(&mut self, bx: BoxIdx, font: FontId, size: f32, wrap: bool) {
        if self.at_start {
            return;
        }
        self.push_text(bx, " ", font, size, true, false);
        self.at_start = true;
        self.break_pending = wrap;
    }

    fn push_text(
        &mut self,
        bx: BoxIdx,
        text: &str,
        font: FontId,
        size: f32,
        collapsible: bool,
        break_before: bool,
    ) {
        let run = self.ctx.fonts.shape(font, size, text);
        self.items.push(Item {
            width: sanitize(run.advance).max(0.0),
            kind: ItemKind::Text {
                bx,
                text: text.to_owned(),
                run,
                collapsible,
            },
            break_before,
        });
    }

    fn forced_break(&mut self) {
        self.items.push(Item {
            kind: ItemKind::Break,
            width: 0.0,
            break_before: false,
        });
        self.at_start = true;
        self.break_pending = false;
    }
}

// ----- line breaking ---------------------------------------------------------

/// A line: the items `start..end`, and whether it ended with a forced
/// break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) forced: bool,
}

/// Width of an unbreakable chunk for fitting purposes: trailing
/// collapsible spaces don't count because they vanish at a line end.
fn chunk_fit_width(chunk: &[Item]) -> f32 {
    let mut total: f32 = chunk.iter().map(|i| i.width).sum();
    for item in chunk.iter().rev() {
        if item.is_collapsible_space() {
            total -= item.width;
        } else if !item.is_marker() {
            break;
        }
    }
    total
}

/// Greedy line breaking (CSS Text 3 §5): items are grouped into chunks at
/// soft wrap opportunities; a chunk that doesn't fit starts a new line
/// unless the line is still empty, in which case it overflows.
pub(crate) fn break_lines(items: &[Item], available: f32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0.0f32;
    let mut i = 0;
    while i < items.len() {
        if items[i].is_break() {
            lines.push(Line {
                start,
                end: i,
                forced: true,
            });
            start = i + 1;
            used = 0.0;
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < items.len() && !items[j].break_before && !items[j].is_break() {
            j += 1;
        }
        let chunk = &items[i..j];
        let line_has_content = items[start..i].iter().any(Item::has_content);
        if items[i].break_before
            && line_has_content
            && used + chunk_fit_width(chunk) > available + EPS
        {
            lines.push(Line {
                start,
                end: i,
                forced: false,
            });
            start = i;
            used = 0.0;
        }
        used += chunk.iter().map(|it| it.width).sum::<f32>();
        i = j;
    }
    if start < items.len() && items[start..].iter().any(Item::has_content) {
        lines.push(Line {
            start,
            end: items.len(),
            forced: false,
        });
    }
    lines
}

// ----- line placement --------------------------------------------------------

/// Result of laying out a block container's inline content.
pub(crate) struct InlineOut {
    pub(crate) height: f32,
    pub(crate) line_count: usize,
    /// Baselines of the first and last line, relative to the content top.
    pub(crate) first_baseline: Option<f32>,
    pub(crate) last_baseline: Option<f32>,
}

/// Lay out the inline content of `bx` into line boxes appended to
/// `container`, whose content box is `width` wide.
pub(crate) fn layout_inline_content(
    ctx: &mut Ctx<'_>,
    bx: BoxIdx,
    container: LayoutBoxId,
    width: f32,
    cb_height: Option<f32>,
) -> InlineOut {
    let container_style = ctx.bt.get(bx).style.clone();
    let anon_style = Arc::new(anonymous_style(&container_style));
    let mut items = flatten(ctx, bx, Some(width));

    let mut atoms: Vec<Option<AtomOut>> = Vec::with_capacity(items.len());
    for item in &mut items {
        let atom = if let ItemKind::Atom(b) = item.kind {
            let out = ctx.layout_inline_block(b, width, cb_height);
            item.width = out.margin_width;
            Some(out)
        } else {
            None
        };
        atoms.push(atom);
    }

    let strut = ctx.metrics_for(&container_style);
    let lines = break_lines(&items, width);
    let mut placer = Placer {
        ctx,
        items: &items,
        atoms: &atoms,
        width,
        align: container_style.text_align,
        anon_style,
        container,
        strut,
        open: Vec::new(),
        stack: Vec::new(),
    };

    let mut y = 0.0f32;
    let mut first_baseline = None;
    let mut last_baseline = None;
    for (n, line) in lines.iter().enumerate() {
        let (height, baseline) = placer.place_line(line, y, n + 1 == lines.len());
        first_baseline.get_or_insert(y + baseline);
        last_baseline = Some(y + baseline);
        y = sanitize(y + height);
    }
    InlineOut {
        height: y.max(0.0),
        line_count: lines.len(),
        first_baseline,
        last_baseline,
    }
}

/// An inline box fragment that is open on the current line. Coordinates
/// are line-local (relative to the line box's content origin).
struct Frame {
    bx: BoxIdx,
    id: LayoutBoxId,
    content_x: f32,
    content_y: f32,
}

/// Text from one text node accumulating into a single fragment.
struct PendingText {
    bx: BoxIdx,
    parent: LayoutBoxId,
    origin: (f32, f32),
    start_x: f32,
    text: String,
    glyphs: Vec<Glyph>,
    advance: f32,
}

struct Placer<'c, 'a> {
    ctx: &'c mut Ctx<'a>,
    items: &'c [Item],
    atoms: &'c [Option<AtomOut>],
    width: f32,
    align: TextAlign,
    anon_style: Arc<ComputedStyle>,
    container: LayoutBoxId,
    strut: InlineMetrics,
    /// Inline boxes still open at the end of the previous line, outermost
    /// first.
    open: Vec<BoxIdx>,
    stack: Vec<Frame>,
}

impl Placer<'_, '_> {
    /// The box new fragments are appended to and its line-local content
    /// origin.
    fn parent(&self, line_id: LayoutBoxId) -> (LayoutBoxId, (f32, f32)) {
        match self.stack.last() {
            Some(f) => (f.id, (f.content_x, f.content_y)),
            None => (line_id, (0.0, 0.0)),
        }
    }

    /// Build one line box at `y`; returns its height and line-local
    /// baseline.
    fn place_line(&mut self, line: &Line, y: f32, is_last: bool) -> (f32, f32) {
        let items = &self.items[line.start..line.end];

        // Trailing collapsible spaces vanish (CSS Text §4.1.3 step 3).
        let mut dropped = vec![false; items.len()];
        for (k, item) in items.iter().enumerate().rev() {
            if item.is_collapsible_space() {
                dropped[k] = true;
            } else if !item.is_marker() {
                break;
            }
        }
        let widths: Vec<f32> = items
            .iter()
            .zip(&dropped)
            .map(|(it, &d)| if d { 0.0 } else { it.width })
            .collect();
        let total: f32 = widths.iter().sum();
        let spaces = items
            .iter()
            .zip(&dropped)
            .filter(|(it, d)| !**d && it.is_collapsible_space())
            .count();
        let free = self.width - total;
        let (mut x, extra) = match self.align {
            TextAlign::Center if free > 0.0 => (free / 2.0, 0.0),
            TextAlign::Right | TextAlign::End if free > 0.0 => (free, 0.0),
            TextAlign::Justify if free > 0.0 && spaces > 0 && !is_last && !line.forced => {
                (0.0, free / spaces as f32)
            }
            _ => (0.0, 0.0),
        };

        // Vertical: the strut, every open inline box, and every item.
        let mut above = self.strut.above;
        let mut below = self.strut.below;
        for &b in &self.open {
            let m = self.ctx.metrics_for(&self.ctx.bt.get(b).style.clone());
            above = above.max(m.above);
            below = below.max(m.below);
        }
        for (k, item) in items.iter().enumerate() {
            if dropped[k] {
                continue;
            }
            match item.kind {
                ItemKind::Text { bx, .. } | ItemKind::Open(bx) => {
                    let m = self.ctx.metrics_for(&self.ctx.bt.get(bx).style.clone());
                    above = above.max(m.above);
                    below = below.max(m.below);
                }
                ItemKind::Atom(_) => {
                    if let Some(a) = &self.atoms[line.start + k] {
                        let up = a.baseline.unwrap_or(a.margin_height);
                        above = above.max(up);
                        below = below.max(a.margin_height - up);
                    }
                }
                ItemKind::Close(_) | ItemKind::Break => {}
            }
        }
        let line_height = sanitize(above + below).max(0.0);
        let baseline = above;

        let line_id = self.ctx.push(LayoutBox {
            kind: BoxKind::LineBox,
            node: None,
            style: self.anon_style.clone(),
            content: Rect::new(0.0, y, self.width, line_height),
            padding: Edges::all(0.0),
            border: Edges::all(0.0),
            margin: Edges::all(0.0),
            children: Vec::new(),
        });
        self.ctx.get_mut(self.container).children.push(line_id);

        // Reopen inline boxes continued from the previous line.
        self.stack.clear();
        for b in std::mem::take(&mut self.open) {
            self.open_fragment(b, &mut x, baseline, false, line_id);
        }

        let mut pending: Option<PendingText> = None;
        for (k, item) in items.iter().enumerate() {
            if dropped[k] {
                continue;
            }
            match &item.kind {
                ItemKind::Text {
                    bx,
                    text,
                    run,
                    collapsible,
                } => {
                    let w = widths[k] + if *collapsible { extra } else { 0.0 };
                    if pending.as_ref().is_some_and(|p| p.bx != *bx) {
                        self.flush_text(pending.take(), baseline);
                    }
                    let (parent, origin) = self.parent(line_id);
                    let p = pending.get_or_insert_with(|| PendingText {
                        bx: *bx,
                        parent,
                        origin,
                        start_x: x,
                        text: String::new(),
                        glyphs: Vec::new(),
                        advance: 0.0,
                    });
                    let offset = p.text.len();
                    p.text.push_str(text);
                    p.glyphs.extend(run.glyphs.iter().map(|g| Glyph {
                        cluster: g.cluster + offset,
                        ..*g
                    }));
                    if *collapsible
                        && extra > 0.0
                        && let Some(last) = p.glyphs.last_mut()
                    {
                        last.advance += extra;
                    }
                    p.advance += w;
                    x += w;
                }
                ItemKind::Open(b) => {
                    self.flush_text(pending.take(), baseline);
                    self.open_fragment(*b, &mut x, baseline, true, line_id);
                }
                ItemKind::Close(b) => {
                    self.flush_text(pending.take(), baseline);
                    if let Some(frame) = self.stack.pop() {
                        debug_assert_eq!(frame.bx, *b, "open/close markers nest");
                        self.close_fragment(frame, &mut x, true);
                    }
                }
                ItemKind::Atom(_) => {
                    self.flush_text(pending.take(), baseline);
                    if let Some(a) = &self.atoms[line.start + k] {
                        let (parent, origin) = self.parent(line_id);
                        let up = a.baseline.unwrap_or(a.margin_height);
                        let b = self.ctx.get_mut(a.id);
                        let left = b.margin.left + b.border.left + b.padding.left;
                        let top = b.margin.top + b.border.top + b.padding.top;
                        b.content.x = x + left - origin.0;
                        b.content.y = baseline - up + top - origin.1;
                        self.ctx.get_mut(parent).children.push(a.id);
                        x += a.margin_width;
                    }
                }
                ItemKind::Break => {}
            }
        }
        self.flush_text(pending.take(), baseline);

        // Inline boxes still open continue on the next line.
        self.open = self.stack.iter().map(|f| f.bx).collect();
        while let Some(frame) = self.stack.pop() {
            self.close_fragment(frame, &mut x, false);
        }
        (line_height, baseline)
    }

    /// Start an `Inline` fragment for box `b` at `x`. `first` fragments
    /// carry the box's left margin, border, and padding.
    fn open_fragment(
        &mut self,
        b: BoxIdx,
        x: &mut f32,
        baseline: f32,
        first: bool,
        line_id: LayoutBoxId,
    ) {
        let node = self.ctx.bt.get(b);
        let style = node.style.clone();
        let (p, bd, ml, _) = inline_edges(&style, Some(self.width));
        let m = self.ctx.metrics_for(&style);
        let (parent, origin) = self.parent(line_id);
        let left = if first { ml } else { 0.0 };
        let content_x = *x + left + if first { bd.left + p.left } else { 0.0 };
        let content_y = baseline - m.ascent;
        let id = self.ctx.push(LayoutBox {
            kind: BoxKind::Inline,
            node: node.node,
            style,
            content: Rect::new(
                content_x - origin.0,
                content_y - origin.1,
                0.0,
                m.ascent + m.descent,
            ),
            padding: Edges {
                top: p.top,
                right: 0.0,
                bottom: p.bottom,
                left: if first { p.left } else { 0.0 },
            },
            border: Edges {
                top: bd.top,
                right: 0.0,
                bottom: bd.bottom,
                left: if first { bd.left } else { 0.0 },
            },
            margin: Edges {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left,
            },
            children: Vec::new(),
        });
        self.ctx.get_mut(parent).children.push(id);
        self.stack.push(Frame {
            bx: b,
            id,
            content_x,
            content_y,
        });
        *x = content_x;
    }

    /// Finish a fragment at `x`. `last` fragments carry the right edges.
    fn close_fragment(&mut self, frame: Frame, x: &mut f32, last: bool) {
        let width = (*x - frame.content_x).max(0.0);
        let style = self.ctx.bt.get(frame.bx).style.clone();
        let b = self.ctx.get_mut(frame.id);
        b.content.width = width;
        if last {
            let (p, bd, _, mr) = inline_edges(&style, Some(self.width));
            b.padding.right = p.right;
            b.border.right = bd.right;
            b.margin.right = mr;
            *x += p.right + bd.right + mr;
        }
    }

    /// Emit the accumulated text of one text node as a `Text` fragment.
    fn flush_text(&mut self, pending: Option<PendingText>, baseline: f32) {
        let Some(p) = pending else {
            return;
        };
        let node = self.ctx.bt.get(p.bx);
        let style = node.style.clone();
        let m = self.ctx.metrics_for(&style);
        let id = self.ctx.push(LayoutBox {
            kind: BoxKind::Text(TextFragment {
                text: p.text,
                run: ShapedRun {
                    glyphs: p.glyphs,
                    advance: p.advance,
                },
                font: m.font,
                baseline: baseline - p.origin.1,
            }),
            node: node.node,
            style,
            content: Rect::new(
                p.start_x - p.origin.0,
                baseline - m.ascent - p.origin.1,
                p.advance,
                m.ascent + m.descent,
            ),
            padding: Edges::all(0.0),
            border: Edges::all(0.0),
            margin: Edges::all(0.0),
            children: Vec::new(),
        });
        self.ctx.get_mut(p.parent).children.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: f32, break_before: bool) -> Item {
        Item {
            kind: ItemKind::Text {
                bx: 0,
                text: "w".into(),
                run: ShapedRun::default(),
                collapsible: false,
            },
            width: w,
            break_before,
        }
    }

    fn space(w: f32) -> Item {
        Item {
            kind: ItemKind::Text {
                bx: 0,
                text: " ".into(),
                run: ShapedRun::default(),
                collapsible: true,
            },
            width: w,
            break_before: false,
        }
    }

    fn br() -> Item {
        Item {
            kind: ItemKind::Break,
            width: 0.0,
            break_before: false,
        }
    }

    fn ranges(lines: &[Line]) -> Vec<(usize, usize, bool)> {
        lines.iter().map(|l| (l.start, l.end, l.forced)).collect()
    }

    #[test]
    fn greedy_breaking_ignores_trailing_space() {
        // "aaaa bbbb cccc" at 10px per char in 100px.
        let items = [
            word(40.0, false),
            space(10.0),
            word(40.0, true),
            space(10.0),
            word(40.0, true),
        ];
        assert_eq!(
            ranges(&break_lines(&items, 100.0)),
            [(0, 4, false), (4, 5, false)]
        );
        // Exactly fits: "aaaa bbbb" is 90 wide, "cccc" makes 140.
        assert_eq!(
            ranges(&break_lines(&items, 90.0)),
            [(0, 4, false), (4, 5, false)]
        );
        // Everything on one line when wide enough.
        assert_eq!(ranges(&break_lines(&items, 140.0)), [(0, 5, false)]);
    }

    #[test]
    fn overlong_word_overflows_alone() {
        let items = [
            word(200.0, false),
            space(10.0),
            word(30.0, true),
            space(10.0),
            word(300.0, true),
        ];
        assert_eq!(
            ranges(&break_lines(&items, 100.0)),
            [(0, 2, false), (2, 4, false), (4, 5, false)]
        );
        // Zero width: one chunk per line, never an infinite loop.
        assert_eq!(break_lines(&items, 0.0).len(), 3);
    }

    #[test]
    fn forced_breaks_make_empty_lines_but_trailing_break_does_not() {
        let items = [word(10.0, false), br(), br(), word(10.0, false), br()];
        assert_eq!(
            ranges(&break_lines(&items, 100.0)),
            [(0, 1, true), (2, 2, true), (3, 4, true)]
        );
        assert_eq!(ranges(&break_lines(&[br()], 100.0)), [(0, 0, true)]);
    }

    #[test]
    fn space_only_content_makes_no_line() {
        assert!(break_lines(&[space(10.0)], 100.0).is_empty());
        assert!(break_lines(&[], 100.0).is_empty());
    }

    #[test]
    fn nowrap_items_never_break() {
        let items = [word(80.0, false), space(10.0), word(80.0, false)];
        assert_eq!(ranges(&break_lines(&items, 100.0)), [(0, 3, false)]);
    }
}
