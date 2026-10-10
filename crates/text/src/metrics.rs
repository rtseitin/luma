//! Vertical font metrics.

use crate::context::{FontContext, FontId, sanitize_size, scale};

/// Vertical metrics of a face at a given size, in pixels.
///
/// Distances are measured from the baseline. Following CSS, `ascent` is
/// upward and `descent` downward, and both are non-negative, so the
/// content area of a line of this font is `ascent + descent` tall.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FontMetrics {
    /// Distance from the baseline up to the top of the em box's content area.
    pub ascent: f32,
    /// Distance from the baseline down to the bottom of the content area.
    pub descent: f32,
    /// Extra spacing the font recommends between lines.
    pub line_gap: f32,
    /// Height of a lowercase `x` above the baseline.
    pub x_height: f32,
    /// Distance from the baseline down to the top of an underline.
    pub underline_offset: f32,
    /// Thickness of an underline.
    pub underline_thickness: f32,
}

impl FontContext {
    /// Metrics of `font` at `size_px`.
    ///
    /// Ascent, descent, and line gap come from `hhea` (or `OS/2` typo
    /// metrics when the font sets `USE_TYPO_METRICS`). Missing x-height and
    /// underline values are measured from the `x` glyph or estimated.
    pub fn metrics(&self, font: FontId, size_px: f32) -> FontMetrics {
        let size = sanitize_size(size_px);
        self.with_face(font, |face| {
            let s = scale(&face, size);
            let ascent = (f32::from(face.ascender()) * s).max(0.0);
            let descent = (-f32::from(face.descender()) * s).max(0.0);
            let line_gap = (f32::from(face.line_gap()) * s).max(0.0);
            let x_height = face
                .x_height()
                .filter(|&h| h > 0)
                .map(f32::from)
                .or_else(|| {
                    let x = face.glyph_index('x')?;
                    Some(f32::from(face.glyph_bounding_box(x)?.y_max))
                })
                .map_or(ascent * 0.5, |h| h * s);
            let (underline_offset, underline_thickness) = match face.underline_metrics() {
                Some(m) if m.thickness > 0 => {
                    (-f32::from(m.position) * s, f32::from(m.thickness) * s)
                }
                _ => (descent / 2.0, size / 16.0),
            };
            FontMetrics {
                ascent,
                descent,
                line_gap,
                x_height,
                underline_offset,
                underline_thickness,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{FontContext, FontStyle};

    fn sans(fonts: &FontContext) -> crate::FontId {
        fonts.select(&["sans-serif".to_owned()], 400, FontStyle::Normal)
    }

    #[test]
    fn dejavu_metrics_are_plausible() {
        let fonts = FontContext::new();
        let m = fonts.metrics(sans(&fonts), 16.0);
        assert!(m.ascent > 0.0, "{m:?}");
        assert!(m.descent >= 0.0, "{m:?}");
        let height = m.ascent + m.descent;
        assert!((16.0 * 1.1..=16.0 * 1.3).contains(&height), "{m:?}");
        assert!(m.x_height > 0.0 && m.x_height < m.ascent, "{m:?}");
        assert!(
            m.underline_offset > 0.0 && m.underline_offset < m.descent,
            "{m:?}"
        );
        assert!(
            m.underline_thickness > 0.0 && m.underline_thickness < 2.0,
            "{m:?}"
        );
    }

    #[test]
    fn metrics_scale_linearly() {
        let fonts = FontContext::new();
        let a = fonts.metrics(sans(&fonts), 10.0);
        let b = fonts.metrics(sans(&fonts), 20.0);
        assert!((b.ascent - 2.0 * a.ascent).abs() < 1e-3);
        assert!((b.descent - 2.0 * a.descent).abs() < 1e-3);
    }

    #[test]
    fn bad_sizes_give_zero_metrics() {
        let fonts = FontContext::new();
        for size in [0.0, -1.0, f32::NAN, f32::NEG_INFINITY] {
            let m = fonts.metrics(sans(&fonts), size);
            assert_eq!(m.ascent, 0.0);
            assert_eq!(m.descent, 0.0);
            assert!(m.x_height.is_finite() && m.underline_offset.is_finite());
        }
    }
}
