//! Style resolution for Luma: selector matching, the cascade, and computed
//! values.
//!
//! [`style_document`] takes a parsed [`Document`](luma_dom::Document) and
//! produces a [`StyleMap`] holding a [`ComputedStyle`] for every element
//! (and, for convenience, every text node, which shares its parent's
//! style). Layout reads from the map by [`NodeId`](luma_dom::NodeId); this
//! crate never modifies the DOM.
//!
//! The process per element follows [CSS Cascading and Inheritance Level
//! 4][cascade]:
//!
//! 1. Collect declarations from every rule whose selector matches, from
//!    the built-in user-agent stylesheet and from author stylesheets
//!    (`<style>` elements and `style` attributes).
//! 2. Sort by origin and importance, then specificity, then source order.
//! 3. Take the last value for each property as the cascaded value.
//! 4. Fill in missing properties by inheritance or initial value, resolve
//!    `inherit`/`initial`/`unset`, and compute: relative lengths become
//!    pixels, `currentcolor` becomes a color, keywords become numbers.
//!
//! [cascade]: https://www.w3.org/TR/css-cascade-4/
//!
//! # Scope
//!
//! The property set is what block layout and text rendering need:
//! `display`, box sizes and `min`/`max`, margins, padding, borders,
//! `color`, `background-color`, the `font-*` longhands, `line-height`,
//! `text-align`, `white-space`, and `text-decoration-line`, plus their
//! shorthands. Unknown properties and invalid values are ignored, as CSS
//! requires. `<link rel=stylesheet>` is not loaded here; pass externally
//! fetched sheets through [`style_document`]'s `extra` argument.

mod cascade;
mod computed;
mod matching;
mod properties;

pub use cascade::{StyleMap, Viewport, media_matches, style_document};
pub use computed::{
    BorderStyle, Color, ComputedStyle, DEFAULT_FONT_SIZE, Display, Edges, FontStyle, LengthPct,
    LineHeight, MaxSize, Size, TextAlign, TextDecoration, WhiteSpace,
};
pub use matching::matches_selector;
pub use properties::{Longhand, PropertyDeclaration, Value, parse_declaration};

#[cfg(test)]
mod tests {
    use super::*;
    use luma_dom::{Document, NodeId};

    struct Styled {
        doc: Document,
        map: StyleMap,
    }

    impl Styled {
        fn new(html: &str) -> Self {
            Self::with_viewport(html, Viewport::default())
        }

        fn with_viewport(html: &str, viewport: Viewport) -> Self {
            let doc = luma_html::parse(html);
            let map = style_document(&doc, &[], viewport);
            Self { doc, map }
        }

        fn id(&self, id: &str) -> NodeId {
            self.doc
                .descendants(self.doc.root())
                .find(|&n| self.doc.element(n).is_some_and(|e| e.id() == Some(id)))
                .unwrap_or_else(|| panic!("no element #{id}"))
        }

        fn style(&self, id: &str) -> &ComputedStyle {
            self.map.get(self.id(id)).expect("styled")
        }

        fn tag(&self, name: &str) -> &ComputedStyle {
            let n = self
                .doc
                .descendants(self.doc.root())
                .find(|&n| self.doc.element(n).is_some_and(|e| e.name == name))
                .unwrap_or_else(|| panic!("no <{name}>"));
            self.map.get(n).expect("styled")
        }
    }

    fn px(v: f32) -> Size {
        Size::Length(LengthPct::Px(v))
    }

    #[test]
    fn user_agent_defaults() {
        let s = Styled::new(
            "<p>a <b>b</b> <span>c</span></p><ul><li>x</li></ul><h1>t</h1><a href=x>l</a>",
        );
        assert_eq!(s.tag("html").display, Display::Block);
        assert_eq!(s.tag("head").display, Display::None);
        assert_eq!(s.tag("body").margin, Edges::all(px(8.0)));
        assert_eq!(s.tag("p").display, Display::Block);
        assert_eq!(s.tag("p").margin.top, px(16.0), "1em at 16px");
        assert_eq!(s.tag("span").display, Display::Inline);
        assert_eq!(s.tag("b").font_weight, 700, "bolder from 400");
        assert_eq!(s.tag("li").display, Display::ListItem);
        assert_eq!(s.tag("ul").padding.left, LengthPct::Px(40.0));
        assert_eq!(s.tag("h1").font_size, 32.0);
        assert_eq!(s.tag("h1").margin.top, px(32.0 * 0.67));
        assert_eq!(s.tag("a").color, Color::rgb(0, 0, 0xEE));
        assert!(s.tag("a").text_decoration.underline);
        assert_eq!(s.tag("body").font_size, DEFAULT_FONT_SIZE);
        assert_eq!(s.tag("body").color, Color::BLACK);
    }

