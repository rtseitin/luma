//! Random HTML + CSS soup must not panic, and every box must have a
//! finite, non-negative rectangle.

use luma_layout::layout;
use luma_style::{Viewport, style_document};
use luma_text::FontContext;

/// A tiny LCG so the crate does not depend on `rand`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }

    fn usize(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.usize(items.len())]
    }

    fn chance(&mut self, n: u32) -> bool {
        self.next().is_multiple_of(u64::from(n.max(1)))
    }
}

const TAGS: &[&str] = &[
    "div",
    "p",
    "span",
    "b",
    "i",
    "em",
    "strong",
    "a",
    "code",
    "pre",
    "ul",
    "ol",
    "li",
    "h1",
    "h2",
    "br",
    "blockquote",
];

const PROPS: &[&str] = &[
    "display:block",
    "display:inline",
    "display:inline-block",
    "display:none",
    "display:list-item",
    "display:flex",
    "width:50%",
    "width:20px",
    "width:auto",
    "width:200%",
    "height:10px",
    "height:50%",
    "min-width:30px",
    "max-width:80px",
    "min-height:5px",
    "max-height:40px",
    "margin:0",
    "margin:10px",
    "margin:0 auto",
    "margin:-8px 4px",
    "padding:4px",
    "padding:10%",
    "border:2px solid red",
    "border:1px dashed",
    "font-size:12px",
    "font-size:2em",
    "font-family:monospace",
    "font-weight:700",
    "font-style:italic",
    "line-height:1.2",
    "line-height:0",
    "text-align:center",
    "text-align:right",
    "text-align:justify",
    "white-space:pre",
    "white-space:nowrap",
    "white-space:pre-line",
    "white-space:pre-wrap",
];

const WORDS: &[&str] = &[
    "a",
    "aa",
    "word",
    "longerword",
    "x y z",
    "  spaces  ",
    "one\ntwo",
    "\t\ttab",
    "aaaaaaaaaaaaaaaaaaaa",
    "\u{200B}\u{FEFF}",
];

fn soup(rng: &mut Rng) -> String {
    let mut out = String::from("<style>");
    for _ in 0..rng.usize(4) {
        let _ = std::fmt::write(
            &mut out,
            format_args!("{} {{ {}; }}\n", rng.pick(TAGS), rng.pick(PROPS)),
        );
    }
    out.push_str("</style>");
    let depth = rng.usize(8);
    for _ in 0..depth {
        let tag = *rng.pick(TAGS);
        if tag == "br" {
            out.push_str("<br>");
            continue;
        }
        let style = if rng.chance(2) {
            format!(" style='{}'", rng.pick(PROPS))
        } else {
            String::new()
        };
        let _ = std::fmt::write(&mut out, format_args!("<{tag}{style}>"));
        if rng.chance(2) {
            out.push_str(rng.pick(WORDS));
        }
    }
    if rng.chance(3) {
        out.push_str(rng.pick(WORDS));
    }
    out
}

#[test]
fn random_soup_does_not_panic() {
    let fonts = FontContext::new();
    let viewport = Viewport {
        width: 320.0,
        height: 240.0,
    };
    for seed in 0..200u64 {
        let html = soup(&mut Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1));
        let doc = luma_html::parse(&html);
        let styles = style_document(&doc, &[], viewport);
        let tree = layout(&doc, &styles, &fonts, viewport);
        for (_, b) in tree.iter() {
            let r = b.content;
            assert!(
                r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite(),
                "seed {seed}: {r:?}\n{html}"
            );
            assert!(
                r.width >= 0.0 && r.height >= 0.0,
                "seed {seed}: {r:?}\n{html}"
            );
        }
        assert!(
            tree.document_height().is_finite(),
            "seed {seed}: height\n{html}"
        );
    }
}
