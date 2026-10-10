//! The cascade: collect matching declarations, order them, compute values.

use std::sync::{Arc, OnceLock};

use luma_css::{
    ColorValue, Length, LengthPercentage, LengthUnit, MediaFeature, MediaQuery, MediaType, Range,
    Rule, Selector, Specificity, Stylesheet,
};
use luma_dom::{Document, NodeId};

use crate::computed::{
    BorderStyle, ComputedStyle, DEFAULT_FONT_SIZE, LengthPct, LineHeight, MaxSize, Size,
};
use crate::matching::matches_selector;
use crate::properties::{
    FontSizeSpec, FontWeightSpec, LineHeightSpec, Longhand, PropertyDeclaration, SpecMaxSize,
    SpecSize, Value, parse_declaration,
};

/// The viewport, used for `vw`/`vh` units and media queries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Width in CSS pixels.
    pub width: f32,
    /// Height in CSS pixels.
    pub height: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1024.0,
            height: 768.0,
        }
    }
}

/// Computed styles for every node in a document, indexed by [`NodeId`].
///
/// Element nodes get their own style. Text nodes share their parent's
/// style (text inherits everything it needs). Other nodes have none.
#[derive(Debug, Clone)]
pub struct StyleMap {
    styles: Vec<Option<Arc<ComputedStyle>>>,
}

impl StyleMap {
    /// The computed style of a node, if it has one.
    pub fn get(&self, id: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(id.index()).and_then(|s| s.as_deref())
    }

    /// A shared handle to the computed style of a node, if it has one.
    /// Cheap to clone; lets later stages keep styles alive without
    /// borrowing the map.
    pub fn get_arc(&self, id: NodeId) -> Option<Arc<ComputedStyle>> {
        self.styles.get(id.index()).and_then(Clone::clone)
    }

    /// Number of slots (equals the document's node count).
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    UserAgent,
    Author,
}

/// A flattened rule: media queries already evaluated, declarations already
/// parsed into longhands.
struct RuleEntry {
    selectors: Vec<Selector>,
    decls: Vec<(PropertyDeclaration, bool)>,
    origin: Origin,
    order: u32,
}

struct Matched<'a> {
    /// Cascade precedence: higher wins. UA normal < author normal <
    /// author !important < UA !important.
    precedence: u8,
    /// The `style` attribute beats any selector of the same precedence.
    is_style_attr: bool,
    specificity: Specificity,
    order: u32,
    decl: &'a PropertyDeclaration,
}

fn precedence(origin: Origin, important: bool) -> u8 {
    match (origin, important) {
        (Origin::UserAgent, false) => 0,
        (Origin::Author, false) => 1,
        (Origin::Author, true) => 2,
        (Origin::UserAgent, true) => 3,
    }
}

fn ua_stylesheet() -> &'static Stylesheet {
    static UA: OnceLock<Stylesheet> = OnceLock::new();
    UA.get_or_init(|| luma_css::parse_stylesheet(include_str!("ua.css")))
}

/// Stylesheets from `<style>` elements, in document order.
fn document_stylesheets(doc: &Document) -> Vec<Stylesheet> {
    doc.descendants(doc.root())
        .filter(|&id| doc.element(id).is_some_and(|e| e.name == "style"))
        .map(|id| luma_css::parse_stylesheet(&doc.text_content(id)))
        .collect()
}

fn flatten(
    rules: &[Rule],
    origin: Origin,
    viewport: Viewport,
    order: &mut u32,
    out: &mut Vec<RuleEntry>,
) {
    for rule in rules {
        match rule {
            Rule::Style(sr) => {
                let decls: Vec<(PropertyDeclaration, bool)> = sr
                    .declarations
                    .iter()
                    .flat_map(|d| {
                        parse_declaration(d)
                            .into_iter()
                            .map(move |pd| (pd, d.important))
                    })
                    .collect();
                if !decls.is_empty() {
                    out.push(RuleEntry {
                        selectors: sr.selectors.clone(),
                        decls,
                        origin,
                        order: *order,
                    });
                }
                *order += 1;
            }
            Rule::Media(mr) => {
                if mr.queries.iter().any(|q| media_matches(q, viewport)) {
                    flatten(&mr.rules, origin, viewport, order, out);
                }
            }
        }
    }
}

