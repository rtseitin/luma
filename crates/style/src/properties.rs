//! The property table: which longhands exist, how each parses, and how
//! shorthands expand.

use luma_css::{ColorValue, ComponentValue, Declaration, Length, LengthPercentage, Token};

use crate::computed::{BorderStyle, Display, FontStyle, TextAlign, TextDecoration, WhiteSpace};

/// Every longhand property the engine understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Longhand {
    /// `display`
    Display,
    /// `width`
    Width,
    /// `height`
    Height,
    /// `min-width`
    MinWidth,
    /// `min-height`
    MinHeight,
    /// `max-width`
    MaxWidth,
    /// `max-height`
    MaxHeight,
    /// `margin-top`
    MarginTop,
    /// `margin-right`
    MarginRight,
    /// `margin-bottom`
    MarginBottom,
    /// `margin-left`
    MarginLeft,
    /// `padding-top`
    PaddingTop,
    /// `padding-right`
    PaddingRight,
    /// `padding-bottom`
    PaddingBottom,
    /// `padding-left`
    PaddingLeft,
    /// `border-top-width`
    BorderTopWidth,
    /// `border-right-width`
    BorderRightWidth,
    /// `border-bottom-width`
    BorderBottomWidth,
    /// `border-left-width`
    BorderLeftWidth,
    /// `border-top-style`
    BorderTopStyle,
    /// `border-right-style`
    BorderRightStyle,
    /// `border-bottom-style`
    BorderBottomStyle,
    /// `border-left-style`
    BorderLeftStyle,
    /// `border-top-color`
    BorderTopColor,
    /// `border-right-color`
    BorderRightColor,
    /// `border-bottom-color`
    BorderBottomColor,
    /// `border-left-color`
    BorderLeftColor,
    /// `color`
    Color,
    /// `background-color`
    BackgroundColor,
    /// `font-size`
    FontSize,
    /// `font-family`
    FontFamily,
    /// `font-weight`
    FontWeight,
    /// `font-style`
    FontStyle,
    /// `line-height`
    LineHeight,
    /// `text-align`
    TextAlign,
    /// `white-space`
    WhiteSpace,
    /// `text-decoration-line`
    TextDecorationLine,
}

impl Longhand {
    /// Number of longhands.
    pub const COUNT: usize = 37;

    /// Dense index for table lookups.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Whether the property inherits by default.
    pub fn is_inherited(self) -> bool {
        matches!(
            self,
            Longhand::Color
                | Longhand::FontSize
                | Longhand::FontFamily
                | Longhand::FontWeight
                | Longhand::FontStyle
                | Longhand::LineHeight
                | Longhand::TextAlign
                | Longhand::WhiteSpace
        )
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "display" => Self::Display,
            "width" => Self::Width,
            "height" => Self::Height,
            "min-width" => Self::MinWidth,
            "min-height" => Self::MinHeight,
            "max-width" => Self::MaxWidth,
            "max-height" => Self::MaxHeight,
            "margin-top" => Self::MarginTop,
            "margin-right" => Self::MarginRight,
            "margin-bottom" => Self::MarginBottom,
            "margin-left" => Self::MarginLeft,
            "padding-top" => Self::PaddingTop,
            "padding-right" => Self::PaddingRight,
            "padding-bottom" => Self::PaddingBottom,
            "padding-left" => Self::PaddingLeft,
            "border-top-width" => Self::BorderTopWidth,
            "border-right-width" => Self::BorderRightWidth,
            "border-bottom-width" => Self::BorderBottomWidth,
            "border-left-width" => Self::BorderLeftWidth,
            "border-top-style" => Self::BorderTopStyle,
            "border-right-style" => Self::BorderRightStyle,
            "border-bottom-style" => Self::BorderBottomStyle,
            "border-left-style" => Self::BorderLeftStyle,
            "border-top-color" => Self::BorderTopColor,
            "border-right-color" => Self::BorderRightColor,
            "border-bottom-color" => Self::BorderBottomColor,
            "border-left-color" => Self::BorderLeftColor,
            "color" => Self::Color,
            "background-color" => Self::BackgroundColor,
            "font-size" => Self::FontSize,
            "font-family" => Self::FontFamily,
            "font-weight" => Self::FontWeight,
            "font-style" => Self::FontStyle,
            "line-height" => Self::LineHeight,
            "text-align" => Self::TextAlign,
            "white-space" => Self::WhiteSpace,
            "text-decoration-line" => Self::TextDecorationLine,
            _ => return None,
        })
    }
}

