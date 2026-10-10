//! Fonts for Luma: selection, metrics, shaping, and glyph rasterisation.
//!
//! Layout needs glyph advances to wrap text and paint needs glyph images
//! to draw it, so both go through one [`FontContext`]:
//!
//! - [`FontContext::select`] picks a face for a `font-family` list, weight,
//!   and style, following the [CSS Fonts Level 4 font matching
//!   algorithm][matching] (via `fontdb`): families are tried in order, and
//!   within a family the closest style, then the closest weight in the
//!   direction the spec prescribes, wins.
//! - [`FontContext::metrics`] reads ascent, descent, line gap, x-height,
//!   and underline position from the face's [OpenType][opentype] `hhea`,
//!   `OS/2`, and `post` tables (via `ttf-parser`).
//! - [`FontContext::shape`] turns a string into positioned glyphs with
//!   kerning and ligatures applied (via `rustybuzz`, a port of HarfBuzz).
//! - [`FontContext::rasterize`] renders one glyph outline to an
//!   anti-aliased coverage mask (via `tiny-skia`).
//!
//! Results of shaping and rasterising are cached per context.
//!
//! [matching]: https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm
//! [opentype]: https://learn.microsoft.com/en-us/typography/opentype/spec/
//!
//! # Bundled fonts
//!
//! [`FontContext::new`] uses only the DejaVu Sans and DejaVu Sans Mono
//! faces compiled into this crate, so measurements and pixels are identical
//! on every machine. The generic families `serif`, `sans-serif`,
//! `system-ui`, `cursive`, and `fantasy` map to DejaVu Sans, and
//! `monospace` maps to DejaVu Sans Mono. [`FontContext::with_system_fonts`]
//! adds the fonts installed on the system; the bundled faces remain as the
//! last fallback.
//!
//! # Simplifications
//!
//! - No per-character font fallback: a character missing from the selected
//!   face is drawn as that face's `.notdef` glyph.
//! - No synthetic bold or italic; the closest real face is used.
//! - `font-stretch`, font variations, and OpenType feature settings are
//!   not supported.
//! - Glyphs are not hinted.
//! - Text is shaped as a single run with direction and script guessed from
//!   its content; there is no bidi reordering of mixed-direction text.

mod context;
mod metrics;
mod raster;
mod shape;

pub use context::{FaceInfo, FontContext, FontId};
pub use luma_style::FontStyle;
pub use metrics::FontMetrics;
pub use raster::RasterGlyph;
pub use shape::{Glyph, ShapedRun};