/// Evaluate a media query. Luma is a screen UA.
pub fn media_matches(q: &MediaQuery, viewport: Viewport) -> bool {
    let type_ok = matches!(q.media_type, MediaType::All | MediaType::Screen);
    let features_ok = q.features.iter().all(|f| {
        let (range, len, actual) = match f {
            MediaFeature::Width(r, l) => (*r, *l, viewport.width),
            MediaFeature::Height(r, l) => (*r, *l, viewport.height),
            MediaFeature::Unknown => return false,
        };
        // Relative units in media queries use the initial font size.
        let ctx = LengthCtx {
            font_size: DEFAULT_FONT_SIZE,
            root_font_size: DEFAULT_FONT_SIZE,
            viewport,
        };
        let px = ctx.resolve(len);
        match range {
            Range::Min => actual >= px,
            Range::Max => actual <= px,
            Range::Exact => (actual - px).abs() < 0.5,
        }
    });
    (type_ok && features_ok) != q.negated
}

/// Compute styles for every element in `doc`.
///
/// `extra` are author stylesheets that apply before the document's own
/// `<style>` elements (e.g. external sheets loaded by the caller).
pub fn style_document(doc: &Document, extra: &[Stylesheet], viewport: Viewport) -> StyleMap {
    let mut entries = Vec::new();
    let mut order = 0;
    flatten(
        &ua_stylesheet().rules,
        Origin::UserAgent,
        viewport,
        &mut order,
        &mut entries,
    );
    for sheet in extra {
        flatten(
            &sheet.rules,
            Origin::Author,
            viewport,
            &mut order,
            &mut entries,
        );
    }
    for sheet in document_stylesheets(doc) {
        flatten(
            &sheet.rules,
            Origin::Author,
            viewport,
            &mut order,
            &mut entries,
        );
    }

    let initial = Arc::new(ComputedStyle::initial());
    let mut styles: Vec<Option<Arc<ComputedStyle>>> = vec![None; doc.node_count()];
    let mut root_font_size = DEFAULT_FONT_SIZE;

    // Pre-order: parents are always styled before their children.
    for id in doc.descendants(doc.root()) {
        let node = &doc[id];
        let parent_style = node
            .parent()
            .and_then(|p| styles[p.index()].clone())
            .unwrap_or_else(|| initial.clone());

        if node.is_text() {
            styles[id.index()] = Some(parent_style);
            continue;
        }
        let Some(el) = node.as_element() else {
            continue;
        };

        let is_root = node.parent() == Some(doc.root());
        let style_attr = el.attr("style").map(luma_css::parse_declaration_list);
        let style_attr_decls: Vec<PropertyDeclaration> = style_attr
            .iter()
            .flatten()
            .flat_map(|d| {
                // `!important` in a style attribute still wins over normal
                // author rules; model it by giving it author-important
                // precedence via a separate pass below.
                parse_declaration(d)
                    .into_iter()
                    .map(move |pd| (pd, d.important))
            })
            .filter(|(_, important)| !important)
            .map(|(pd, _)| pd)
            .collect();
        let style_attr_important: Vec<PropertyDeclaration> = style_attr
            .iter()
            .flatten()
            .filter(|d| d.important)
            .flat_map(parse_declaration)
            .collect();

        let mut matched: Vec<Matched> = Vec::new();
        for entry in &entries {
            for sel in &entry.selectors {
                if matches_selector(doc, id, sel) {
                    for (decl, important) in &entry.decls {
                        matched.push(Matched {
                            precedence: precedence(entry.origin, *important),
                            is_style_attr: false,
                            specificity: sel.specificity,
                            order: entry.order,
                            decl,
                        });
                    }
                }
            }
        }
        for (i, decl) in style_attr_decls.iter().enumerate() {
            matched.push(Matched {
                precedence: precedence(Origin::Author, false),
                is_style_attr: true,
                specificity: Specificity::default(),
                order: i as u32,
                decl,
            });
        }
        for (i, decl) in style_attr_important.iter().enumerate() {
            matched.push(Matched {
                precedence: precedence(Origin::Author, true),
                is_style_attr: true,
                specificity: Specificity::default(),
                order: i as u32,
                decl,
            });
        }

        matched.sort_by_key(|m| (m.precedence, m.is_style_attr, m.specificity, m.order));

        let mut specified: [Option<&Value>; Longhand::COUNT] = [None; Longhand::COUNT];
        for m in &matched {
            specified[m.decl.longhand.index()] = Some(&m.decl.value);
        }

        let computed = compute(&specified, &parent_style, is_root, root_font_size, viewport);
        if is_root {
            root_font_size = computed.font_size;
        }
        styles[id.index()] = Some(Arc::new(computed));
    }

    StyleMap { styles }
}

