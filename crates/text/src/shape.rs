//! Text shaping: characters to positioned glyphs.

use crate::context::{CACHE_LIMIT, FontContext, FontId, lock, sanitize_size, scale};

/// The glyphs for a run of text in one font at one size.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapedRun {
    /// Glyphs in visual (left-to-right drawing) order.
    pub glyphs: Vec<Glyph>,
    /// Total horizontal advance, in pixels: the sum of the glyph advances.
    pub advance: f32,
}

/// One positioned glyph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// Glyph index in the font. 0 is `.notdef`, used for characters the
    /// font lacks.
    pub id: u16,
    /// Horizontal shift from the pen position when drawing, in pixels.
    pub x_offset: f32,
    /// Vertical shift from the baseline when drawing, in pixels; positive
    /// is up.
    pub y_offset: f32,
    /// How far the pen moves right after this glyph, in pixels.
    pub advance: f32,
    /// Byte index in the source text of the start of the cluster this
    /// glyph belongs to. Several glyphs can share a cluster (combining
    /// marks) and one glyph can cover several characters (ligatures).
    pub cluster: usize,
}

impl FontContext {
    /// Shape `text` with `font` at `size_px`.
    ///
    /// Kerning and standard ligatures are applied. Script and direction are
    /// guessed from the text; right-to-left text comes back in visual order
    /// with descending clusters. Results are cached.
    pub fn shape(&self, font: FontId, size_px: f32, text: &str) -> ShapedRun {
        let size = sanitize_size(size_px);
        if text.is_empty() {
            return ShapedRun::default();
        }
        let key = (font, size.to_bits());
        if let Some(run) = lock(&self.shape_cache).get(&key).and_then(|m| m.get(text)) {
            return run.clone();
        }

        let run = self.with_face(font, |face| shape_with(face, size, text));

        let mut cache = lock(&self.shape_cache);
        let runs = cache.entry(key).or_default();
        if runs.len() >= CACHE_LIMIT {
            runs.clear();
        }
        runs.insert(text.to_owned(), run.clone());
        run
    }
}

fn shape_with(face: ttf_parser::Face<'_>, size: f32, text: &str) -> ShapedRun {
    let s = scale(&face, size);
    let face = rustybuzz::Face::from_face(face);
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let output = rustybuzz::shape(&face, &[], buffer);

    let glyphs: Vec<Glyph> = output
        .glyph_infos()
        .iter()
        .zip(output.glyph_positions())
        .map(|(info, pos)| Glyph {
            id: u16::try_from(info.glyph_id).unwrap_or(0),
            x_offset: pos.x_offset as f32 * s,
            y_offset: pos.y_offset as f32 * s,
            advance: pos.x_advance as f32 * s,
            cluster: info.cluster as usize,
        })
        .collect();
    let advance = glyphs.iter().map(|g| g.advance).sum();
    ShapedRun { glyphs, advance }
}

#[cfg(test)]
mod tests {
    use crate::{FontContext, FontId, FontStyle};

    fn font(fonts: &FontContext, family: &str) -> FontId {
        fonts.select(&[family.to_owned()], 400, FontStyle::Normal)
    }

    #[test]
    fn hello() {
        let fonts = FontContext::new();
        let run = fonts.shape(font(&fonts, "sans-serif"), 16.0, "Hello");
        assert_eq!(run.glyphs.len(), 5);
        assert!(run.advance > 0.0);
        let clusters: Vec<usize> = run.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, [0, 1, 2, 3, 4]);
        assert!(run.glyphs.iter().all(|g| g.id != 0 && g.advance > 0.0));
        // Both `l`s are the same glyph.
        assert_eq!(run.glyphs[2].id, run.glyphs[3].id);
    }

    #[test]
    fn ligatures_may_merge_glyphs() {
        let fonts = FontContext::new();
        let run = fonts.shape(font(&fonts, "sans-serif"), 16.0, "ff");
        assert!((1..=2).contains(&run.glyphs.len()), "{run:?}");
        assert!(run.advance > 0.0);
    }

    #[test]
    fn advance_scales_with_size() {
        let fonts = FontContext::new();
        let f = font(&fonts, "sans-serif");
        let a = fonts.shape(f, 10.0, "Hello world").advance;
        let b = fonts.shape(f, 30.0, "Hello world").advance;
        assert!((b - 3.0 * a).abs() < 1e-3, "{a} {b}");
    }

    #[test]
    fn monospace_ascii_advances_are_equal() {
        let fonts = FontContext::new();
        let f = font(&fonts, "monospace");
        let expected = fonts.shape(f, 16.0, "M").advance;
        assert!(expected > 0.0);
        for c in ' '..='~' {
            let run = fonts.shape(f, 16.0, &c.to_string());
            assert_eq!(run.glyphs.len(), 1, "{c:?}");
            assert_eq!(run.advance, expected, "{c:?}");
        }
        let line = fonts.shape(f, 16.0, "iiii WWWW");
        assert!((line.advance - 9.0 * expected).abs() < 1e-3);
    }

    #[test]
    fn clusters_are_byte_indices() {
        let fonts = FontContext::new();
        let run = fonts.shape(font(&fonts, "sans-serif"), 16.0, "aé b");
        let clusters: Vec<usize> = run.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, [0, 1, 3, 4]);
    }

    #[test]
    fn right_to_left_is_visual_order() {
        let fonts = FontContext::new();
        let run = fonts.shape(font(&fonts, "sans-serif"), 16.0, "שלום");
        assert_eq!(run.glyphs.len(), 4);
        assert!(
            run.glyphs.windows(2).all(|w| w[0].cluster > w[1].cluster),
            "{run:?}"
        );
    }

    #[test]
    fn missing_characters_become_notdef() {
        let fonts = FontContext::new();
        let run = fonts.shape(font(&fonts, "sans-serif"), 16.0, "漢");
        assert_eq!(run.glyphs.len(), 1);
        assert_eq!(run.glyphs[0].id, 0);
        assert!(run.advance > 0.0);
    }

    #[test]
    fn empty_and_degenerate_input() {
        let fonts = FontContext::new();
        let f = font(&fonts, "sans-serif");
        assert_eq!(fonts.shape(f, 16.0, ""), Default::default());
        for size in [0.0, -5.0, f32::NAN, f32::INFINITY] {
            let run = fonts.shape(f, size, "abc");
            assert_eq!(run.advance, 0.0);
            assert!(run.glyphs.iter().all(|g| g.advance == 0.0));
        }
        let run = fonts.shape(f, 1e30, "abc");
        assert!(run.advance.is_finite() && run.advance > 0.0);
        let run = fonts.shape(f, 16.0, "\0\u{7}\t\n\r\u{200B}\u{FEFF}\u{10FFFF}");
        assert!(run.advance.is_finite());
    }

    #[test]
    fn repeated_shaping_is_cached_and_identical() {
        let fonts = FontContext::new();
        let f = font(&fonts, "sans-serif");
        let a = fonts.shape(f, 16.0, "cached");
        let b = fonts.shape(f, 16.0, "cached");
        assert_eq!(a, b);
    }
}
