//! Computed style: the fully resolved value of every supported property.
//!
//! Relative lengths (em, rem, vw, ...) are resolved to pixels here.
//! Percentages are kept, because what they are relative to (the containing
//! block) is only known during layout.

pub use luma_css::Color;

/// Four values, one per side.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Edges<T> {
    /// Top.
    pub top: T,
    /// Right.
    pub right: T,
    /// Bottom.
    pub bottom: T,
    /// Left.
    pub left: T,
}

impl<T: Copy> Edges<T> {
    /// The same value on every side.
    pub fn all(v: T) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }
}

/// A resolved length or a percentage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthPct {
    /// Pixels.
    Px(f32),
    /// Percentage of the containing block (`50.0` for `50%`).
    Percent(f32),
}

impl LengthPct {
    /// Zero pixels.
    pub const ZERO: LengthPct = LengthPct::Px(0.0);

    /// Resolve against a containing-block size.
    pub fn resolve(self, container: f32) -> f32 {
        match self {
            LengthPct::Px(px) => px,
            LengthPct::Percent(p) => container * p / 100.0,
        }
    }
}

/// `width`, `height`, `min-*`, and margins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    /// `auto`
    Auto,
    /// A length or percentage.
    Length(LengthPct),
}

/// `max-width` and `max-height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaxSize {
    /// `none`
    None,
    /// A length or percentage.
    Length(LengthPct),
}

/// `display`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// `block`
    Block,
    /// `inline`
    Inline,
    /// `inline-block`
    InlineBlock,
    /// `list-item`
    ListItem,
    /// `flex`
    Flex,
    /// `none`
    None,
}

/// `border-*-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    /// `none`
    None,
    /// `hidden`
    Hidden,
    /// `solid`
    Solid,
    /// `dashed`
    Dashed,
    /// `dotted`
    Dotted,
    /// `double`
    Double,
    /// `groove`
    Groove,
    /// `ridge`
    Ridge,
    /// `inset`
    Inset,
    /// `outset`
    Outset,
}

/// `font-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    /// `normal`
    Normal,
    /// `italic`
    Italic,
    /// `oblique`
    Oblique,
}

/// `line-height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    /// `normal`; layout picks a font-appropriate value (~1.2).
    Normal,
    /// A unitless multiplier of the element's own font size. Kept as a
    /// number so descendants with different font sizes re-apply it.
    Number(f32),
    /// An absolute length.
    Px(f32),
}

/// `text-align`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    /// `left`
    Left,
    /// `right`
    Right,
    /// `center`
    Center,
    /// `justify`
    Justify,
    /// `start`
    Start,
    /// `end`
    End,
}

/// `white-space`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpace {
    /// `normal`
    Normal,
    /// `pre`
    Pre,
    /// `nowrap`
    Nowrap,
    /// `pre-wrap`
    PreWrap,
    /// `pre-line`
    PreLine,
}

/// `text-decoration-line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextDecoration {
    /// `underline`
    pub underline: bool,
    /// `overline`
    pub overline: bool,
    /// `line-through`
    pub line_through: bool,
}

/// Fully computed style for one element.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    /// `display`
    pub display: Display,

    /// `width`
    pub width: Size,
    /// `height`
    pub height: Size,
    /// `min-width`
    pub min_width: Size,
    /// `min-height`
    pub min_height: Size,
    /// `max-width`
    pub max_width: MaxSize,
    /// `max-height`
    pub max_height: MaxSize,

    /// `margin-*`
    pub margin: Edges<Size>,
    /// `padding-*`
    pub padding: Edges<LengthPct>,
    /// `border-*-width`, in pixels. Zero when the side's style is `none`
    /// or `hidden`.
    pub border_width: Edges<f32>,
    /// `border-*-style`
    pub border_style: Edges<BorderStyle>,
    /// `border-*-color`, with `currentcolor` resolved.
    pub border_color: Edges<Color>,

    /// `color`
    pub color: Color,
    /// `background-color`
    pub background_color: Color,

    /// `font-size`, in pixels.
    pub font_size: f32,
    /// `font-family`, in preference order.
    pub font_family: Vec<String>,
    /// `font-weight`, 1..=1000.
    pub font_weight: u16,
    /// `font-style`
    pub font_style: FontStyle,
    /// `line-height`
    pub line_height: LineHeight,

    /// `text-align`
    pub text_align: TextAlign,
    /// `white-space`
    pub white_space: WhiteSpace,
    /// `text-decoration-line`
    pub text_decoration: TextDecoration,
}

/// Default font size of the root element, in pixels.
pub const DEFAULT_FONT_SIZE: f32 = 16.0;

impl ComputedStyle {
    /// The initial value of every property. This is the style of the root
    /// element's (nonexistent) parent.
    pub fn initial() -> Self {
        Self {
            display: Display::Inline,
            width: Size::Auto,
            height: Size::Auto,
            min_width: Size::Auto,
            min_height: Size::Auto,
            max_width: MaxSize::None,
            max_height: MaxSize::None,
            margin: Edges::all(Size::Length(LengthPct::ZERO)),
            padding: Edges::all(LengthPct::ZERO),
            border_width: Edges::all(0.0),
            border_style: Edges::all(BorderStyle::None),
            border_color: Edges::all(Color::BLACK),
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            font_size: DEFAULT_FONT_SIZE,
            font_family: vec!["serif".to_owned()],
            font_weight: 400,
            font_style: FontStyle::Normal,
            line_height: LineHeight::Normal,
            text_align: TextAlign::Start,
            white_space: WhiteSpace::Normal,
            text_decoration: TextDecoration::default(),
        }
    }

    /// A child's starting style: inherited properties copied from
    /// `parent`, everything else at its initial value.
    pub fn inherit_from(parent: &ComputedStyle) -> Self {
        Self {
            color: parent.color,
            font_size: parent.font_size,
            font_family: parent.font_family.clone(),
            font_weight: parent.font_weight,
            font_style: parent.font_style,
            line_height: parent.line_height,
            text_align: parent.text_align,
            white_space: parent.white_space,
            ..Self::initial()
        }
    }

    /// `line-height` resolved to pixels using this element's font size.
    /// `normal` uses a 1.2 multiplier.
    pub fn line_height_px(&self) -> f32 {
        match self.line_height {
            LineHeight::Normal => self.font_size * 1.2,
            LineHeight::Number(n) => self.font_size * n,
            LineHeight::Px(px) => px,
        }
    }
}
