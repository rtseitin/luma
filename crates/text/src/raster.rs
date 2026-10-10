//! Glyph rasterisation: outlines to anti-aliased coverage masks.

use tiny_skia::{FillRule, Mask, PathBuilder, Transform};

use crate::context::{CACHE_LIMIT, FontContext, FontId, lock, sanitize_size, scale};

/// Glyph images larger than this many pixels are not rendered.
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

/// A rendered glyph: an 8-bit coverage mask and where to put it.
///
/// To draw the glyph with its origin (pen position on the baseline) at
/// `(x, y)` in a y-down canvas, put the mask's top-left pixel at
/// `(x + left, y - top)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RasterGlyph {
    /// Mask width in pixels. Zero for glyphs with no outline, like spaces.
    pub width: u32,
    /// Mask height in pixels.
    pub height: u32,
    /// Offset of the mask's left edge from the glyph origin, rightwards.
    pub left: i32,
    /// Offset of the mask's top edge above the baseline, upwards.
    pub top: i32,
    /// Row-major coverage, `width * height` bytes; 0 is empty, 255 fully
    /// covered.
    pub coverage: Vec<u8>,
}

impl FontContext {
    /// Render glyph `glyph` of `font` at `size_px`. Results are cached.
    ///
    /// Glyphs without an outline (spaces, invalid ids, bitmap-only emoji)
    /// give an empty mask.
    pub fn rasterize(&self, font: FontId, size_px: f32, glyph: u16) -> RasterGlyph {
        let size = sanitize_size(size_px);
        let key = (font, size.to_bits(), glyph);
        if let Some(r) = lock(&self.raster_cache).get(&key) {
            return r.clone();
        }

        let raster = self
            .with_face(font, |face| rasterize_with(&face, size, glyph))
            .unwrap_or_default();

        let mut cache = lock(&self.raster_cache);
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, raster.clone());
        raster
    }
}

fn rasterize_with(face: &ttf_parser::Face<'_>, size: f32, glyph: u16) -> Option<RasterGlyph> {
    if size == 0.0 {
        return None;
    }
    let mut outline = Outline {
        path: PathBuilder::new(),
        scale: scale(face, size),
    };
    face.outline_glyph(ttf_parser::GlyphId(glyph), &mut outline)?;
    let path = outline.path.finish()?;

    let bounds = path.bounds();
    let left = bounds.left().floor();
    let top = bounds.top().floor();
    let width = (bounds.right().ceil() - left) as u32;
    let height = (bounds.bottom().ceil() - top) as u32;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    let mut mask = Mask::new(width, height)?;
    mask.fill_path(
        &path,
        FillRule::Winding,
        true,
        Transform::from_translate(-left, -top),
    );
    Some(RasterGlyph {
        width,
        height,
        left: left as i32,
        top: -top as i32,
        coverage: mask.data().to_vec(),
    })
}

/// Converts font-unit, y-up outlines to pixel, y-down paths.
struct Outline {
    path: PathBuilder,
    scale: f32,
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x * self.scale, -y * self.scale);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(x * self.scale, -y * self.scale);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let s = self.scale;
        self.path.quad_to(x1 * s, -y1 * s, x * s, -y * s);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let s = self.scale;
        self.path
            .cubic_to(x1 * s, -y1 * s, x2 * s, -y2 * s, x * s, -y * s);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

#[cfg(test)]
mod tests {
    use crate::{FontContext, FontId, FontStyle};

    fn glyph_for(fonts: &FontContext, font: FontId, text: &str) -> u16 {
        fonts.shape(font, 32.0, text).glyphs[0].id
    }

    fn sans(fonts: &FontContext) -> FontId {
        fonts.select(&["sans-serif".to_owned()], 400, FontStyle::Normal)
    }

    #[test]
    fn capital_h_at_32px() {
        let fonts = FontContext::new();
        let f = sans(&fonts);
        let g = fonts.rasterize(f, 32.0, glyph_for(&fonts, f, "H"));
        assert!((20..=40).contains(&g.height), "{}x{}", g.width, g.height);
        assert!((10..=40).contains(&g.width), "{}x{}", g.width, g.height);
        assert_eq!(g.coverage.len(), (g.width * g.height) as usize);
        assert!(g.coverage.contains(&255), "solid stems");
        assert!(g.coverage.contains(&0), "gap between stems");
        // Sits on the baseline, starting just right of the origin.
        assert!((20..=26).contains(&g.top), "top {}", g.top);
        assert!((0..=5).contains(&g.left), "left {}", g.left);
    }

    #[test]
    fn descender_extends_below_baseline() {
        let fonts = FontContext::new();
        let f = sans(&fonts);
        let g = fonts.rasterize(f, 32.0, glyph_for(&fonts, f, "g"));
        assert!(g.height as i32 > g.top, "top {} height {}", g.top, g.height);
    }

    #[test]
    fn space_and_bad_glyphs_are_empty() {
        let fonts = FontContext::new();
        let f = sans(&fonts);
        let space = fonts.rasterize(f, 32.0, glyph_for(&fonts, f, " "));
        assert_eq!((space.width, space.height), (0, 0));
        assert!(space.coverage.is_empty());
        assert_eq!(fonts.rasterize(f, 32.0, u16::MAX), Default::default());
        assert_eq!(fonts.rasterize(f, 0.0, 43), Default::default());
        assert_eq!(fonts.rasterize(f, f32::NAN, 43), Default::default());
    }

    #[test]
    fn huge_sizes_do_not_allocate_unbounded_masks() {
        let fonts = FontContext::new();
        let f = sans(&fonts);
        let g = fonts.rasterize(f, 1e30, glyph_for(&fonts, f, "H"));
        assert!(u64::from(g.width) * u64::from(g.height) <= super::MAX_PIXELS);
    }

    #[test]
    fn rasterising_is_cached_and_identical() {
        let fonts = FontContext::new();
        let f = sans(&fonts);
        let id = glyph_for(&fonts, f, "a");
        assert_eq!(fonts.rasterize(f, 18.0, id), fonts.rasterize(f, 18.0, id));
    }
}