const MARGINS: [Longhand; 4] = [
    Longhand::MarginTop,
    Longhand::MarginRight,
    Longhand::MarginBottom,
    Longhand::MarginLeft,
];
const PADDINGS: [Longhand; 4] = [
    Longhand::PaddingTop,
    Longhand::PaddingRight,
    Longhand::PaddingBottom,
    Longhand::PaddingLeft,
];
const BORDER_WIDTHS: [Longhand; 4] = [
    Longhand::BorderTopWidth,
    Longhand::BorderRightWidth,
    Longhand::BorderBottomWidth,
    Longhand::BorderLeftWidth,
];
const BORDER_STYLES: [Longhand; 4] = [
    Longhand::BorderTopStyle,
    Longhand::BorderRightStyle,
    Longhand::BorderBottomStyle,
    Longhand::BorderLeftStyle,
];
const BORDER_COLORS: [Longhand; 4] = [
    Longhand::BorderTopColor,
    Longhand::BorderRightColor,
    Longhand::BorderBottomColor,
    Longhand::BorderLeftColor,
];

/// `width`-like specified value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecSize {
    /// `auto`
    Auto,
    /// A length or percentage.
    Length(LengthPercentage),
}

/// `max-width`-like specified value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecMaxSize {
    /// `none`
    None,
    /// A length or percentage.
    Length(LengthPercentage),
}

/// Specified `font-size`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSizeSpec {
    /// A length (em/% relative to the parent's font size).
    Length(LengthPercentage),
    /// An absolute-size keyword, already mapped to pixels.
    Absolute(f32),
    /// `smaller`
    Smaller,
    /// `larger`
    Larger,
}

/// Specified `font-weight`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontWeightSpec {
    /// A number or `normal`/`bold`.
    Absolute(u16),
    /// `bolder`
    Bolder,
    /// `lighter`
    Lighter,
}

/// Specified `line-height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeightSpec {
    /// `normal`
    Normal,
    /// A unitless number.
    Number(f32),
    /// A length or percentage (of the element's font size).
    Length(LengthPercentage),
}

/// A specified value for some longhand, before computation.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `inherit`
    Inherit,
    /// `initial`
    Initial,
    /// `unset`
    Unset,
    /// For `display`.
    Display(Display),
    /// For `width`, `height`, `min-*`, `margin-*`.
    Size(SpecSize),
    /// For `max-*`.
    MaxSize(SpecMaxSize),
    /// For `padding-*`.
    LengthPct(LengthPercentage),
    /// For `border-*-width`.
    Length(Length),
    /// For `color`, `background-color`, `border-*-color`.
    Color(ColorValue),
    /// For `border-*-style`.
    BorderStyle(BorderStyle),
    /// For `font-size`.
    FontSize(FontSizeSpec),
    /// For `font-family`.
    FontFamily(Vec<String>),
    /// For `font-weight`.
    FontWeight(FontWeightSpec),
    /// For `font-style`.
    FontStyle(FontStyle),
    /// For `line-height`.
    LineHeight(LineHeightSpec),
    /// For `text-align`.
    TextAlign(TextAlign),
    /// For `white-space`.
    WhiteSpace(WhiteSpace),
    /// For `text-decoration-line`.
    TextDecoration(TextDecoration),
}

/// A longhand with its specified value.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDeclaration {
    /// The property.
    pub longhand: Longhand,
    /// Its value.
    pub value: Value,
}

