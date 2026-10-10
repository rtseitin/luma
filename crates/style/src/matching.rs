//! Selector matching against the DOM.

use luma_css::{AttrOp, AttrSelector, Combinator, Compound, PseudoClass, Selector, SimpleSelector};
use luma_dom::{Document, Element, NodeId};

/// Does `element` match `selector`?
///
/// Pseudo-elements are ignored: a selector with `::before` matches the
/// originating element.
pub fn matches_selector(doc: &Document, element: NodeId, selector: &Selector) -> bool {
    matches_from(doc, element, selector, 0)
}

fn matches_from(doc: &Document, node: NodeId, sel: &Selector, idx: usize) -> bool {
    if !matches_compound(doc, node, &sel.compounds[idx]) {
        return false;
    }
    if idx + 1 == sel.compounds.len() {
        return true;
    }
    match sel.combinators[idx] {
        Combinator::Child => {
            parent_element(doc, node).is_some_and(|p| matches_from(doc, p, sel, idx + 1))
        }
        Combinator::Descendant => {
            let mut cur = parent_element(doc, node);
            while let Some(p) = cur {
                if matches_from(doc, p, sel, idx + 1) {
                    return true;
                }
                cur = parent_element(doc, p);
            }
            false
        }
        Combinator::NextSibling => {
            prev_element_sibling(doc, node).is_some_and(|s| matches_from(doc, s, sel, idx + 1))
        }
        Combinator::SubsequentSibling => {
            let mut cur = prev_element_sibling(doc, node);
            while let Some(s) = cur {
                if matches_from(doc, s, sel, idx + 1) {
                    return true;
                }
                cur = prev_element_sibling(doc, s);
            }
            false
        }
    }
}

fn matches_compound(doc: &Document, node: NodeId, compound: &Compound) -> bool {
    let Some(el) = doc.element(node) else {
        return false;
    };
    compound
        .parts
        .iter()
        .all(|p| matches_simple(doc, node, el, p))
}

fn matches_simple(doc: &Document, node: NodeId, el: &Element, simple: &SimpleSelector) -> bool {
    match simple {
        SimpleSelector::Universal => true,
        SimpleSelector::Type(t) => el.name == *t,
        SimpleSelector::Id(id) => el.id() == Some(id.as_str()),
        SimpleSelector::Class(c) => el.has_class(c),
        SimpleSelector::Attribute(a) => matches_attr(el, a),
        SimpleSelector::PseudoClass(pc) => matches_pseudo(doc, node, el, pc),
    }
}

fn matches_attr(el: &Element, a: &AttrSelector) -> bool {
    let Some(actual) = el.attr(&a.name) else {
        return false;
    };
    let Some((op, expected)) = &a.op else {
        return true;
    };
    let (actual, expected) = if a.case_insensitive {
        (actual.to_ascii_lowercase(), expected.to_ascii_lowercase())
    } else {
        (actual.to_owned(), expected.clone())
    };
    match op {
        AttrOp::Equals => actual == expected,
        AttrOp::Includes => {
            !expected.is_empty() && actual.split_ascii_whitespace().any(|w| w == expected)
        }
        AttrOp::DashMatch => {
            actual == expected
                || (actual.starts_with(&expected) && actual[expected.len()..].starts_with('-'))
        }
        AttrOp::Prefix => !expected.is_empty() && actual.starts_with(&expected),
        AttrOp::Suffix => !expected.is_empty() && actual.ends_with(&expected),
        AttrOp::Substring => !expected.is_empty() && actual.contains(&expected),
    }
}

