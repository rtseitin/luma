//! Named character references.
//!
//! The full HTML table has 2231 entries. This is a practical subset: the
//! complete Latin-1 set (which is also the set that may legally appear
//! without a trailing semicolon), plus common punctuation, arrows,
//! mathematical symbols, and Greek letters. Unknown references are left in
//! the text verbatim, which is what the spec does for them anyway.

/// `(name, legacy, value)`. `legacy` entries match with or without a
/// trailing `;`; the rest require it.
const TABLE: &[(&str, bool, &str)] = &[
    // Latin-1 and the core four. All legacy.
    ("AElig", true, "\u{C6}"),
    ("AMP", true, "&"),
    ("Aacute", true, "\u{C1}"),
    ("Acirc", true, "\u{C2}"),
    ("Agrave", true, "\u{C0}"),
    ("Aring", true, "\u{C5}"),
    ("Atilde", true, "\u{C3}"),
    ("Auml", true, "\u{C4}"),
    ("COPY", true, "\u{A9}"),
    ("Ccedil", true, "\u{C7}"),
    ("ETH", true, "\u{D0}"),
    ("Eacute", true, "\u{C9}"),
    ("Ecirc", true, "\u{CA}"),
    ("Egrave", true, "\u{C8}"),
    ("Euml", true, "\u{CB}"),
    ("GT", true, ">"),
    ("Iacute", true, "\u{CD}"),
    ("Icirc", true, "\u{CE}"),
    ("Igrave", true, "\u{CC}"),
    ("Iuml", true, "\u{CF}"),
    ("LT", true, "<"),
    ("Ntilde", true, "\u{D1}"),
    ("Oacute", true, "\u{D3}"),
    ("Ocirc", true, "\u{D4}"),
    ("Ograve", true, "\u{D2}"),
    ("Oslash", true, "\u{D8}"),
    ("Otilde", true, "\u{D5}"),
    ("Ouml", true, "\u{D6}"),
    ("QUOT", true, "\""),
    ("REG", true, "\u{AE}"),
    ("THORN", true, "\u{DE}"),
    ("Uacute", true, "\u{DA}"),
    ("Ucirc", true, "\u{DB}"),
    ("Ugrave", true, "\u{D9}"),
    ("Uuml", true, "\u{DC}"),
    ("Yacute", true, "\u{DD}"),
    ("aacute", true, "\u{E1}"),
    ("acirc", true, "\u{E2}"),
    ("acute", true, "\u{B4}"),
    ("aelig", true, "\u{E6}"),
    ("agrave", true, "\u{E0}"),
    ("amp", true, "&"),
    ("aring", true, "\u{E5}"),
    ("atilde", true, "\u{E3}"),
    ("auml", true, "\u{E4}"),
    ("brvbar", true, "\u{A6}"),
    ("ccedil", true, "\u{E7}"),
    ("cedil", true, "\u{B8}"),
    ("cent", true, "\u{A2}"),
    ("copy", true, "\u{A9}"),
    ("curren", true, "\u{A4}"),
    ("deg", true, "\u{B0}"),
    ("divide", true, "\u{F7}"),
    ("eacute", true, "\u{E9}"),
    ("ecirc", true, "\u{EA}"),
    ("egrave", true, "\u{E8}"),
    ("eth", true, "\u{F0}"),
    ("euml", true, "\u{EB}"),
    ("frac12", true, "\u{BD}"),
    ("frac14", true, "\u{BC}"),
    ("frac34", true, "\u{BE}"),
    ("gt", true, ">"),
    ("iacute", true, "\u{ED}"),
    ("icirc", true, "\u{EE}"),
    ("iexcl", true, "\u{A1}"),
    ("igrave", true, "\u{EC}"),
    ("iquest", true, "\u{BF}"),
    ("iuml", true, "\u{EF}"),
    ("laquo", true, "\u{AB}"),
    ("lt", true, "<"),
    ("macr", true, "\u{AF}"),
    ("micro", true, "\u{B5}"),
    ("middot", true, "\u{B7}"),
    ("nbsp", true, "\u{A0}"),
    ("not", true, "\u{AC}"),
    ("ntilde", true, "\u{F1}"),
    ("oacute", true, "\u{F3}"),
    ("ocirc", true, "\u{F4}"),
    ("ograve", true, "\u{F2}"),
    ("ordf", true, "\u{AA}"),
    ("ordm", true, "\u{BA}"),
    ("oslash", true, "\u{F8}"),
    ("otilde", true, "\u{F5}"),
    ("ouml", true, "\u{F6}"),
    ("para", true, "\u{B6}"),
    ("plusmn", true, "\u{B1}"),
    ("pound", true, "\u{A3}"),
    ("quot", true, "\""),
    ("raquo", true, "\u{BB}"),
    ("reg", true, "\u{AE}"),
    ("sect", true, "\u{A7}"),
    ("shy", true, "\u{AD}"),
    ("sup1", true, "\u{B9}"),
    ("sup2", true, "\u{B2}"),
    ("sup3", true, "\u{B3}"),
    ("szlig", true, "\u{DF}"),
    ("thorn", true, "\u{FE}"),
    ("times", true, "\u{D7}"),
    ("uacute", true, "\u{FA}"),
    ("ucirc", true, "\u{FB}"),
    ("ugrave", true, "\u{F9}"),
    ("uml", true, "\u{A8}"),
    ("uuml", true, "\u{FC}"),
    ("yacute", true, "\u{FD}"),
    ("yen", true, "\u{A5}"),
    ("yuml", true, "\u{FF}"),
    // Punctuation and typography.
    ("apos", false, "'"),
    ("hellip", false, "\u{2026}"),
    ("mdash", false, "\u{2014}"),
    ("ndash", false, "\u{2013}"),
    ("lsquo", false, "\u{2018}"),
    ("rsquo", false, "\u{2019}"),
    ("sbquo", false, "\u{201A}"),
    ("ldquo", false, "\u{201C}"),
    ("rdquo", false, "\u{201D}"),
    ("bdquo", false, "\u{201E}"),
    ("lsaquo", false, "\u{2039}"),
    ("rsaquo", false, "\u{203A}"),
    ("dagger", false, "\u{2020}"),
    ("Dagger", false, "\u{2021}"),
    ("bull", false, "\u{2022}"),
    ("permil", false, "\u{2030}"),
    ("prime", false, "\u{2032}"),
    ("Prime", false, "\u{2033}"),
    ("oline", false, "\u{203E}"),
    ("frasl", false, "\u{2044}"),
    ("trade", false, "\u{2122}"),
    ("euro", false, "\u{20AC}"),
    ("ensp", false, "\u{2002}"),
    ("emsp", false, "\u{2003}"),
    ("thinsp", false, "\u{2009}"),
    ("zwnj", false, "\u{200C}"),
    ("zwj", false, "\u{200D}"),
    ("lrm", false, "\u{200E}"),
    ("rlm", false, "\u{200F}"),
    ("fnof", false, "\u{192}"),
    ("circ", false, "\u{2C6}"),
    ("tilde", false, "\u{2DC}"),
    ("OElig", false, "\u{152}"),
    ("oelig", false, "\u{153}"),
    ("Scaron", false, "\u{160}"),
    ("scaron", false, "\u{161}"),
    ("Yuml", false, "\u{178}"),
    // Arrows.
    ("larr", false, "\u{2190}"),
    ("uarr", false, "\u{2191}"),
    ("rarr", false, "\u{2192}"),
    ("darr", false, "\u{2193}"),
    ("harr", false, "\u{2194}"),
    ("crarr", false, "\u{21B5}"),
    ("lArr", false, "\u{21D0}"),
    ("uArr", false, "\u{21D1}"),
    ("rArr", false, "\u{21D2}"),
    ("dArr", false, "\u{21D3}"),
    ("hArr", false, "\u{21D4}"),
    // Mathematical.
    ("forall", false, "\u{2200}"),
    ("part", false, "\u{2202}"),
    ("exist", false, "\u{2203}"),
    ("empty", false, "\u{2205}"),
    ("nabla", false, "\u{2207}"),
    ("isin", false, "\u{2208}"),
    ("notin", false, "\u{2209}"),
    ("ni", false, "\u{220B}"),
    ("prod", false, "\u{220F}"),
    ("sum", false, "\u{2211}"),
    ("minus", false, "\u{2212}"),
    ("lowast", false, "\u{2217}"),
    ("radic", false, "\u{221A}"),
    ("prop", false, "\u{221D}"),
    ("infin", false, "\u{221E}"),
    ("ang", false, "\u{2220}"),
    ("and", false, "\u{2227}"),
    ("or", false, "\u{2228}"),
    ("cap", false, "\u{2229}"),
    ("cup", false, "\u{222A}"),
    ("int", false, "\u{222B}"),
    ("there4", false, "\u{2234}"),
    ("sim", false, "\u{223C}"),
    ("cong", false, "\u{2245}"),
    ("asymp", false, "\u{2248}"),
    ("ne", false, "\u{2260}"),
    ("equiv", false, "\u{2261}"),
    ("le", false, "\u{2264}"),
    ("ge", false, "\u{2265}"),
    ("sub", false, "\u{2282}"),
    ("sup", false, "\u{2283}"),
    ("nsub", false, "\u{2284}"),
    ("sube", false, "\u{2286}"),
    ("supe", false, "\u{2287}"),
    ("oplus", false, "\u{2295}"),
    ("otimes", false, "\u{2297}"),
    ("perp", false, "\u{22A5}"),
    ("sdot", false, "\u{22C5}"),
    ("lceil", false, "\u{2308}"),
    ("rceil", false, "\u{2309}"),
    ("lfloor", false, "\u{230A}"),
    ("rfloor", false, "\u{230B}"),
    ("lang", false, "\u{27E8}"),
    ("rang", false, "\u{27E9}"),
    ("loz", false, "\u{25CA}"),
    ("spades", false, "\u{2660}"),
    ("clubs", false, "\u{2663}"),
    ("hearts", false, "\u{2665}"),
    ("diams", false, "\u{2666}"),
    ("weierp", false, "\u{2118}"),
    ("image", false, "\u{2111}"),
    ("real", false, "\u{211C}"),
    ("alefsym", false, "\u{2135}"),
    ("check", false, "\u{2713}"),
    // Greek.
    ("Alpha", false, "\u{391}"),
    ("Beta", false, "\u{392}"),
    ("Gamma", false, "\u{393}"),
    ("Delta", false, "\u{394}"),
    ("Epsilon", false, "\u{395}"),
    ("Zeta", false, "\u{396}"),
    ("Eta", false, "\u{397}"),
    ("Theta", false, "\u{398}"),
    ("Iota", false, "\u{399}"),
    ("Kappa", false, "\u{39A}"),
    ("Lambda", false, "\u{39B}"),
    ("Mu", false, "\u{39C}"),
    ("Nu", false, "\u{39D}"),
    ("Xi", false, "\u{39E}"),
    ("Omicron", false, "\u{39F}"),
    ("Pi", false, "\u{3A0}"),
    ("Rho", false, "\u{3A1}"),
    ("Sigma", false, "\u{3A3}"),
    ("Tau", false, "\u{3A4}"),
    ("Upsilon", false, "\u{3A5}"),
    ("Phi", false, "\u{3A6}"),
    ("Chi", false, "\u{3A7}"),
    ("Psi", false, "\u{3A8}"),
    ("Omega", false, "\u{3A9}"),
    ("alpha", false, "\u{3B1}"),
    ("beta", false, "\u{3B2}"),
    ("gamma", false, "\u{3B3}"),
    ("delta", false, "\u{3B4}"),
    ("epsilon", false, "\u{3B5}"),
    ("zeta", false, "\u{3B6}"),
    ("eta", false, "\u{3B7}"),
    ("theta", false, "\u{3B8}"),
    ("iota", false, "\u{3B9}"),
    ("kappa", false, "\u{3BA}"),
    ("lambda", false, "\u{3BB}"),
    ("mu", false, "\u{3BC}"),
    ("nu", false, "\u{3BD}"),
    ("xi", false, "\u{3BE}"),
    ("omicron", false, "\u{3BF}"),
    ("pi", false, "\u{3C0}"),
    ("rho", false, "\u{3C1}"),
    ("sigmaf", false, "\u{3C2}"),
    ("sigma", false, "\u{3C3}"),
    ("tau", false, "\u{3C4}"),
    ("upsilon", false, "\u{3C5}"),
    ("phi", false, "\u{3C6}"),
    ("chi", false, "\u{3C7}"),
    ("psi", false, "\u{3C8}"),
    ("omega", false, "\u{3C9}"),
    ("thetasym", false, "\u{3D1}"),
    ("upsih", false, "\u{3D2}"),
    ("piv", false, "\u{3D6}"),
];