// ----- computation -------------------------------------------------------------

#[derive(Clone, Copy)]
struct LengthCtx {
    font_size: f32,
    root_font_size: f32,
    viewport: Viewport,
}

impl LengthCtx {
    fn resolve(&self, len: Length) -> f32 {
        if let Some(px) = len.absolute_px() {
            return px;
        }
        let v = len.value;
        match len.unit {
            LengthUnit::Em => v * self.font_size,
            LengthUnit::Rem => v * self.root_font_size,
            LengthUnit::Ex | LengthUnit::Ch => v * self.font_size * 0.5,
            LengthUnit::Vw => v * self.viewport.width / 100.0,
            LengthUnit::Vh => v * self.viewport.height / 100.0,
            LengthUnit::Vmin => v * self.viewport.width.min(self.viewport.height) / 100.0,
            LengthUnit::Vmax => v * self.viewport.width.max(self.viewport.height) / 100.0,
            // Absolute units were handled above.
            _ => v,
        }
    }

    fn resolve_lp(&self, lp: LengthPercentage) -> LengthPct {
        match lp {
            LengthPercentage::Length(l) => LengthPct::Px(self.resolve(l)),
            LengthPercentage::Percentage(p) => LengthPct::Percent(p),
        }
    }

    fn resolve_size(&self, s: SpecSize) -> Size {
        match s {
            SpecSize::Auto => Size::Auto,
            SpecSize::Length(lp) => Size::Length(self.resolve_lp(lp)),
        }
    }

    fn resolve_max(&self, s: SpecMaxSize) -> MaxSize {
        match s {
            SpecMaxSize::None => MaxSize::None,
            SpecMaxSize::Length(lp) => MaxSize::Length(self.resolve_lp(lp)),
        }
    }
}

