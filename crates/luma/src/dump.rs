//! Text dumps of pipeline stages, for debugging and as test oracles.
//!
//! The formats are meant to be stable and greppable: one line per item,
//! `key=value` pairs separated by single spaces, no spaces inside values
//! except within quoted font family names.

use std::fmt::Write;

use luma_dom::{Document, Element, NodeId};
use luma_style::{
    Color, ComputedStyle, Display, Edges, LengthPct, LineHeight, MaxSize, Size, StyleMap,
    TextDecoration, WhiteSpace,
};
use luma_text::FontContext;

/// One line per element, indented two spaces per tree depth, e.g.
///
/// ```text
/// <p class="x"> display=Block ... color=#222222 font-size=16 margin=[16,0,16,0] ...
/// ```
///
/// Edges whose four sides are equal are written as a single value.
/// Elements without a computed style are skipped.
pub fn style_dump(doc: &Document, styles: &StyleMap) -> String {
    let mut out = String::new();
    let mut stack: Vec<(NodeId, usize)> = vec![(doc.root(), 0)];
    while let Some((id, depth)) = stack.pop() {
        let child_depth = match (doc.element(id), styles.get(id)) {
            (Some(el), Some(style)) => {
                out.push_str(&"  ".repeat(depth));
                write_tag(&mut out, el);
                write_style(&mut out, style);
                out.push('\n');
                depth + 1
            }
            _ => depth,
        };
        let children: Vec<NodeId> = doc.children(id).collect();
        stack.extend(children.into_iter().rev().map(|c| (c, child_depth)));
    }
    out
}

/// One line per visible, non-blank text node, indented like
/// [`style_dump`]: the text, the face selected for it, and its measured
/// width when shaped as a single unbroken run, e.g.
///
/// ```text
/// "Hello, Luma" font="DejaVu Sans" weight=700 style=Normal size=32 glyphs=11 advance=219.42
/// ```
///
/// Text inside `display: none` elements is skipped. Collapsible whitespace
/// is squeezed to single spaces and trimmed before shaping, which
/// approximates (but is not) CSS white-space processing. `missing=N` is
/// appended when N characters had no glyph in the selected face.
pub fn text_dump(doc: &Document, styles: &StyleMap, fonts: &FontContext) -> String {
    let mut out = String::new();
    let mut stack: Vec<(NodeId, usize, bool)> = vec![(doc.root(), 0, false)];
    while let Some((id, depth, mut hidden)) = stack.pop() {
        let node = doc.get(id);
        let mut child_depth = depth;
        if node.is_element() {
            hidden |= styles.get(id).is_none_or(|s| s.display == Display::None);
            child_depth += 1;
        } else if let Some(text) = node.as_text()
            && !hidden
            && let Some(style) = styles.get(id)
        {
            write_text(&mut out, depth, text, style, fonts);
        }
        let children: Vec<NodeId> = doc.children(id).collect();
        stack.extend(children.into_iter().rev().map(|c| (c, child_depth, hidden)));
    }
    out
}

fn write_text(
    out: &mut String,
    depth: usize,
    text: &str,
    style: &ComputedStyle,
    fonts: &FontContext,
) {
    let text = match style.white_space {
        WhiteSpace::Pre | WhiteSpace::PreWrap => text.to_owned(),
        _ => text
            .split([' ', '\t', '\n', '\r', '\x0C'])
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    };
    if text.trim().is_empty() {
        return;
    }
    let font = fonts.select(&style.font_family, style.font_weight, style.font_style);
    let run = fonts.shape(font, style.font_size, &text);
    let face = fonts.face_info(font);

    const PREVIEW: usize = 50;
    let preview: String = if text.chars().count() > PREVIEW {
        text.chars().take(PREVIEW - 1).chain(['…']).collect()
    } else {
        text.clone()
    };
    out.push_str(&"  ".repeat(depth));
    let _ = write!(
        out,
        "{preview:?} font={} weight={} style={:?} size={} glyphs={} advance={}",
        font_family(std::slice::from_ref(&face.family)),
        face.weight,
        face.style,
        num(style.font_size),
        run.glyphs.len(),
        num(run.advance),
    );
    let missing = run.glyphs.iter().filter(|g| g.id == 0).count();
    if missing > 0 {
        let _ = write!(out, " missing={missing}");
    }
    out.push('\n');
}