/// Turn a parsed declaration into longhand declarations, expanding
/// shorthands. Returns an empty vector for unknown properties or invalid
/// values, which is how CSS ignores them.
pub fn parse_declaration(decl: &Declaration) -> Vec<PropertyDeclaration> {
    let v = &decl.value;
    if v.is_empty() {
        return Vec::new();
    }

    if let Some(lh) = Longhand::from_name(&decl.name) {
        return parse_longhand(lh, v)
            .map(|value| {
                vec![PropertyDeclaration {
                    longhand: lh,
                    value,
                }]
            })
            .unwrap_or_default();
    }

    match decl.name.as_str() {
        "margin" => expand_box(v, MARGINS, |cv| parse_size(cv).map(Value::Size)),
        "padding" => expand_box(v, PADDINGS, |cv| {
            LengthPercentage::parse(cv).map(Value::LengthPct)
        }),
        "border-width" => expand_box(v, BORDER_WIDTHS, |cv| {
            parse_border_width(cv).map(Value::Length)
        }),
        "border-style" => expand_box(v, BORDER_STYLES, |cv| {
            parse_border_style(cv).map(Value::BorderStyle)
        }),
        "border-color" => expand_box(v, BORDER_COLORS, |cv| {
            luma_css::Color::parse(cv).map(Value::Color)
        }),
        "border" => expand_border(v, &[0, 1, 2, 3]),
        "border-top" => expand_border(v, &[0]),
        "border-right" => expand_border(v, &[1]),
        "border-bottom" => expand_border(v, &[2]),
        "border-left" => expand_border(v, &[3]),
        "background" => expand_background(v),
        "text-decoration" => expand_text_decoration(v),
        _ => Vec::new(),
    }
}

fn css_wide(v: &[ComponentValue]) -> Option<Value> {
    if v.len() != 1 {
        return None;
    }
    match v[0].as_ident()?.to_ascii_lowercase().as_str() {
        "inherit" => Some(Value::Inherit),
        "initial" => Some(Value::Initial),
        "unset" => Some(Value::Unset),
        _ => None,
    }
}

fn single(v: &[ComponentValue]) -> Option<&ComponentValue> {
    (v.len() == 1).then(|| &v[0])
}

fn ident(cv: &ComponentValue) -> Option<String> {
    cv.as_ident().map(str::to_ascii_lowercase)
}

fn parse_longhand(lh: Longhand, v: &[ComponentValue]) -> Option<Value> {
    if let Some(w) = css_wide(v) {
        return Some(w);
    }
    use Longhand as L;
    match lh {
        L::Display => parse_display(single(v)?).map(Value::Display),
        L::Width | L::Height | L::MinWidth | L::MinHeight => {
            parse_size(single(v)?).map(Value::Size)
        }
        L::MaxWidth | L::MaxHeight => parse_max_size(single(v)?).map(Value::MaxSize),
        L::MarginTop | L::MarginRight | L::MarginBottom | L::MarginLeft => {
            parse_size(single(v)?).map(Value::Size)
        }
        L::PaddingTop | L::PaddingRight | L::PaddingBottom | L::PaddingLeft => {
            LengthPercentage::parse(single(v)?).map(Value::LengthPct)
        }
        L::BorderTopWidth | L::BorderRightWidth | L::BorderBottomWidth | L::BorderLeftWidth => {
            parse_border_width(single(v)?).map(Value::Length)
        }
        L::BorderTopStyle | L::BorderRightStyle | L::BorderBottomStyle | L::BorderLeftStyle => {
            parse_border_style(single(v)?).map(Value::BorderStyle)
        }
        L::BorderTopColor
        | L::BorderRightColor
        | L::BorderBottomColor
        | L::BorderLeftColor
        | L::Color
        | L::BackgroundColor => luma_css::Color::parse(single(v)?).map(Value::Color),
        L::FontSize => parse_font_size(single(v)?).map(Value::FontSize),
        L::FontFamily => parse_font_family(v).map(Value::FontFamily),
        L::FontWeight => parse_font_weight(single(v)?).map(Value::FontWeight),
        L::FontStyle => parse_font_style(single(v)?).map(Value::FontStyle),
        L::LineHeight => parse_line_height(single(v)?).map(Value::LineHeight),
        L::TextAlign => parse_text_align(single(v)?).map(Value::TextAlign),
        L::WhiteSpace => parse_white_space(single(v)?).map(Value::WhiteSpace),
        L::TextDecorationLine => parse_text_decoration_line(v).map(Value::TextDecoration),
    }
}

// ----- individual value grammars ---------------------------------------------