fn compute(
    specified: &[Option<&Value>; Longhand::COUNT],
    parent: &ComputedStyle,
    is_root: bool,
    root_font_size: f32,
    viewport: Viewport,
) -> ComputedStyle {
    let initial = ComputedStyle::initial();
    let mut cs = ComputedStyle::inherit_from(parent);

    // Resolve the "source" for css-wide keywords: inherit copies from the
    // parent (or initial for the root), unset depends on the property.
    let source_for = |value: &Value, lh: Longhand| -> Option<&ComputedStyle> {
        match value {
            Value::Inherit => Some(if is_root { &initial } else { parent }),
            Value::Initial => Some(&initial),
            Value::Unset => Some(if lh.is_inherited() && !is_root {
                parent
            } else {
                &initial
            }),
            _ => None,
        }
    };

    // Font size first: every em below depends on it. Its own em/% are
    // relative to the parent's font size.
    let parent_ctx = LengthCtx {
        font_size: parent.font_size,
        root_font_size: if is_root {
            DEFAULT_FONT_SIZE
        } else {
            root_font_size
        },
        viewport,
    };
    if let Some(v) = specified[Longhand::FontSize.index()] {
        cs.font_size = match v {
            Value::FontSize(spec) => match spec {
                FontSizeSpec::Length(LengthPercentage::Length(l)) => parent_ctx.resolve(*l),
                FontSizeSpec::Length(LengthPercentage::Percentage(p)) => {
                    parent.font_size * p / 100.0
                }
                FontSizeSpec::Absolute(px) => *px,
                FontSizeSpec::Smaller => parent.font_size / 1.2,
                FontSizeSpec::Larger => parent.font_size * 1.2,
            },
            other => source_for(other, Longhand::FontSize).map_or(cs.font_size, |s| s.font_size),
        };
    }

    let ctx = LengthCtx {
        font_size: cs.font_size,
        ..parent_ctx
    };

    // Color next: `currentcolor` elsewhere refers to it.
    if let Some(v) = specified[Longhand::Color.index()] {
        cs.color = match v {
            // `currentcolor` on `color` itself means inherit.
            Value::Color(ColorValue::CurrentColor) => parent.color,
            Value::Color(ColorValue::Color(c)) => *c,
            other => source_for(other, Longhand::Color).map_or(cs.color, |s| s.color),
        };
    }

    for (i, v) in specified.iter().enumerate() {
        let Some(v) = v else {
            continue;
        };
        let lh = LONGHANDS[i];
        if matches!(lh, Longhand::FontSize | Longhand::Color) {
            continue;
        }
        if let Some(src) = source_for(v, lh) {
            copy_longhand(&mut cs, src, lh);
            continue;
        }
        apply(&mut cs, lh, v, parent, &ctx);
    }

    // Border widths compute to zero when the side has no visible style.
    for (style, width) in [
        (cs.border_style.top, &mut cs.border_width.top),
        (cs.border_style.right, &mut cs.border_width.right),
        (cs.border_style.bottom, &mut cs.border_width.bottom),
        (cs.border_style.left, &mut cs.border_width.left),
    ] {
        if matches!(style, BorderStyle::None | BorderStyle::Hidden) {
            *width = 0.0;
        }
    }

    cs
}

fn color_of(v: &ColorValue, cs: &ComputedStyle) -> luma_css::Color {
    match v {
        ColorValue::Color(c) => *c,
        ColorValue::CurrentColor => cs.color,
    }
}