fn matches_pseudo(doc: &Document, node: NodeId, el: &Element, pc: &PseudoClass) -> bool {
    match pc {
        PseudoClass::Root => parent_element(doc, node).is_none(),
        PseudoClass::Empty => doc
            .children(node)
            .all(|c| !(doc[c].is_element() || doc[c].is_text())),
        PseudoClass::FirstChild => prev_element_sibling(doc, node).is_none(),
        PseudoClass::LastChild => next_element_sibling(doc, node).is_none(),
        PseudoClass::OnlyChild => {
            prev_element_sibling(doc, node).is_none() && next_element_sibling(doc, node).is_none()
        }
        PseudoClass::FirstOfType => sibling_index(doc, node, &el.name, false) == 1,
        PseudoClass::LastOfType => sibling_index(doc, node, &el.name, true) == 1,
        PseudoClass::OnlyOfType => {
            sibling_index(doc, node, &el.name, false) == 1
                && sibling_index(doc, node, &el.name, true) == 1
        }
        PseudoClass::NthChild(n) => n.matches(child_index(doc, node, false)),
        PseudoClass::NthLastChild(n) => n.matches(child_index(doc, node, true)),
        PseudoClass::NthOfType(n) => n.matches(sibling_index(doc, node, &el.name, false)),
        PseudoClass::NthLastOfType(n) => n.matches(sibling_index(doc, node, &el.name, true)),
        PseudoClass::Not(list) => !list.iter().any(|s| matches_selector(doc, node, s)),
        PseudoClass::Is(list) | PseudoClass::Where(list) => {
            list.iter().any(|s| matches_selector(doc, node, s))
        }
        PseudoClass::Link | PseudoClass::AnyLink => {
            matches!(el.name.as_str(), "a" | "area") && el.has_attr("href")
        }
        PseudoClass::Visited | PseudoClass::Hover | PseudoClass::Active | PseudoClass::Focus => {
            false
        }
        PseudoClass::Enabled => is_form_control(el) && !el.has_attr("disabled"),
        PseudoClass::Disabled => is_form_control(el) && el.has_attr("disabled"),
        PseudoClass::Checked => {
            (el.name == "input"
                && matches!(el.attr("type"), Some("checkbox" | "radio"))
                && el.has_attr("checked"))
                || (el.name == "option" && el.has_attr("selected"))
        }
    }
}

fn is_form_control(el: &Element) -> bool {
    matches!(
        el.name.as_str(),
        "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
    )
}

/// Parent, if it is an element (not the document root).
fn parent_element(doc: &Document, node: NodeId) -> Option<NodeId> {
    doc[node].parent().filter(|&p| doc[p].is_element())
}

fn prev_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = doc[node].prev_sibling();
    while let Some(s) = cur {
        if doc[s].is_element() {
            return Some(s);
        }
        cur = doc[s].prev_sibling();
    }
    None
}

fn next_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = doc[node].next_sibling();
    while let Some(s) = cur {
        if doc[s].is_element() {
            return Some(s);
        }
        cur = doc[s].next_sibling();
    }
    None
}

/// 1-based position among element siblings, counting from the start or
/// the end.
fn child_index(doc: &Document, node: NodeId, from_end: bool) -> usize {
    let step = if from_end {
        next_element_sibling
    } else {
        prev_element_sibling
    };
    let mut i = 1;
    let mut cur = step(doc, node);
    while let Some(s) = cur {
        i += 1;
        cur = step(doc, s);
    }
    i
}

