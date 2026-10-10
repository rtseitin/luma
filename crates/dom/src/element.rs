/// A single `name="value"` pair on an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// Attribute name. The HTML parser lowercases these.
    pub name: String,
    /// Attribute value. Empty for boolean attributes like `<input disabled>`.
    pub value: String,
}

impl Attribute {
    /// Create an attribute.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// The payload of an element node.
///
/// Attributes are kept in a `Vec` in source order. Elements rarely carry
/// more than a handful, so a linear scan beats hashing and we keep the
/// order the author wrote them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// Local tag name, e.g. `div`. The HTML parser lowercases these. Unknown
    /// or made-up tag names are stored as-is; the DOM does not validate them.
    pub name: String,
    /// Attributes in source order.
    pub attrs: Vec<Attribute>,
}

impl Element {
    /// Create an element with the given tag name and attributes.
    pub fn new(name: impl Into<String>, attrs: Vec<Attribute>) -> Self {
        Self {
            name: name.into(),
            attrs,
        }
    }

    /// Value of the first attribute with this exact name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    /// Whether an attribute with this exact name is present.
    pub fn has_attr(&self, name: &str) -> bool {
        self.attrs.iter().any(|a| a.name == name)
    }

    /// Set an attribute, replacing the existing value if the name is already
    /// present, otherwise appending it.
    pub fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self.attrs.iter_mut().find(|a| a.name == name) {
            Some(a) => a.value = value,
            None => self.attrs.push(Attribute { name, value }),
        }
    }

    /// Remove an attribute by name. Returns the removed attribute, if any.
    pub fn remove_attr(&mut self, name: &str) -> Option<Attribute> {
        let idx = self.attrs.iter().position(|a| a.name == name)?;
        Some(self.attrs.remove(idx))
    }

    /// The `id` attribute.
    pub fn id(&self) -> Option<&str> {
        self.attr("id")
    }

    /// The whitespace-separated tokens of the `class` attribute.
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.attr("class").unwrap_or("").split_ascii_whitespace()
    }

    /// Whether the `class` attribute contains this token.
    pub fn has_class(&self, class: &str) -> bool {
        self.classes().any(|c| c == class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el() -> Element {
        Element::new(
            "div",
            vec![
                Attribute::new("id", "main"),
                Attribute::new("class", "  a b\tc\n"),
                Attribute::new("hidden", ""),
            ],
        )
    }

    #[test]
    fn attr_lookup() {
        let e = el();
        assert_eq!(e.attr("id"), Some("main"));
        assert_eq!(e.attr("hidden"), Some(""));
        assert_eq!(e.attr("missing"), None);
        assert!(e.has_attr("hidden"));
        assert!(
            !e.has_attr("ID"),
            "lookup is case-sensitive; parser lowercases"
        );
    }

    #[test]
    fn id_and_classes() {
        let e = el();
        assert_eq!(e.id(), Some("main"));
        assert_eq!(e.classes().collect::<Vec<_>>(), ["a", "b", "c"]);
        assert!(e.has_class("b"));
        assert!(!e.has_class("d"));
        assert_eq!(Element::new("p", vec![]).classes().count(), 0);
    }

    #[test]
    fn set_and_remove() {
        let mut e = el();
        e.set_attr("id", "other");
        assert_eq!(e.id(), Some("other"));
        assert_eq!(e.attrs.len(), 3, "replaced in place");
        e.set_attr("title", "t");
        assert_eq!(e.attrs.len(), 4);
        assert_eq!(e.remove_attr("title").map(|a| a.value), Some("t".into()));
        assert!(e.remove_attr("title").is_none());
    }
}