/// Find the longest named reference that is a prefix of `rest` (the input
/// immediately after the `&`).
///
/// Returns the number of characters consumed (including the `;` if one was
/// matched) and the replacement text. Following the spec, a name without a
/// `;` only matches if it is in the legacy set.
pub(crate) fn lookup(rest: &[char]) -> Option<(usize, &'static str)> {
    let mut best: Option<(usize, &'static str)> = None;
    for &(name, legacy, value) in TABLE {
        if !starts_with(rest, name) {
            continue;
        }
        let len = name.len();
        let consumed = if rest.get(len) == Some(&';') {
            len + 1
        } else if legacy {
            len
        } else {
            continue;
        };
        if best.is_none_or(|(l, _)| consumed > l) {
            best = Some((consumed, value));
        }
    }
    best
}

fn starts_with(chars: &[char], s: &str) -> bool {
    s.chars()
        .enumerate()
        .all(|(i, sc)| chars.get(i) == Some(&sc))
}

/// Replacement for numeric references in the C1 control range, which
/// browsers interpret as Windows-1252.
pub(crate) fn c1_remap(code: u32) -> Option<char> {
    Some(match code {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{2C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{160}',
        0x8B => '\u{2039}',
        0x8C => '\u{152}',
        0x8E => '\u{17D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{2DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{161}',
        0x9B => '\u{203A}',
        0x9C => '\u{153}',
        0x9E => '\u{17E}',
        0x9F => '\u{178}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn exact_with_semicolon() {
        assert_eq!(lookup(&chars("amp;x")), Some((4, "&")));
        assert_eq!(lookup(&chars("hellip;")), Some((7, "\u{2026}")));
    }

    #[test]
    fn legacy_without_semicolon() {
        assert_eq!(lookup(&chars("copy 2026")), Some((4, "\u{A9}")));
        assert_eq!(lookup(&chars("lt")), Some((2, "<")));
    }

    #[test]
    fn non_legacy_requires_semicolon() {
        assert_eq!(lookup(&chars("hellip x")), None);
    }

    #[test]
    fn longest_match_wins() {
        // "not" is legacy, "notin;" is longer.
        assert_eq!(lookup(&chars("notin;")), Some((6, "\u{2209}")));
        // Without the semicolon only "not" can match.
        assert_eq!(lookup(&chars("notin")), Some((3, "\u{AC}")));
        // "sup" needs ";", "sup2" is legacy.
        assert_eq!(lookup(&chars("sup2")), Some((4, "\u{B2}")));
    }

    #[test]
    fn unknown_is_none() {
        assert_eq!(lookup(&chars("bogus;")), None);
        assert_eq!(lookup(&chars("")), None);
    }
}