    #[test]
    fn specificity_and_order() {
        let s = Styled::new(
            "<style>
                #a { color: red }
                .c { color: green }
                p { color: blue }
                p { color: purple }
                .c.d { color: orange }
            </style>
            <p id=a class='c d'>1</p>
            <p id=b class='c d'>2</p>
            <p id=e class=c>3</p>
            <p id=f>4</p>",
        );
        assert_eq!(s.style("a").color, Color::rgb(255, 0, 0), "id wins");
        assert_eq!(
            s.style("b").color,
            Color::rgb(255, 165, 0),
            "two classes beat one"
        );
        assert_eq!(
            s.style("e").color,
            Color::rgb(0, 128, 0),
            "class beats type"
        );
        assert_eq!(
            s.style("f").color,
            Color::rgb(128, 0, 128),
            "later rule wins"
        );
    }

    #[test]
    fn important_and_style_attribute() {
        let s = Styled::new(
            "<style>
                #a { color: red !important }
                #a { color: green }
                #b { color: red }
                #c { color: red !important }
            </style>
            <p id=a style='color: blue'>1</p>
            <p id=b style='color: blue'>2</p>
            <p id=c style='color: blue'>3</p>
            <p id=d style='color: blue !important; color: green'>4</p>",
        );
        assert_eq!(
            s.style("a").color,
            Color::rgb(255, 0, 0),
            "!important beats style attr"
        );
        assert_eq!(
            s.style("b").color,
            Color::rgb(0, 0, 255),
            "style attr beats id"
        );
        assert_eq!(s.style("c").color, Color::rgb(255, 0, 0));
        assert_eq!(
            s.style("d").color,
            Color::rgb(0, 0, 255),
            "!important in style attr"
        );
    }

    #[test]
    fn inheritance() {
        let s = Styled::new(
            "<div id=a style='color: red; margin: 10px; font-family: Foo; text-align: center'>
                <span id=b>x<i id=c>y</i></span>
                <p id=d style='margin: inherit; color: unset; text-align: initial'>z</p>
            </div>",
        );
        assert_eq!(s.style("b").color, Color::rgb(255, 0, 0));
        assert_eq!(s.style("c").color, Color::rgb(255, 0, 0));
        assert_eq!(s.style("c").font_family, ["Foo"]);
        assert_eq!(s.style("c").text_align, TextAlign::Center);
        assert_eq!(
            s.style("b").margin,
            Edges::all(px(0.0)),
            "margin does not inherit"
        );
        assert_eq!(
            s.style("d").margin,
            Edges::all(px(10.0)),
            "explicit inherit"
        );
        assert_eq!(
            s.style("d").color,
            Color::rgb(255, 0, 0),
            "unset on inherited = inherit"
        );
        assert_eq!(s.style("d").text_align, TextAlign::Start, "initial");
        assert_eq!(s.style("c").font_style, FontStyle::Italic);
    }

    #[test]
    fn relative_lengths() {
        let s = Styled::with_viewport(
            "<html style='font-size: 10px'>
             <div id=a style='font-size: 20px; padding: 1em; width: 50vw'>
                <p id=b style='font-size: 2em; margin: 1em; height: 2rem; line-height: 1.5'>
                  <span id=c style='font-size: 150%; line-height: 200%'>x</span>
                </p>
                <p id=d style='font-size: smaller'>y</p>
             </div>",
            Viewport {
                width: 800.0,
                height: 600.0,
            },
        );
        let a = s.style("a");
        assert_eq!(
            a.padding,
            Edges::all(LengthPct::Px(20.0)),
            "em uses own font size"
        );
        assert_eq!(a.width, px(400.0));
        let b = s.style("b");
        assert_eq!(b.font_size, 40.0, "em on font-size uses parent's");
        assert_eq!(b.margin, Edges::all(px(40.0)));
        assert_eq!(b.height, px(20.0), "rem uses root font size");
        assert_eq!(b.line_height, LineHeight::Number(1.5));
        assert_eq!(b.line_height_px(), 60.0);
        let c = s.style("c");
        assert_eq!(c.font_size, 60.0);
        assert_eq!(c.line_height, LineHeight::Px(120.0), "% of own font size");
        assert_eq!(s.style("d").font_size, 20.0 / 1.2);
    }

