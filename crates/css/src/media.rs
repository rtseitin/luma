//! Media query parsing: the Media Queries Level 3 subset of media types
//! plus `width`/`height` range features. Evaluation needs a viewport and
//! lives in `luma-style`.

use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::Token;
use crate::value::Length;

/// One query in a media query list.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaQuery {
    /// `not` prefix.
    pub negated: bool,
    /// Media type; `All` if omitted.
    pub media_type: MediaType,
    /// Features joined with `and`. All must match.
    pub features: Vec<MediaFeature>,
}

impl MediaQuery {
    /// `all`: matches everything.
    pub fn all() -> Self {
        Self {
            negated: false,
            media_type: MediaType::All,
            features: Vec::new(),
        }
    }

    /// `not all`: matches nothing. Used for unparseable queries.
    pub fn not_all() -> Self {
        Self {
            negated: true,
            ..Self::all()
        }
    }
}

/// Media types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    /// `all`
    All,
    /// `screen`
    Screen,
    /// `print`
    Print,
    /// Anything else (never matches).
    Other(String),
}

/// `min-`, `max-`, or exact comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    /// `min-width: X` means viewport >= X.
    Min,
    /// `max-width: X` means viewport <= X.
    Max,
    /// `width: X` means viewport == X.
    Exact,
}

/// A parenthesized media feature.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaFeature {
    /// `(width: L)`, `(min-width: L)`, `(max-width: L)`
    Width(Range, Length),
    /// `(height: L)`, `(min-height: L)`, `(max-height: L)`
    Height(Range, Length),
    /// Any feature we do not understand. Never matches.
    Unknown,
}

pub(crate) fn parse_media_query_list(items: &[ComponentValue]) -> Vec<MediaQuery> {
    if items.iter().all(ComponentValue::is_whitespace) {
        return vec![MediaQuery::all()];
    }
    items
        .split(|cv| matches!(cv, ComponentValue::Token(Token::Comma)))
        .map(|part| parse_media_query(part).unwrap_or_else(MediaQuery::not_all))
        .collect()
}

fn parse_media_query(items: &[ComponentValue]) -> Option<MediaQuery> {
    let items: Vec<&ComponentValue> = items.iter().filter(|cv| !cv.is_whitespace()).collect();
    let mut pos = 0;
    let mut negated = false;
    let mut media_type = MediaType::All;
    let mut features = Vec::new();

    let ident_at = |pos: usize| items.get(pos).and_then(|cv| cv.as_ident());

    match ident_at(pos).map(str::to_ascii_lowercase).as_deref() {
        Some("not") => {
            negated = true;
            pos += 1;
        }
        Some("only") => pos += 1,
        _ => {}
    }

    let mut explicit_type = false;
    if let Some(name) = ident_at(pos) {
        if name.eq_ignore_ascii_case("and") {
            return None;
        }
        media_type = match name.to_ascii_lowercase().as_str() {
            "all" => MediaType::All,
            "screen" => MediaType::Screen,
            "print" => MediaType::Print,
            other => MediaType::Other(other.to_owned()),
        };
        explicit_type = true;
        pos += 1;
    }

    while pos < items.len() {
        if explicit_type || !features.is_empty() {
            if !ident_at(pos).is_some_and(|s| s.eq_ignore_ascii_case("and")) {
                return None;
            }
            pos += 1;
        }
        let Some(ComponentValue::Block {
            kind: BlockKind::Paren,
            content,
        }) = items.get(pos)
        else {
            return None;
        };
        features.push(parse_feature(content)?);
        pos += 1;
    }

    if !explicit_type && features.is_empty() {
        return None;
    }

    Some(MediaQuery {
        negated,
        media_type,
        features,
    })
}

fn parse_feature(content: &[ComponentValue]) -> Option<MediaFeature> {
    let items: Vec<&ComponentValue> = content.iter().filter(|cv| !cv.is_whitespace()).collect();
    let name = items.first()?.as_ident()?.to_ascii_lowercase();
    if items.len() == 1 {
        // Boolean context, e.g. `(color)`. Not modelled.
        return Some(MediaFeature::Unknown);
    }
    if !matches!(items.get(1), Some(ComponentValue::Token(Token::Colon))) || items.len() != 3 {
        return None;
    }
    let (range, base) = if let Some(rest) = name.strip_prefix("min-") {
        (Range::Min, rest)
    } else if let Some(rest) = name.strip_prefix("max-") {
        (Range::Max, rest)
    } else {
        (Range::Exact, name.as_str())
    };
    Some(match base {
        "width" => MediaFeature::Width(range, Length::parse(items[2])?),
        "height" => MediaFeature::Height(range, Length::parse(items[2])?),
        _ => MediaFeature::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::component_values;
    use crate::value::LengthUnit;

    fn parse(src: &str) -> Vec<MediaQuery> {
        parse_media_query_list(&component_values(src))
    }

    #[test]
    fn empty_is_all() {
        assert_eq!(parse(""), [MediaQuery::all()]);
        assert_eq!(parse("  "), [MediaQuery::all()]);
    }

    #[test]
    fn types_and_features() {
        let q = parse("screen and (min-width: 600px) and (max-height: 50em)");
        assert_eq!(q.len(), 1);
        assert!(!q[0].negated);
        assert_eq!(q[0].media_type, MediaType::Screen);
        assert_eq!(
            q[0].features,
            [
                MediaFeature::Width(Range::Min, Length::new(600.0, LengthUnit::Px)),
                MediaFeature::Height(Range::Max, Length::new(50.0, LengthUnit::Em)),
            ]
        );
    }

    #[test]
    fn feature_only_and_not() {
        let q = parse("(width: 0), not print, only screen");
        assert_eq!(q.len(), 3);
        assert_eq!(q[0].media_type, MediaType::All);
        assert_eq!(q[0].features.len(), 1);
        assert!(q[1].negated);
        assert_eq!(q[1].media_type, MediaType::Print);
        assert_eq!(q[2].media_type, MediaType::Screen);
    }

    #[test]
    fn invalid_becomes_not_all() {
        assert_eq!(parse("and (min-width: 1px)"), [MediaQuery::not_all()]);
        assert_eq!(parse("screen (min-width: 1px)"), [MediaQuery::not_all()]);
        assert_eq!(parse("screen and"), [MediaQuery::not_all()]);
        assert_eq!(parse("(min-width 1px)"), [MediaQuery::not_all()]);
    }

    #[test]
    fn unknown_feature_is_kept_as_unknown() {
        let q = parse("(prefers-color-scheme: dark)");
        assert_eq!(q[0].features, [MediaFeature::Unknown]);
        let q = parse("(hover)");
        assert_eq!(q[0].features, [MediaFeature::Unknown]);
    }
}