/// 1-based position among element siblings with the same tag name.
fn sibling_index(doc: &Document, node: NodeId, name: &str, from_end: bool) -> usize {
    let step = if from_end {
        next_element_sibling
    } else {
        prev_element_sibling
    };
    let mut i = 1;
    let mut cur = step(doc, node);
    while let Some(s) = cur {
        if doc.element(s).is_some_and(|e| e.name == name) {
            i += 1;
        }
        cur = step(doc, s);
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the ids of elements matching `selector` in document order,
    /// rendered as `tag#id` for readability.
    fn query(html: &str, selector: &str) -> Vec<String> {
        let doc = luma_html::parse(html);
        let sels = luma_css::parse_selector_list(selector).expect("valid selector");
        doc.descendants(doc.root())
            .filter(|&id| sels.iter().any(|s| matches_selector(&doc, id, s)))
            .map(|id| {
                let el = doc.element(id).unwrap();
                match el.id() {
                    Some(i) => format!("{}#{i}", el.name),
                    None => el.name.clone(),
                }
            })
            .collect()
    }

    const DOC: &str = r#"
        <div id=a class="x y">
          <p id=b class=x>one</p>
          <!-- comment -->
          <p id=c>two <span id=d>three</span></p>
          <ul id=e>
            <li id=f>1</li><li id=g class=x>2</li><li id=h>3</li><li id=i>4</li>
          </ul>
          <a id=j href="/page">link</a>
          <a id=k>anchor</a>
          <input id=l type=checkbox checked>
          <input id=m disabled>
          <p id=n></p>
        </div>"#;

    #[test]
    fn type_class_id_universal() {
        assert_eq!(query(DOC, "p"), ["p#b", "p#c", "p#n"]);
        assert_eq!(
            query(DOC, "P"),
            ["p#b", "p#c", "p#n"],
            "type selectors are case-insensitive"
        );
        assert_eq!(query(DOC, ".x"), ["div#a", "p#b", "li#g"]);
        assert_eq!(query(DOC, ".y"), ["div#a"]);
        assert_eq!(
            query(DOC, ".X"),
            Vec::<String>::new(),
            "classes are case-sensitive"
        );
        assert_eq!(query(DOC, "#d"), ["span#d"]);
        assert_eq!(query(DOC, "p.x"), ["p#b"]);
        assert_eq!(query(DOC, "*").len(), 17);
    }

    #[test]
    fn combinators() {
        assert_eq!(query(DOC, "div span"), ["span#d"]);
        assert_eq!(query(DOC, "div > span"), Vec::<String>::new());
        assert_eq!(query(DOC, "p > span"), ["span#d"]);
        assert_eq!(query(DOC, "html span"), ["span#d"]);
        assert_eq!(
            query(DOC, "p + p"),
            ["p#c"],
            "comments between siblings are skipped"
        );
        assert_eq!(query(DOC, "ul ~ p"), ["p#n"]);
        assert_eq!(query(DOC, "ul + p"), Vec::<String>::new());
        assert_eq!(
            query(DOC, "body > div > ul > li"),
            ["li#f", "li#g", "li#h", "li#i"]
        );
    }

    #[test]
    fn attributes() {
        assert_eq!(query(DOC, "[href]"), ["a#j"]);
        assert_eq!(query(DOC, "[class=x]"), ["p#b", "li#g"]);
        assert_eq!(query(DOC, "[class~=y]"), ["div#a"]);
        assert_eq!(query(DOC, "[href^='/']"), ["a#j"]);
        assert_eq!(query(DOC, "[href$=age]"), ["a#j"]);
        assert_eq!(query(DOC, "[href*=pag]"), ["a#j"]);
        assert_eq!(query(DOC, "[type=CHECKBOX]"), Vec::<String>::new());
        assert_eq!(query(DOC, "[type=CHECKBOX i]"), ["input#l"]);
        assert_eq!(query("<p lang=en-US>", "[lang|=en]"), ["p"]);
        assert_eq!(
            query("<p lang=english>", "[lang|=en]"),
            Vec::<String>::new()
        );
        assert_eq!(query("<p class=''>", "[class~='']"), Vec::<String>::new());
    }

    #[test]
    fn structural_pseudo_classes() {
        assert_eq!(query(DOC, ":root"), ["html"]);
        assert_eq!(query(DOC, "li:first-child"), ["li#f"]);
        assert_eq!(query(DOC, "li:last-child"), ["li#i"]);
        assert_eq!(query(DOC, "li:nth-child(2)"), ["li#g"]);
        assert_eq!(query(DOC, "li:nth-child(odd)"), ["li#f", "li#h"]);
        assert_eq!(query(DOC, "li:nth-child(-n+2)"), ["li#f", "li#g"]);
        assert_eq!(query(DOC, "li:nth-last-child(1)"), ["li#i"]);
        assert_eq!(query(DOC, "div > p:first-of-type"), ["p#b"]);
        assert_eq!(query(DOC, "div > p:last-of-type"), ["p#n"]);
        assert_eq!(query(DOC, "div > p:nth-of-type(2)"), ["p#c"]);
        assert_eq!(query(DOC, "div > a:nth-last-of-type(1)"), ["a#k"]);
        assert_eq!(query(DOC, "span:only-child"), ["span#d"]);
        assert_eq!(query(DOC, "ul:only-of-type"), ["ul#e"]);
        assert_eq!(query(DOC, "p:empty"), ["p#n"]);
        assert_eq!(
            query("<p> </p>", "p:empty"),
            Vec::<String>::new(),
            "whitespace is not empty"
        );
        assert_eq!(query("<p><!-- c --></p>", "p:empty"), ["p"]);
    }

    #[test]
    fn logical_pseudo_classes() {
        assert_eq!(query(DOC, "div > p:not(.x)"), ["p#c", "p#n"]);
        assert_eq!(
            query(DOC, "li:not(:first-child, :last-child)"),
            ["li#g", "li#h"]
        );
        assert_eq!(query(DOC, ":is(span, ul)"), ["span#d", "ul#e"]);
        assert_eq!(query(DOC, "li:where(.x)"), ["li#g"]);
    }

    #[test]
    fn link_and_form_pseudo_classes() {
        assert_eq!(query(DOC, ":link"), ["a#j"]);
        assert_eq!(query(DOC, "a:any-link"), ["a#j"]);
        assert_eq!(query(DOC, "a:hover"), Vec::<String>::new());
        assert_eq!(query(DOC, ":checked"), ["input#l"]);
        assert_eq!(query(DOC, ":disabled"), ["input#m"]);
        assert_eq!(query(DOC, "input:enabled"), ["input#l"]);
        assert_eq!(query(DOC, "p:enabled"), Vec::<String>::new());
    }

    #[test]
    fn pseudo_elements_match_originating_element() {
        assert_eq!(query(DOC, "span::before"), ["span#d"]);
    }
}