fn parse_display(cv: &ComponentValue) -> Option<Display> {
    Some(match ident(cv)?.as_str() {
        "block" => Display::Block,
        "inline" => Display::Inline,
        "inline-block" => Display::InlineBlock,
        "list-item" => Display::ListItem,
        "flex" => Display::Flex,
        "none" => Display::None,
        _ => return None,
    })
}

fn parse_size(cv: &ComponentValue) -> Option<SpecSize> {
    if ident(cv).as_deref() == Some("auto") {
        return Some(SpecSize::Auto);
    }
    LengthPercentage::parse(cv).map(SpecSize::Length)
}

fn parse_max_size(cv: &ComponentValue) -> Option<SpecMaxSize> {
    if ident(cv).as_deref() == Some("none") {
        return Some(SpecMaxSize::None);
    }
    LengthPercentage::parse(cv).map(SpecMaxSize::Length)
}

fn parse_border_width(cv: &ComponentValue) -> Option<Length> {
    match ident(cv).as_deref() {
        Some("thin") => Some(Length::px(1.0)),
        Some("medium") => Some(Length::px(3.0)),
        Some("thick") => Some(Length::px(5.0)),
        Some(_) => None,
        None => Length::parse(cv).filter(|l| l.value >= 0.0),
    }
}

fn parse_border_style(cv: &ComponentValue) -> Option<BorderStyle> {
    Some(match ident(cv)?.as_str() {
        "none" => BorderStyle::None,
        "hidden" => BorderStyle::Hidden,
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        "groove" => BorderStyle::Groove,
        "ridge" => BorderStyle::Ridge,
        "inset" => BorderStyle::Inset,
        "outset" => BorderStyle::Outset,
        _ => return None,
    })
}

fn parse_font_size(cv: &ComponentValue) -> Option<FontSizeSpec> {
    if let Some(kw) = ident(cv) {
        return Some(match kw.as_str() {
            "xx-small" => FontSizeSpec::Absolute(9.0),
            "x-small" => FontSizeSpec::Absolute(10.0),
            "small" => FontSizeSpec::Absolute(13.0),
            "medium" => FontSizeSpec::Absolute(16.0),
            "large" => FontSizeSpec::Absolute(18.0),
            "x-large" => FontSizeSpec::Absolute(24.0),
            "xx-large" => FontSizeSpec::Absolute(32.0),
            "xxx-large" => FontSizeSpec::Absolute(48.0),
            "smaller" => FontSizeSpec::Smaller,
            "larger" => FontSizeSpec::Larger,
            _ => return None,
        });
    }
    LengthPercentage::parse(cv).map(FontSizeSpec::Length)
}