/// `<name id="..." class="...">`, with only the attributes that identify
/// an element in a selector.
fn write_tag(out: &mut String, el: &Element) {
    out.push('<');
    out.push_str(&el.name);
    for attr in el
        .attrs
        .iter()
        .filter(|a| a.name == "id" || a.name == "class")
    {
        let _ = write!(
            out,
            " {}=\"{}\"",
            attr.name,
            attr.value.replace('"', "&quot;")
        );
    }
    out.push('>');
}

fn write_style(out: &mut String, s: &ComputedStyle) {
    let fields: [(&str, String); 22] = [
        ("display", format!("{:?}", s.display)),
        ("width", size(s.width)),
        ("height", size(s.height)),
        ("min-width", size(s.min_width)),
        ("min-height", size(s.min_height)),
        ("max-width", max_size(s.max_width)),
        ("max-height", max_size(s.max_height)),
        ("margin", edges(&s.margin, |v| size(*v))),
        ("padding", edges(&s.padding, |v| length_pct(*v))),
        ("border-width", edges(&s.border_width, |v| num(*v))),
        ("border-style", edges(&s.border_style, |v| format!("{v:?}"))),
        ("border-color", edges(&s.border_color, |v| color(*v))),
        ("color", color(s.color)),
        ("background-color", color(s.background_color)),
        ("font-size", num(s.font_size)),
        ("font-family", font_family(&s.font_family)),
        ("font-weight", s.font_weight.to_string()),
        ("font-style", format!("{:?}", s.font_style)),
        ("line-height", line_height(s.line_height)),
        ("text-align", format!("{:?}", s.text_align)),
        ("white-space", format!("{:?}", s.white_space)),
        ("text-decoration", text_decoration(s.text_decoration)),
    ];
    for (name, value) in fields {
        let _ = write!(out, " {name}={value}");
    }
}

/// A pixel value with at most two decimals and no trailing zeros.
fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

fn length_pct(v: LengthPct) -> String {
    match v {
        LengthPct::Px(px) => num(px),
        LengthPct::Percent(p) => format!("{}%", num(p)),
    }
}

fn size(v: Size) -> String {
    match v {
        Size::Auto => "auto".to_owned(),
        Size::Length(l) => length_pct(l),
    }
}

fn max_size(v: MaxSize) -> String {
    match v {
        MaxSize::None => "none".to_owned(),
        MaxSize::Length(l) => length_pct(l),
    }
}

fn edges<T: PartialEq>(e: &Edges<T>, f: impl Fn(&T) -> String) -> String {
    if e.top == e.right && e.top == e.bottom && e.top == e.left {
        f(&e.top)
    } else {
        format!(
            "[{},{},{},{}]",
            f(&e.top),
            f(&e.right),
            f(&e.bottom),
            f(&e.left)
        )
    }
}

/// `#rrggbb`, or `#rrggbbaa` when not fully opaque.
fn color(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
    }
}

fn font_family(families: &[String]) -> String {
    let names: Vec<String> = families
        .iter()
        .map(|f| {
            if f.chars().any(|c| c.is_whitespace() || c == ',') {
                format!("\"{f}\"")
            } else {
                f.clone()
            }
        })
        .collect();
    names.join(",")
}

fn line_height(v: LineHeight) -> String {
    match v {
        LineHeight::Normal => "normal".to_owned(),
        LineHeight::Number(n) => num(n),
        LineHeight::Px(px) => format!("{}px", num(px)),
    }
}

