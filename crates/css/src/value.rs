//! Property-agnostic value types: lengths and colors.
//!
//! These parse from component values and are used by every property
//! grammar in `luma-style`. Resolving relative units (em, vw, ...) to
//! pixels needs context (font size, viewport) and is done there.

use crate::parser::ComponentValue;
use crate::tokenizer::Token;

/// Length units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    /// CSS pixels.
    Px,
    /// Relative to the element's font size.
    Em,
    /// Relative to the root element's font size.
    Rem,
    /// x-height of the font (approximated).
    Ex,
    /// Width of `0` in the font (approximated).
    Ch,
    /// 1% of viewport width.
    Vw,
    /// 1% of viewport height.
    Vh,
    /// 1% of the smaller viewport dimension.
    Vmin,
    /// 1% of the larger viewport dimension.
    Vmax,
    /// Points (1/72 in).
    Pt,
    /// Picas (12 pt).
    Pc,
    /// Inches (96 px).
    In,
    /// Centimeters.
    Cm,
    /// Millimeters.
    Mm,
    /// Quarter-millimeters.
    Q,
}

impl LengthUnit {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "px" => Self::Px,
            "em" => Self::Em,
            "rem" => Self::Rem,
            "ex" => Self::Ex,
            "ch" => Self::Ch,
            "vw" => Self::Vw,
            "vh" => Self::Vh,
            "vmin" => Self::Vmin,
            "vmax" => Self::Vmax,
            "pt" => Self::Pt,
            "pc" => Self::Pc,
            "in" => Self::In,
            "cm" => Self::Cm,
            "mm" => Self::Mm,
            "q" => Self::Q,
            _ => return None,
        })
    }
}

/// A length with a unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    /// Numeric value.
    pub value: f32,
    /// Unit.
    pub unit: LengthUnit,
}

impl Length {
    /// Construct a length.
    pub fn new(value: f32, unit: LengthUnit) -> Self {
        Self { value, unit }
    }

    /// A length in pixels.
    pub fn px(value: f32) -> Self {
        Self::new(value, LengthUnit::Px)
    }

    /// Parse a dimension token, or unitless `0`.
    pub fn parse(cv: &ComponentValue) -> Option<Length> {
        match cv {
            ComponentValue::Token(Token::Dimension { value, unit, .. }) => {
                Some(Length::new(*value, LengthUnit::from_name(unit)?))
            }
            ComponentValue::Token(Token::Number { value, .. }) if *value == 0.0 => {
                Some(Length::px(0.0))
            }
            _ => None,
        }
    }

    /// Convert to pixels if the unit is absolute; `None` for relative
    /// units, which need context to resolve.
    pub fn absolute_px(&self) -> Option<f32> {
        Some(match self.unit {
            LengthUnit::Px => self.value,
            LengthUnit::Pt => self.value * 96.0 / 72.0,
            LengthUnit::Pc => self.value * 16.0,
            LengthUnit::In => self.value * 96.0,
            LengthUnit::Cm => self.value * 96.0 / 2.54,
            LengthUnit::Mm => self.value * 96.0 / 25.4,
            LengthUnit::Q => self.value * 96.0 / 101.6,
            _ => return None,
        })
    }
}

/// A length or a percentage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthPercentage {
    /// A length.
    Length(Length),
    /// A percentage (`50.0` for `50%`); what it is relative to depends on
    /// the property.
    Percentage(f32),
}

impl LengthPercentage {
    /// Parse a dimension, percentage, or unitless `0`.
    pub fn parse(cv: &ComponentValue) -> Option<Self> {
        if let ComponentValue::Token(Token::Percentage(p)) = cv {
            return Some(Self::Percentage(*p));
        }
        Length::parse(cv).map(Self::Length)
    }
}

/// An sRGB color with 8-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha; 255 is opaque.
    pub a: u8,
}

/// A color value as specified: either a concrete color or `currentcolor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorValue {
    /// A concrete color.
    Color(Color),
    /// `currentcolor`: resolves to the element's `color` property.
    CurrentColor,
}