fn parse_font_family(v: &[ComponentValue]) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for part in v.split(|cv| matches!(cv, ComponentValue::Token(Token::Comma))) {
        match part {
            [ComponentValue::Token(Token::Str(s))] => out.push(s.clone()),
            [] => return None,
            idents => {
                // Unquoted family names are a sequence of identifiers.
                let words: Option<Vec<&str>> = idents.iter().map(|cv| cv.as_ident()).collect();
                let words = words?;
                let name = words.join(" ");
                // Generic families are lowercased so layout can recognize them.
                let lower = name.to_ascii_lowercase();
                out.push(
                    if matches!(
                        lower.as_str(),
                        "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
                    ) {
                        lower
                    } else {
                        name
                    },
                );
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn parse_font_weight(cv: &ComponentValue) -> Option<FontWeightSpec> {
    match cv {
        ComponentValue::Token(Token::Number { value, .. }) if (1.0..=1000.0).contains(value) => {
            Some(FontWeightSpec::Absolute(*value as u16))
        }
        _ => Some(match ident(cv)?.as_str() {
            "normal" => FontWeightSpec::Absolute(400),
            "bold" => FontWeightSpec::Absolute(700),
            "bolder" => FontWeightSpec::Bolder,
            "lighter" => FontWeightSpec::Lighter,
            _ => return None,
        }),
    }
}

fn parse_font_style(cv: &ComponentValue) -> Option<FontStyle> {
    Some(match ident(cv)?.as_str() {
        "normal" => FontStyle::Normal,
        "italic" => FontStyle::Italic,
        "oblique" => FontStyle::Oblique,
        _ => return None,
    })
}

fn parse_line_height(cv: &ComponentValue) -> Option<LineHeightSpec> {
    if ident(cv).as_deref() == Some("normal") {
        return Some(LineHeightSpec::Normal);
    }
    if let ComponentValue::Token(Token::Number { value, .. }) = cv {
        return (*value >= 0.0).then_some(LineHeightSpec::Number(*value));
    }
    LengthPercentage::parse(cv).map(LineHeightSpec::Length)
}

fn parse_text_align(cv: &ComponentValue) -> Option<TextAlign> {
    Some(match ident(cv)?.as_str() {
        "left" => TextAlign::Left,
        "right" => TextAlign::Right,
        "center" => TextAlign::Center,
        "justify" => TextAlign::Justify,
        "start" => TextAlign::Start,
        "end" => TextAlign::End,
        _ => return None,
    })
}

fn parse_white_space(cv: &ComponentValue) -> Option<WhiteSpace> {
    Some(match ident(cv)?.as_str() {
        "normal" => WhiteSpace::Normal,
        "pre" => WhiteSpace::Pre,
        "nowrap" => WhiteSpace::Nowrap,
        "pre-wrap" => WhiteSpace::PreWrap,
        "pre-line" => WhiteSpace::PreLine,
        _ => return None,
    })
}

fn parse_text_decoration_line(v: &[ComponentValue]) -> Option<TextDecoration> {
    if v.len() == 1 && ident(&v[0]).as_deref() == Some("none") {
        return Some(TextDecoration::default());
    }
    let mut td = TextDecoration::default();
    for cv in v {
        match ident(cv)?.as_str() {
            "underline" if !td.underline => td.underline = true,
            "overline" if !td.overline => td.overline = true,
            "line-through" if !td.line_through => td.line_through = true,
            _ => return None,
        }
    }
    Some(td)
}

// ----- shorthands ------------------------------------------------------------

/// Expand a 1-4 value box shorthand (`margin`, `padding`, `border-width`,
/// ...) into top/right/bottom/left.
fn expand_box(
    v: &[ComponentValue],
    longhands: [Longhand; 4],
    parse: impl Fn(&ComponentValue) -> Option<Value>,
) -> Vec<PropertyDeclaration> {
    if let Some(w) = css_wide(v) {
        return longhands
            .iter()
            .map(|&lh| PropertyDeclaration {
                longhand: lh,
                value: w.clone(),
            })
            .collect();
    }
    if v.is_empty() || v.len() > 4 {
        return Vec::new();
    }
    let Some(vals): Option<Vec<Value>> = v.iter().map(&parse).collect() else {
        return Vec::new();
    };
    let (t, r, b, l) = match vals.as_slice() {
        [a] => (a, a, a, a),
        [a, b] => (a, b, a, b),
        [a, b, c] => (a, b, c, b),
        [a, b, c, d] => (a, b, c, d),
        _ => unreachable!(),
    };
    [t, r, b, l]
        .into_iter()
        .zip(longhands)
        .map(|(value, longhand)| PropertyDeclaration {
            longhand,
            value: value.clone(),
        })
        .collect()
}

/// `border` and `border-<side>`: any order of width, style, color, each
/// optional. Omitted parts reset to their initial values. `sides` are
/// indices into the TRBL arrays.
fn expand_border(v: &[ComponentValue], sides: &[usize]) -> Vec<PropertyDeclaration> {
    let emit = |width: Value, style: Value, color: Value| -> Vec<PropertyDeclaration> {
        let mut out = Vec::with_capacity(sides.len() * 3);
        for &s in sides {
            out.push(PropertyDeclaration {
                longhand: BORDER_WIDTHS[s],
                value: width.clone(),
            });
            out.push(PropertyDeclaration {
                longhand: BORDER_STYLES[s],
                value: style.clone(),
            });
            out.push(PropertyDeclaration {
                longhand: BORDER_COLORS[s],
                value: color.clone(),
            });
        }
        out
    };

    if let Some(w) = css_wide(v) {
        return emit(w.clone(), w.clone(), w);
    }
    if v.is_empty() || v.len() > 3 {
        return Vec::new();
    }

    let mut width = None;
    let mut style = None;
    let mut color = None;
    for cv in v {
        if width.is_none()
            && let Some(w) = parse_border_width(cv)
        {
            width = Some(w);
        } else if style.is_none()
            && let Some(s) = parse_border_style(cv)
        {
            style = Some(s);
        } else if color.is_none()
            && let Some(c) = luma_css::Color::parse(cv)
        {
            color = Some(c);
        } else {
            return Vec::new();
        }
    }
    emit(
        Value::Length(width.unwrap_or(Length::px(3.0))),
        Value::BorderStyle(style.unwrap_or(BorderStyle::None)),
        Value::Color(color.unwrap_or(ColorValue::CurrentColor)),
    )
}

/// `background`: only the color is modelled. Any other component (image,
/// position, ...) is accepted and ignored.
fn expand_background(v: &[ComponentValue]) -> Vec<PropertyDeclaration> {
    let value = css_wide(v).unwrap_or_else(|| {
        let color = v
            .iter()
            .filter(|cv| !matches!(cv, ComponentValue::Token(Token::Comma)))
            .find_map(luma_css::Color::parse)
            .unwrap_or(ColorValue::Color(luma_css::Color::TRANSPARENT));
        Value::Color(color)
    });
    vec![PropertyDeclaration {
        longhand: Longhand::BackgroundColor,
        value,
    }]
}

/// `text-decoration`: line keywords are kept; a color or style component
/// is accepted and ignored.
fn expand_text_decoration(v: &[ComponentValue]) -> Vec<PropertyDeclaration> {
    let value = match css_wide(v) {
        Some(w) => w,
        None => {
            let lines: Vec<ComponentValue> = v
                .iter()
                .filter(|cv| {
                    ident(cv).is_some_and(|i| {
                        matches!(
                            i.as_str(),
                            "none" | "underline" | "overline" | "line-through"
                        )
                    })
                })
                .cloned()
                .collect();
            if lines.is_empty() {
                Value::TextDecoration(TextDecoration::default())
            } else {
                match parse_text_decoration_line(&lines) {
                    Some(td) => Value::TextDecoration(td),
                    None => return Vec::new(),
                }
            }
        }
    };
    vec![PropertyDeclaration {
        longhand: Longhand::TextDecorationLine,
        value,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use luma_css::LengthUnit;

    fn parse(src: &str) -> Vec<PropertyDeclaration> {
        let decls = luma_css::parse_declaration_list(src);
        assert_eq!(decls.len(), 1, "{src:?}");
        parse_declaration(&decls[0])
    }

    fn lhs(src: &str) -> Vec<Longhand> {
        parse(src).into_iter().map(|d| d.longhand).collect()
    }

    #[test]
    fn longhand_count_matches_enum() {
        assert_eq!(Longhand::TextDecorationLine.index() + 1, Longhand::COUNT);
    }

    #[test]
    fn unknown_or_invalid_is_empty() {
        assert!(parse("bogus: 1").is_empty());
        assert!(parse("display: bogus").is_empty());
        assert!(parse("width: red").is_empty());
        assert!(parse("width: 1px 2px").is_empty());
        assert!(parse("margin: 1px 2px 3px 4px 5px").is_empty());
        assert!(parse("margin: 1px bogus").is_empty());
        assert!(parse("border-width: -1px").is_empty());
        assert!(parse("font-weight: 0").is_empty());
    }

    #[test]
    fn css_wide_keywords() {
        assert_eq!(parse("color: inherit")[0].value, Value::Inherit);
        assert_eq!(parse("margin: INITIAL").len(), 4);
        assert!(
            parse("margin: initial")
                .iter()
                .all(|d| d.value == Value::Initial)
        );
        assert!(
            parse("border: unset")
                .iter()
                .all(|d| d.value == Value::Unset)
        );
        assert_eq!(parse("border: unset").len(), 12);
    }

    #[test]
    fn box_shorthand_expansion() {
        let px = |v: f32| Value::Size(SpecSize::Length(LengthPercentage::Length(Length::px(v))));
        let d = parse("margin: 1px");
        assert_eq!(
            d.iter().map(|d| d.value.clone()).collect::<Vec<_>>(),
            [px(1.0), px(1.0), px(1.0), px(1.0)]
        );
        let d = parse("margin: 1px 2px");
        assert_eq!(
            d.iter().map(|d| d.value.clone()).collect::<Vec<_>>(),
            [px(1.0), px(2.0), px(1.0), px(2.0)]
        );
        let d = parse("margin: 1px 2px 3px");
        assert_eq!(
            d.iter().map(|d| d.value.clone()).collect::<Vec<_>>(),
            [px(1.0), px(2.0), px(3.0), px(2.0)]
        );
        let d = parse("margin: 1px 2px 3px auto");
        assert_eq!(d[3].value, Value::Size(SpecSize::Auto));
        assert_eq!(lhs("margin: 0"), MARGINS);
        assert_eq!(lhs("padding: 0"), PADDINGS);
    }

    #[test]
    fn border_shorthand() {
        let d = parse("border: 2px dashed red");
        assert_eq!(d.len(), 12);
        assert_eq!(d[0].longhand, Longhand::BorderTopWidth);
        assert_eq!(d[0].value, Value::Length(Length::px(2.0)));
        assert_eq!(d[1].value, Value::BorderStyle(BorderStyle::Dashed));
        assert_eq!(
            d[2].value,
            Value::Color(ColorValue::Color(luma_css::Color::rgb(255, 0, 0)))
        );

        // Any order; omitted parts get initial values.
        let d = parse("border: solid");
        assert_eq!(d[0].value, Value::Length(Length::px(3.0)));
        assert_eq!(d[1].value, Value::BorderStyle(BorderStyle::Solid));
        assert_eq!(d[2].value, Value::Color(ColorValue::CurrentColor));

        let d = parse("border-left: red thin");
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].longhand, Longhand::BorderLeftWidth);
        assert_eq!(d[0].value, Value::Length(Length::px(1.0)));

        assert!(parse("border: 1px 2px").is_empty(), "two widths");
        assert!(parse("border: 1px solid red blue").is_empty(), "too many");
    }

    #[test]
    fn background_and_text_decoration() {
        let d = parse("background: url(x.png) no-repeat #fff");
        assert_eq!(d.len(), 1);
        assert_eq!(
            d[0].value,
            Value::Color(ColorValue::Color(luma_css::Color::WHITE))
        );
        let d = parse("background: url(x.png)");
        assert_eq!(
            d[0].value,
            Value::Color(ColorValue::Color(luma_css::Color::TRANSPARENT))
        );
        let d = parse("text-decoration: underline red wavy");
        assert_eq!(
            d[0].value,
            Value::TextDecoration(TextDecoration {
                underline: true,
                ..Default::default()
            })
        );
        let d = parse("text-decoration-line: underline line-through");
        assert!(matches!(
            d[0].value,
            Value::TextDecoration(TextDecoration {
                underline: true,
                line_through: true,
                overline: false
            })
        ));
        assert!(parse("text-decoration-line: underline underline").is_empty());
    }

    #[test]
    fn font_values() {
        assert_eq!(
            parse("font-family: 'Helvetica Neue', Arial Black, SANS-SERIF")[0].value,
            Value::FontFamily(vec![
                "Helvetica Neue".into(),
                "Arial Black".into(),
                "sans-serif".into()
            ])
        );
        assert!(parse("font-family: 'a',").is_empty());
        assert_eq!(
            parse("font-size: 1.5em")[0].value,
            Value::FontSize(FontSizeSpec::Length(LengthPercentage::Length(Length::new(
                1.5,
                LengthUnit::Em
            ))))
        );
        assert_eq!(
            parse("font-size: large")[0].value,
            Value::FontSize(FontSizeSpec::Absolute(18.0))
        );
        assert_eq!(
            parse("font-weight: bold")[0].value,
            Value::FontWeight(FontWeightSpec::Absolute(700))
        );
        assert_eq!(
            parse("font-weight: 350")[0].value,
            Value::FontWeight(FontWeightSpec::Absolute(350))
        );
        assert_eq!(
            parse("line-height: 1.5")[0].value,
            Value::LineHeight(LineHeightSpec::Number(1.5))
        );
    }
}