fn text_decoration(d: TextDecoration) -> String {
    let lines: Vec<&str> = [
        (d.underline, "underline"),
        (d.overline, "overline"),
        (d.line_through, "line-through"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    if lines.is_empty() {
        "none".to_owned()
    } else {
        lines.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luma_style::{Viewport, style_document};

    fn dump(html: &str) -> String {
        let doc = luma_html::parse(html);
        let styles = style_document(&doc, &[], Viewport::default());
        style_dump(&doc, &styles)
    }

    fn line_for<'a>(dump: &'a str, tag: &str) -> &'a str {
        dump.lines()
            .find(|l| l.trim_start().starts_with(tag))
            .unwrap_or_else(|| panic!("no line for {tag} in\n{dump}"))
    }

    #[test]
    fn one_indented_line_per_element() {
        let out = dump("<p id=a class='x y'>hi <b>there</b></p><!-- c -->");
        let tags: Vec<&str> = out.lines().map(|l| l.split_once('>').unwrap().0).collect();
        assert_eq!(
            tags,
            [
                "<html",
                "  <head",
                "  <body",
                "    <p id=\"a\" class=\"x y\"",
                "      <b",
            ]
        );
    }

    #[test]
    fn values() {
        let out = dump(
            r#"<p style="margin: 16px 0; padding: 10%; color: #222; width: 50%;
                       max-width: 300px; border: 2px solid red;
                       font-family: 'Times New Roman', serif; line-height: 1.5;
                       text-decoration: underline line-through;
                       background-color: rgba(0, 0, 255, 0.5)">x</p>"#,
        );
        let line = line_for(&out, "<p");
        for expected in [
            " display=Block ",
            " width=50% ",
            " height=auto ",
            " max-width=300 ",
            " max-height=none ",
            " margin=[16,0,16,0] ",
            " padding=10% ",
            " border-width=2 ",
            " border-style=Solid ",
            " border-color=#ff0000 ",
            " color=#222222 ",
            " background-color=#0000ff80 ",
            " font-size=16 ",
            " font-family=\"Times New Roman\",serif ",
            " line-height=1.5 ",
            " text-decoration=underline,line-through",
        ] {
            assert!(line.contains(expected), "{expected:?} not in {line}");
        }
    }

    #[test]
    fn fractional_pixels_are_rounded() {
        assert_eq!(num(16.0), "16");
        assert_eq!(num(13.333_333), "13.33");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(-2.25), "-2.25");
    }

    fn text(html: &str) -> String {
        let doc = luma_html::parse(html);
        let styles = style_document(&doc, &[], Viewport::default());
        text_dump(&doc, &styles, &FontContext::new())
    }

    #[test]
    fn text_lines_for_visible_text_only() {
        let out = text(
            "<title>Hidden title</title><style>p{}</style>
             <h1>Big   bold\n title</h1>
             <div style='display:none'><p>gone</p></div>
             <p>   </p>
             <p style='font-family: monospace'>mono <i>it</i></p>
             <p>漢字 ok</p>",
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4, "{out}");
        assert!(lines[0].starts_with("      \"Big bold title\" font=\"DejaVu Sans\" weight=700 "));
        assert!(lines[0].contains(" size=32 glyphs=14 "), "{}", lines[0]);
        assert!(lines[1].contains("\"mono\" font=\"DejaVu Sans Mono\" weight=400 style=Normal"));
        assert!(lines[2].starts_with("        \"it\" font=\"DejaVu Sans Mono\""));
        assert!(!lines[2].contains("style=Normal"), "{}", lines[2]);
        assert!(lines[3].ends_with(" missing=2"), "{}", lines[3]);
    }

    #[test]
    fn monospace_advance_is_exact() {
        let out = text("<pre style='font-size: 10px'>ab  cd</pre>");
        let fonts = FontContext::new();
        let mono = fonts.select(&["monospace".into()], 400, luma_style::FontStyle::Normal);
        let six_chars = num(fonts.shape(mono, 10.0, "a").advance * 6.0);
        let line = line_for(&out, "\"ab  cd\"");
        assert!(
            line.ends_with(&format!(" glyphs=6 advance={six_chars}")),
            "{line}"
        );
    }

    #[test]
    fn long_text_is_truncated() {
        let out = text(&format!("<p>{}</p>", "word ".repeat(40)));
        assert!(out.contains("…\" font="), "{out}");
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let html = "<div>".repeat(5_000);
        assert!(dump(&html).lines().count() > 1000);
    }
}
