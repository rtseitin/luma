//! Discovery and loading of `<link rel=stylesheet>` sheets.
//!
//! Follows the [HTML `link` type "stylesheet"][spec] closely enough for
//! static pages: `rel` is a case-insensitive, whitespace-separated token
//! list; alternate stylesheets are not applied; a missing or empty `href`
//! is ignored. The `media`, `disabled`, and `type` attributes are ignored.
//!
//! [spec]: https://html.spec.whatwg.org/multipage/links.html#link-type-stylesheet

use luma_css::Stylesheet;
use luma_dom::{Document, NodeId};
use luma_loader::Url;

/// A linked stylesheet that loaded successfully.
pub struct LinkedSheet {
    /// Where the sheet was loaded from.
    pub url: Url,
    /// The parsed sheet.
    pub sheet: Stylesheet,
    /// How many CSS parse errors were recovered from.
    pub error_count: usize,
}

/// Whether a `rel` attribute value asks for a (non-alternate) stylesheet.
pub fn is_stylesheet_rel(rel: &str) -> bool {
    let mut stylesheet = false;
    for token in rel.split_ascii_whitespace() {
        if token.eq_ignore_ascii_case("alternate") {
            return false;
        }
        stylesheet |= token.eq_ignore_ascii_case("stylesheet");
    }
    stylesheet
}

/// The `href` of every stylesheet `<link>` in the document, in tree order.
pub fn stylesheet_links(doc: &Document) -> Vec<(NodeId, &str)> {
    doc.descendants(doc.root())
        .filter_map(|id| {
            let el = doc.element(id)?;
            if el.name != "link" || !is_stylesheet_rel(el.attr("rel")?) {
                return None;
            }
            let href = el.attr("href")?;
            (!href.trim().is_empty()).then_some((id, href))
        })
        .collect()
}

/// Resolve, load, and parse every linked stylesheet, in document order.
///
/// Sheets that fail to resolve or load are skipped; a human-readable
/// warning for each is returned alongside the sheets that did load.
pub fn load_linked_stylesheets(doc: &Document, base: &Url) -> (Vec<LinkedSheet>, Vec<String>) {
    let mut sheets = Vec::new();
    let mut warnings = Vec::new();
    for (_, href) in stylesheet_links(doc) {
        let Some(url) = luma_loader::resolve(base, href) else {
            warnings.push(format!("invalid stylesheet URL `{href}`"));
            continue;
        };
        match luma_loader::load(&url) {
            Ok(resource) => {
                let (sheet, errors) = luma_css::parse_stylesheet_with_errors(&resource.text());
                sheets.push(LinkedSheet {
                    url,
                    sheet,
                    error_count: errors.len(),
                });
            }
            Err(e) => warnings.push(format!("could not load stylesheet {url}: {e}")),
        }
    }
    (sheets, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_tokens() {
        assert!(is_stylesheet_rel("stylesheet"));
        assert!(is_stylesheet_rel("StyleSheet"));
        assert!(is_stylesheet_rel("  preload\tstylesheet\n"));
        assert!(!is_stylesheet_rel("alternate stylesheet"));
        assert!(!is_stylesheet_rel("stylesheet ALTERNATE"));
        assert!(!is_stylesheet_rel("stylesheets"));
        assert!(!is_stylesheet_rel("icon"));
        assert!(!is_stylesheet_rel(""));
    }

    #[test]
    fn links_in_document_order() {
        let doc = luma_html::parse(
            r#"<link rel=stylesheet href=a.css>
               <link rel=icon href=favicon.ico>
               <link rel="STYLESHEET" href="b.css">
               <link rel=stylesheet>
               <link rel=stylesheet href="  ">
               <link href=c.css>
               <body><link rel="stylesheet" href="d.css"></body>"#,
        );
        let hrefs: Vec<&str> = stylesheet_links(&doc).into_iter().map(|(_, h)| h).collect();
        assert_eq!(hrefs, ["a.css", "b.css", "d.css"]);
    }

    #[test]
    fn missing_sheets_are_warnings() {
        let doc = luma_html::parse(r#"<link rel=stylesheet href="nope.css">"#);
        let base = Url::parse("file:///definitely/not/here/index.html").unwrap();
        let (sheets, warnings) = load_linked_stylesheets(&doc, &base);
        assert!(sheets.is_empty());
        assert_eq!(
            warnings,
            ["could not load stylesheet file:///definitely/not/here/nope.css: not found"]
        );
    }
}