impl Color {
    /// Fully transparent black.
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    /// Opaque color from channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Color from channels and alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parse a single component value as a color.
    pub fn parse(cv: &ComponentValue) -> Option<ColorValue> {
        match cv {
            ComponentValue::Token(Token::Hash { value, .. }) => {
                Color::from_hex(value).map(ColorValue::Color)
            }
            ComponentValue::Token(Token::Ident(name)) => {
                let lower = name.to_ascii_lowercase();
                match lower.as_str() {
                    "transparent" => Some(ColorValue::Color(Color::TRANSPARENT)),
                    "currentcolor" => Some(ColorValue::CurrentColor),
                    _ => Color::from_name(&lower).map(ColorValue::Color),
                }
            }
            ComponentValue::Function { name, args } => {
                let args: Vec<&ComponentValue> =
                    args.iter().filter(|a| !a.is_whitespace()).collect();
                match name.to_ascii_lowercase().as_str() {
                    "rgb" | "rgba" => parse_rgb(&args).map(ColorValue::Color),
                    "hsl" | "hsla" => parse_hsl(&args).map(ColorValue::Color),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Parse `rgb`, `rgba`, `rrggbb`, or `rrggbbaa` hex digits (no `#`).
    pub fn from_hex(hex: &str) -> Option<Color> {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()?;
        Some(match digits.len() {
            3 => Color::rgb(digits[0] * 17, digits[1] * 17, digits[2] * 17),
            4 => Color::rgba(
                digits[0] * 17,
                digits[1] * 17,
                digits[2] * 17,
                digits[3] * 17,
            ),
            6 => Color::rgb(
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
            ),
            8 => Color::rgba(
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
                digits[6] * 16 + digits[7],
            ),
            _ => return None,
        })
    }

    /// Look up a CSS named color (lowercase).
    pub fn from_name(name: &str) -> Option<Color> {
        NAMED_COLORS
            .binary_search_by_key(&name, |(n, _)| n)
            .ok()
            .map(|i| {
                let v = NAMED_COLORS[i].1;
                Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
            })
    }
}

/// Split function arguments into channels: either legacy comma-separated
/// `a, b, c[, alpha]` or modern `a b c [/ alpha]`.
fn split_channels<'a>(
    args: &[&'a ComponentValue],
) -> Option<(Vec<&'a ComponentValue>, Option<&'a ComponentValue>)> {
    let has_comma = args
        .iter()
        .any(|a| matches!(a, ComponentValue::Token(Token::Comma)));
    if has_comma {
        let parts: Vec<&[&ComponentValue]> = args
            .split(|a| matches!(a, ComponentValue::Token(Token::Comma)))
            .collect();
        if !(3..=4).contains(&parts.len()) || parts.iter().any(|p| p.len() != 1) {
            return None;
        }
        let channels = parts[..3].iter().map(|p| p[0]).collect();
        let alpha = parts.get(3).map(|p| p[0]);
        return Some((channels, alpha));
    }
    match args.len() {
        3 => Some((args.to_vec(), None)),
        5 if matches!(args[3], ComponentValue::Token(Token::Delim('/'))) => {
            Some((args[..3].to_vec(), Some(args[4])))
        }
        _ => None,
    }
}

fn clamp_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn parse_alpha(cv: Option<&ComponentValue>) -> Option<u8> {
    Some(match cv {
        None => 255,
        Some(ComponentValue::Token(Token::Number { value, .. })) => clamp_u8(value * 255.0),
        Some(ComponentValue::Token(Token::Percentage(p))) => clamp_u8(p * 255.0 / 100.0),
        Some(cv)
            if cv
                .as_ident()
                .is_some_and(|s| s.eq_ignore_ascii_case("none")) =>
        {
            0
        }
        _ => return None,
    })
}

fn parse_rgb(args: &[&ComponentValue]) -> Option<Color> {
    let (channels, alpha) = split_channels(args)?;
    let channel = |cv: &ComponentValue| -> Option<u8> {
        Some(match cv {
            ComponentValue::Token(Token::Number { value, .. }) => clamp_u8(*value),
            ComponentValue::Token(Token::Percentage(p)) => clamp_u8(p * 255.0 / 100.0),
            cv if cv
                .as_ident()
                .is_some_and(|s| s.eq_ignore_ascii_case("none")) =>
            {
                0
            }
            _ => return None,
        })
    };
    Some(Color::rgba(
        channel(channels[0])?,
        channel(channels[1])?,
        channel(channels[2])?,
        parse_alpha(alpha)?,
    ))
}

fn parse_hsl(args: &[&ComponentValue]) -> Option<Color> {
    let (channels, alpha) = split_channels(args)?;
    let hue = match channels[0] {
        ComponentValue::Token(Token::Number { value, .. }) => *value,
        ComponentValue::Token(Token::Dimension { value, unit, .. }) => {
            match unit.to_ascii_lowercase().as_str() {
                "deg" => *value,
                "grad" => value * 360.0 / 400.0,
                "rad" => value.to_degrees(),
                "turn" => value * 360.0,
                _ => return None,
            }
        }
        _ => return None,
    };
    let pct = |cv: &ComponentValue| -> Option<f32> {
        Some(match cv {
            ComponentValue::Token(Token::Percentage(p)) => p / 100.0,
            ComponentValue::Token(Token::Number { value, .. }) => value / 100.0,
            cv if cv
                .as_ident()
                .is_some_and(|s| s.eq_ignore_ascii_case("none")) =>
            {
                0.0
            }
            _ => return None,
        })
    };
    let s = pct(channels[1])?.clamp(0.0, 1.0);
    let l = pct(channels[2])?.clamp(0.0, 1.0);
    let (r, g, b) = hsl_to_rgb(hue.rem_euclid(360.0), s, l);
    Some(Color::rgba(
        clamp_u8(r * 255.0),
        clamp_u8(g * 255.0),
        clamp_u8(b * 255.0),
        parse_alpha(alpha)?,
    ))
}

/// CSS Color 4 §7.1 reference algorithm. `h` in degrees, `s`/`l` in 0..=1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let f = |n: f32| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    (f(0.0), f(8.0), f(4.0))
}

/// CSS named colors, sorted by name for binary search. Values are
/// `0xRRGGBB`.
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkslategrey", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("grey", 0x808080),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90),
    ("lightgrey", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::component_values;

    fn cv(src: &str) -> ComponentValue {
        let mut v = component_values(src);
        assert_eq!(v.len(), 1, "{src:?}");
        v.remove(0)
    }

    fn color(src: &str) -> Option<ColorValue> {
        Color::parse(&cv(src))
    }

    fn rgb(r: u8, g: u8, b: u8) -> Option<ColorValue> {
        Some(ColorValue::Color(Color::rgb(r, g, b)))
    }

    fn rgba(r: u8, g: u8, b: u8, a: u8) -> Option<ColorValue> {
        Some(ColorValue::Color(Color::rgba(r, g, b, a)))
    }

    #[test]
    fn named_table_is_sorted() {
        for w in NAMED_COLORS.windows(2) {
            assert!(w[0].0 < w[1].0, "{} should sort before {}", w[0].0, w[1].0);
        }
        assert_eq!(NAMED_COLORS.len(), 148);
    }

    #[test]
    fn hex_colors() {
        assert_eq!(color("#fff"), rgb(255, 255, 255));
        assert_eq!(color("#F00"), rgb(255, 0, 0));
        assert_eq!(color("#1234"), rgba(0x11, 0x22, 0x33, 0x44));
        assert_eq!(color("#102030"), rgb(0x10, 0x20, 0x30));
        assert_eq!(color("#10203080"), rgba(0x10, 0x20, 0x30, 0x80));
        assert_eq!(color("#12"), None);
        assert_eq!(color("#ggg"), None);
    }

    #[test]
    fn named_and_keyword_colors() {
        assert_eq!(color("red"), rgb(255, 0, 0));
        assert_eq!(color("RebeccaPurple"), rgb(0x66, 0x33, 0x99));
        assert_eq!(
            color("transparent"),
            Some(ColorValue::Color(Color::TRANSPARENT))
        );
        assert_eq!(color("currentColor"), Some(ColorValue::CurrentColor));
        assert_eq!(color("notacolor"), None);
    }

    #[test]
    fn rgb_functions() {
        assert_eq!(color("rgb(1, 2, 3)"), rgb(1, 2, 3));
        assert_eq!(color("rgb(1 2 3)"), rgb(1, 2, 3));
        assert_eq!(color("rgba(1, 2, 3, 0.5)"), rgba(1, 2, 3, 128));
        assert_eq!(color("rgb(1 2 3 / 50%)"), rgba(1, 2, 3, 128));
        assert_eq!(color("rgb(100%, 0%, 50%)"), rgb(255, 0, 128));
        assert_eq!(color("rgb(300, -5, 0)"), rgb(255, 0, 0));
        assert_eq!(color("rgb(1, 2)"), None);
        assert_eq!(color("rgb(1 2 3 4)"), None);
    }

    #[test]
    fn hsl_functions() {
        assert_eq!(color("hsl(0, 100%, 50%)"), rgb(255, 0, 0));
        assert_eq!(color("hsl(120deg 100% 50%)"), rgb(0, 255, 0));
        assert_eq!(color("hsl(240 100% 50%)"), rgb(0, 0, 255));
        assert_eq!(color("hsl(0.5turn, 100%, 50%)"), rgb(0, 255, 255));
        assert_eq!(color("hsl(0, 0%, 50%)"), rgb(128, 128, 128));
        assert_eq!(color("hsla(0, 100%, 50%, 0)"), rgba(255, 0, 0, 0));
    }

    #[test]
    fn lengths() {
        assert_eq!(Length::parse(&cv("10px")), Some(Length::px(10.0)));
        assert_eq!(
            Length::parse(&cv("1.5EM")),
            Some(Length::new(1.5, LengthUnit::Em))
        );
        assert_eq!(Length::parse(&cv("0")), Some(Length::px(0.0)));
        assert_eq!(Length::parse(&cv("5")), None);
        assert_eq!(Length::parse(&cv("5furlongs")), None);
        assert_eq!(Length::parse(&cv("50%")), None);
        assert_eq!(
            LengthPercentage::parse(&cv("50%")),
            Some(LengthPercentage::Percentage(50.0))
        );
    }

    #[test]
    fn absolute_units() {
        assert_eq!(Length::new(1.0, LengthUnit::In).absolute_px(), Some(96.0));
        assert_eq!(Length::new(72.0, LengthUnit::Pt).absolute_px(), Some(96.0));
        assert_eq!(Length::new(2.54, LengthUnit::Cm).absolute_px(), Some(96.0));
        assert_eq!(Length::new(1.0, LengthUnit::Em).absolute_px(), None);
    }
}