fn apply(cs: &mut ComputedStyle, lh: Longhand, v: &Value, parent: &ComputedStyle, ctx: &LengthCtx) {
    use Longhand as L;
    match (lh, v) {
        (L::Display, Value::Display(d)) => cs.display = *d,
        (L::Width, Value::Size(s)) => cs.width = ctx.resolve_size(*s),
        (L::Height, Value::Size(s)) => cs.height = ctx.resolve_size(*s),
        (L::MinWidth, Value::Size(s)) => cs.min_width = ctx.resolve_size(*s),
        (L::MinHeight, Value::Size(s)) => cs.min_height = ctx.resolve_size(*s),
        (L::MaxWidth, Value::MaxSize(s)) => cs.max_width = ctx.resolve_max(*s),
        (L::MaxHeight, Value::MaxSize(s)) => cs.max_height = ctx.resolve_max(*s),
        (L::MarginTop, Value::Size(s)) => cs.margin.top = ctx.resolve_size(*s),
        (L::MarginRight, Value::Size(s)) => cs.margin.right = ctx.resolve_size(*s),
        (L::MarginBottom, Value::Size(s)) => cs.margin.bottom = ctx.resolve_size(*s),
        (L::MarginLeft, Value::Size(s)) => cs.margin.left = ctx.resolve_size(*s),
        (L::PaddingTop, Value::LengthPct(lp)) => cs.padding.top = ctx.resolve_lp(*lp),
        (L::PaddingRight, Value::LengthPct(lp)) => cs.padding.right = ctx.resolve_lp(*lp),
        (L::PaddingBottom, Value::LengthPct(lp)) => cs.padding.bottom = ctx.resolve_lp(*lp),
        (L::PaddingLeft, Value::LengthPct(lp)) => cs.padding.left = ctx.resolve_lp(*lp),
        (L::BorderTopWidth, Value::Length(l)) => cs.border_width.top = ctx.resolve(*l),
        (L::BorderRightWidth, Value::Length(l)) => cs.border_width.right = ctx.resolve(*l),
        (L::BorderBottomWidth, Value::Length(l)) => cs.border_width.bottom = ctx.resolve(*l),
        (L::BorderLeftWidth, Value::Length(l)) => cs.border_width.left = ctx.resolve(*l),
        (L::BorderTopStyle, Value::BorderStyle(s)) => cs.border_style.top = *s,
        (L::BorderRightStyle, Value::BorderStyle(s)) => cs.border_style.right = *s,
        (L::BorderBottomStyle, Value::BorderStyle(s)) => cs.border_style.bottom = *s,
        (L::BorderLeftStyle, Value::BorderStyle(s)) => cs.border_style.left = *s,
        (L::BorderTopColor, Value::Color(c)) => cs.border_color.top = color_of(c, cs),
        (L::BorderRightColor, Value::Color(c)) => cs.border_color.right = color_of(c, cs),
        (L::BorderBottomColor, Value::Color(c)) => cs.border_color.bottom = color_of(c, cs),
        (L::BorderLeftColor, Value::Color(c)) => cs.border_color.left = color_of(c, cs),
        (L::BackgroundColor, Value::Color(c)) => cs.background_color = color_of(c, cs),
        (L::FontFamily, Value::FontFamily(f)) => cs.font_family = f.clone(),
        (L::FontWeight, Value::FontWeight(w)) => {
            cs.font_weight = match w {
                FontWeightSpec::Absolute(w) => *w,
                FontWeightSpec::Bolder => match parent.font_weight {
                    w if w < 350 => 400,
                    w if w < 550 => 700,
                    _ => 900,
                },
                FontWeightSpec::Lighter => match parent.font_weight {
                    w if w < 550 => 100,
                    w if w < 750 => 400,
                    _ => 700,
                },
            }
        }
        (L::FontStyle, Value::FontStyle(s)) => cs.font_style = *s,
        (L::LineHeight, Value::LineHeight(lh)) => {
            cs.line_height = match lh {
                LineHeightSpec::Normal => LineHeight::Normal,
                LineHeightSpec::Number(n) => LineHeight::Number(*n),
                LineHeightSpec::Length(LengthPercentage::Length(l)) => {
                    LineHeight::Px(ctx.resolve(*l))
                }
                LineHeightSpec::Length(LengthPercentage::Percentage(p)) => {
                    LineHeight::Px(cs.font_size * p / 100.0)
                }
            }
        }
        (L::TextAlign, Value::TextAlign(t)) => cs.text_align = *t,
        (L::WhiteSpace, Value::WhiteSpace(w)) => cs.white_space = *w,
        (L::TextDecorationLine, Value::TextDecoration(t)) => cs.text_decoration = *t,
        // Font size and color are handled before this loop; a mismatched
        // (longhand, value) pair cannot be produced by `parse_declaration`.
        _ => {}
    }
}