    #[test]
    fn borders_and_currentcolor() {
        let s = Styled::new(
            "<div id=a style='color: red; border: 2px solid'>
                <p id=b style='border-width: 5px'>no style, so zero</p>
                <p id=c style='border: 1px dashed blue; border-left-color: currentcolor'>x</p>
                <p id=d style='border: 4px solid; border-bottom-style: none'>y</p>
             </div>",
        );
        let a = s.style("a");
        assert_eq!(a.border_width, Edges::all(2.0));
        assert_eq!(a.border_color, Edges::all(Color::rgb(255, 0, 0)));
        assert_eq!(s.style("b").border_width, Edges::all(0.0));
        let c = s.style("c");
        assert_eq!(c.border_color.top, Color::rgb(0, 0, 255));
        assert_eq!(c.border_color.left, Color::rgb(255, 0, 0));
        let d = s.style("d");
        assert_eq!(d.border_width.top, 4.0);
        assert_eq!(d.border_width.bottom, 0.0);
        assert_eq!(d.border_style.bottom, BorderStyle::None);
    }

    #[test]
    fn media_queries() {
        let html = "<style>
            p { color: black }
            @media (min-width: 500px) { p { color: red } }
            @media print { p { color: green } }
            @media screen and (max-width: 400px), (min-height: 2000px) { p { color: blue } }
            @media not screen { p { color: purple } }
            @media (hover) { p { color: yellow } }
        </style><p id=a>x</p>";
        let wide = Styled::with_viewport(
            html,
            Viewport {
                width: 800.0,
                height: 600.0,
            },
        );
        assert_eq!(wide.style("a").color, Color::rgb(255, 0, 0));
        let narrow = Styled::with_viewport(
            html,
            Viewport {
                width: 300.0,
                height: 600.0,
            },
        );
        assert_eq!(narrow.style("a").color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn text_nodes_share_parent_style() {
        let s = Styled::new("<p id=a style='color: red'>text</p>");
        let p = s.id("a");
        let text = s.doc.children(p).next().unwrap();
        assert!(s.doc[text].is_text());
        assert_eq!(s.map.get(text).unwrap().color, Color::rgb(255, 0, 0));
        assert!(s.map.get(s.doc.root()).is_none());
    }

    #[test]
    fn invalid_css_is_ignored_not_fatal() {
        let s = Styled::new(
            "<style>
                p { color: red; color: notacolor; margin: 1px 2px 3px 4px 5px; bogus: 1 }
                p { width: 10px; width: red }
                q { { nested junk ; } color: red }
                r { color: red
                p { height: 5px }
            </style>
            <p id=a style='color: ; background: blue; junk'>x</p>",
        );
        let a = s.style("a");
        assert_eq!(
            a.color,
            Color::rgb(255, 0, 0),
            "invalid later value does not override"
        );
        assert_eq!(
            a.margin,
            Edges::all(px(16.0)) /* from UA */
                .map_vertical_only()
        );
        assert_eq!(a.width, px(10.0));
        // `r { color: red` is never closed, so the `p { height }` rule is
        // swallowed by its block, exactly as the spec (and browsers) do.
        assert_eq!(a.height, Size::Auto, "unclosed block swallows what follows");
        assert_eq!(a.background_color, Color::rgb(0, 0, 255));
    }

    trait MapVerticalOnly {
        fn map_vertical_only(self) -> Self;
    }
    impl MapVerticalOnly for Edges<Size> {
        /// UA gives `p` 1em top/bottom and 0 left/right.
        fn map_vertical_only(self) -> Self {
            Edges {
                top: self.top,
                bottom: self.bottom,
                left: px(0.0),
                right: px(0.0),
            }
        }
    }

    #[test]
    fn display_none_and_hidden_attribute() {
        let s = Styled::new("<p id=a hidden>x</p><p id=b style='display: none'>y</p><p id=c>z</p>");
        assert_eq!(s.style("a").display, Display::None);
        assert_eq!(s.style("b").display, Display::None);
        assert_eq!(s.style("c").display, Display::Block);
    }

    #[test]
    fn extra_stylesheets_apply_before_document_sheets() {
        let doc = luma_html::parse("<style>p { color: blue }</style><p id=a>x</p>");
        let external = luma_css::parse_stylesheet("p { color: red; margin: 0 }");
        let map = style_document(&doc, &[external], Viewport::default());
        let p = doc
            .descendants(doc.root())
            .find(|&n| doc.element(n).is_some_and(|e| e.name == "p"))
            .unwrap();
        let style = map.get(p).unwrap();
        assert_eq!(
            style.color,
            Color::rgb(0, 0, 255),
            "document sheet wins by order"
        );
        assert_eq!(
            style.margin,
            Edges::all(px(0.0)),
            "external sheet still applies"
        );
    }
}