/// Copy one longhand's computed value from `src` into `dst`.
fn copy_longhand(dst: &mut ComputedStyle, src: &ComputedStyle, lh: Longhand) {
    use Longhand as L;
    match lh {
        L::Display => dst.display = src.display,
        L::Width => dst.width = src.width,
        L::Height => dst.height = src.height,
        L::MinWidth => dst.min_width = src.min_width,
        L::MinHeight => dst.min_height = src.min_height,
        L::MaxWidth => dst.max_width = src.max_width,
        L::MaxHeight => dst.max_height = src.max_height,
        L::MarginTop => dst.margin.top = src.margin.top,
        L::MarginRight => dst.margin.right = src.margin.right,
        L::MarginBottom => dst.margin.bottom = src.margin.bottom,
        L::MarginLeft => dst.margin.left = src.margin.left,
        L::PaddingTop => dst.padding.top = src.padding.top,
        L::PaddingRight => dst.padding.right = src.padding.right,
        L::PaddingBottom => dst.padding.bottom = src.padding.bottom,
        L::PaddingLeft => dst.padding.left = src.padding.left,
        L::BorderTopWidth => dst.border_width.top = src.border_width.top,
        L::BorderRightWidth => dst.border_width.right = src.border_width.right,
        L::BorderBottomWidth => dst.border_width.bottom = src.border_width.bottom,
        L::BorderLeftWidth => dst.border_width.left = src.border_width.left,
        L::BorderTopStyle => dst.border_style.top = src.border_style.top,
        L::BorderRightStyle => dst.border_style.right = src.border_style.right,
        L::BorderBottomStyle => dst.border_style.bottom = src.border_style.bottom,
        L::BorderLeftStyle => dst.border_style.left = src.border_style.left,
        L::BorderTopColor => dst.border_color.top = src.border_color.top,
        L::BorderRightColor => dst.border_color.right = src.border_color.right,
        L::BorderBottomColor => dst.border_color.bottom = src.border_color.bottom,
        L::BorderLeftColor => dst.border_color.left = src.border_color.left,
        L::Color => dst.color = src.color,
        L::BackgroundColor => dst.background_color = src.background_color,
        L::FontSize => dst.font_size = src.font_size,
        L::FontFamily => dst.font_family.clone_from(&src.font_family),
        L::FontWeight => dst.font_weight = src.font_weight,
        L::FontStyle => dst.font_style = src.font_style,
        L::LineHeight => dst.line_height = src.line_height,
        L::TextAlign => dst.text_align = src.text_align,
        L::WhiteSpace => dst.white_space = src.white_space,
        L::TextDecorationLine => dst.text_decoration = src.text_decoration,
    }
}

/// All longhands, in `index()` order.
const LONGHANDS: [Longhand; Longhand::COUNT] = [
    Longhand::Display,
    Longhand::Width,
    Longhand::Height,
    Longhand::MinWidth,
    Longhand::MinHeight,
    Longhand::MaxWidth,
    Longhand::MaxHeight,
    Longhand::MarginTop,
    Longhand::MarginRight,
    Longhand::MarginBottom,
    Longhand::MarginLeft,
    Longhand::PaddingTop,
    Longhand::PaddingRight,
    Longhand::PaddingBottom,
    Longhand::PaddingLeft,
    Longhand::BorderTopWidth,
    Longhand::BorderRightWidth,
    Longhand::BorderBottomWidth,
    Longhand::BorderLeftWidth,
    Longhand::BorderTopStyle,
    Longhand::BorderRightStyle,
    Longhand::BorderBottomStyle,
    Longhand::BorderLeftStyle,
    Longhand::BorderTopColor,
    Longhand::BorderRightColor,
    Longhand::BorderBottomColor,
    Longhand::BorderLeftColor,
    Longhand::Color,
    Longhand::BackgroundColor,
    Longhand::FontSize,
    Longhand::FontFamily,
    Longhand::FontWeight,
    Longhand::FontStyle,
    Longhand::LineHeight,
    Longhand::TextAlign,
    Longhand::WhiteSpace,
    Longhand::TextDecorationLine,
];
