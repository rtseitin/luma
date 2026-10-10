//! The font database, face selection, and access to font data.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fontdb::{Database, Family, Query, Source, Stretch, Style, Weight};
use luma_style::FontStyle;

use crate::raster::RasterGlyph;
use crate::shape::ShapedRun;

const SANS: &str = "DejaVu Sans";
const MONO: &str = "DejaVu Sans Mono";

/// The bundled faces. The first is the fallback for everything, so it
/// must be DejaVu Sans regular.
static BUNDLED: [&[u8]; 8] = [
    include_bytes!("../assets/DejaVuSans.ttf"),
    include_bytes!("../assets/DejaVuSans-Bold.ttf"),
    include_bytes!("../assets/DejaVuSans-Oblique.ttf"),
    include_bytes!("../assets/DejaVuSans-BoldOblique.ttf"),
    include_bytes!("../assets/DejaVuSansMono.ttf"),
    include_bytes!("../assets/DejaVuSansMono-Bold.ttf"),
    include_bytes!("../assets/DejaVuSansMono-Oblique.ttf"),
    include_bytes!("../assets/DejaVuSansMono-BoldOblique.ttf"),
];

/// Font sizes are clamped to this many pixels.
const MAX_SIZE: f32 = 10_000.0;

/// A cache is cleared when it reaches this many entries, which bounds
/// memory on pathological pages without needing an eviction policy.
pub(crate) const CACHE_LIMIT: usize = 50_000;

/// A face in a [`FontContext`]. Only meaningful for the context that
/// returned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(u32);

impl FontId {
    /// Position of the face in its context, from 0.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Descriptive information about a face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceInfo {
    /// Family name, e.g. `DejaVu Sans`.
    pub family: String,
    /// Weight, 1..=1000.
    pub weight: u16,
    /// Slant.
    pub style: FontStyle,
    /// Whether every glyph has the same advance.
    pub monospace: bool,
}

/// Font bytes plus the index of the face within them (for collections).
#[derive(Clone)]
struct FontData {
    bytes: Bytes,
    index: u32,
}

#[derive(Clone)]
enum Bytes {
    Static(&'static [u8]),
    Shared(Arc<[u8]>),
}

impl FontData {
    const FALLBACK: FontData = FontData {
        bytes: Bytes::Static(BUNDLED[0]),
        index: 0,
    };

    fn as_slice(&self) -> &[u8] {
        match &self.bytes {
            Bytes::Static(b) => b,
            Bytes::Shared(b) => b,
        }
    }
}

struct Face {
    db_id: fontdb::ID,
    info: FaceInfo,
    bundled: Option<&'static [u8]>,
}

type SelectKey = (String, u16, u8);
type ShapeCache = HashMap<(FontId, u32), HashMap<String, ShapedRun>>;
type RasterCache = HashMap<(FontId, u32, u16), RasterGlyph>;

/// Loaded fonts plus caches of selection, shaping, and rasterising
/// results.
///
/// Every method takes `&self` and never fails: a face whose data cannot be
/// read or parsed is replaced by bundled DejaVu Sans.
pub struct FontContext {
    db: Database,
    faces: Vec<Face>,
    by_db_id: HashMap<fontdb::ID, FontId>,
    /// Lowercased family name → the name as stored in `db`.
    names: HashMap<String, String>,
    data: Mutex<Vec<Option<FontData>>>,
    select_cache: Mutex<HashMap<SelectKey, FontId>>,
    pub(crate) shape_cache: Mutex<ShapeCache>,
    pub(crate) raster_cache: Mutex<RasterCache>,
}

impl FontContext {
    /// A context with only the bundled fonts. Results are identical on
    /// every machine, which is what tests and snapshots need.
    pub fn new() -> Self {
        Self::build(false)
    }

    /// A context with the bundled fonts plus every font installed on the
    /// system. Generic families map to the usual system defaults (`Times New
    /// Roman`, `Arial`, `Courier New`, ...) when those are installed, and
    /// to the bundled fonts otherwise.
    pub fn with_system_fonts() -> Self {
        Self::build(true)
    }

    fn build(system: bool) -> Self {
        let mut db = Database::new();
        let mut bundled = Vec::new();
        for bytes in BUNDLED {
            let source: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(bytes);
            for id in db.load_font_source(Source::Binary(source)) {
                bundled.push((id, bytes));
            }
        }
        if system {
            db.load_system_fonts();
        } else {
            db.set_serif_family(SANS);
            db.set_sans_serif_family(SANS);
            db.set_cursive_family(SANS);
            db.set_fantasy_family(SANS);
            db.set_monospace_family(MONO);
        }

        let mut faces = Vec::new();
        for &(id, bytes) in &bundled {
            if let Some(info) = db.face(id) {
                faces.push(Face {
                    db_id: id,
                    info: face_info(info),
                    bundled: Some(bytes),
                });
            }
        }
        for info in db.faces() {
            if !bundled.iter().any(|&(id, _)| id == info.id) {
                faces.push(Face {
                    db_id: info.id,
                    info: face_info(info),
                    bundled: None,
                });
            }
        }

        let by_db_id = faces
            .iter()
            .enumerate()
            .map(|(i, f)| (f.db_id, FontId(i as u32)))
            .collect();
        let mut names = HashMap::new();
        for info in db.faces() {
            for (name, _) in &info.families {
                names
                    .entry(name.to_lowercase())
                    .or_insert_with(|| name.clone());
            }
        }
        let face_count = faces.len();

        Self {
            db,
            faces,
            by_db_id,
            names,
            data: Mutex::new(vec![None; face_count]),
            select_cache: Mutex::default(),
            shape_cache: Mutex::default(),
            raster_cache: Mutex::default(),
        }
    }

    /// Choose the face that best matches a `font-family` list, a
    /// `font-weight`, and a `font-style`.
    ///
    /// Family names match case-insensitively. The first family that has
    /// any face wins; within it the CSS matching rules pick the style and
    /// then the nearest weight (for a requested weight of 500 or more,
    /// heavier faces are preferred; below 400, lighter ones). When no
    /// listed family is available, `sans-serif` is used.
    pub fn select(&self, family: &[String], weight: u16, style: FontStyle) -> FontId {
        let key = (family.join("\0"), weight, style as u8);
        if let Some(&id) = lock(&self.select_cache).get(&key) {
            return id;
        }

        let mut families: Vec<Family<'_>> =
            family.iter().filter_map(|name| self.family(name)).collect();
        families.extend([Family::SansSerif, Family::Name(SANS)]);
        let query = Query {
            families: &families,
            weight: Weight(weight.clamp(1, 1000)),
            stretch: Stretch::Normal,
            style: match style {
                FontStyle::Normal => Style::Normal,
                FontStyle::Italic => Style::Italic,
                FontStyle::Oblique => Style::Oblique,
            },
        };
        let id = self
            .db
            .query(&query)
            .and_then(|id| self.by_db_id.get(&id).copied())
            .unwrap_or(FontId(0));

        let mut cache = lock(&self.select_cache);
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, id);
        id
    }

    fn family<'a>(&'a self, name: &str) -> Option<Family<'a>> {
        let name = name.trim().to_lowercase();
        Some(match name.as_str() {
            "serif" | "ui-serif" => Family::Serif,
            "sans-serif" | "system-ui" | "ui-sans-serif" | "ui-rounded" | "-apple-system"
            | "blinkmacsystemfont" => Family::SansSerif,
            "monospace" | "ui-monospace" => Family::Monospace,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            _ => Family::Name(self.names.get(&name)?),
        })
    }

    /// Description of a face. An id from another context gets the
    /// fallback face's description.
    pub fn face_info(&self, font: FontId) -> &FaceInfo {
        let face = self.faces.get(font.index()).unwrap_or(&self.faces[0]);
        &face.info
    }

    /// Number of faces available.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Run `f` with the parsed face. Faces whose data can't be loaded or
    /// parsed, and ids from another context, use bundled DejaVu Sans.
    pub(crate) fn with_face<R>(
        &self,
        font: FontId,
        f: impl FnOnce(ttf_parser::Face<'_>) -> R,
    ) -> R {
        let data = self.data(font);
        match ttf_parser::Face::parse(data.as_slice(), data.index) {
            Ok(face) => f(face),
            Err(_) => f(fallback_face()),
        }
    }

    fn data(&self, font: FontId) -> FontData {
        let mut slots = lock(&self.data);
        let Some(slot) = slots.get_mut(font.index()) else {
            return FontData::FALLBACK;
        };
        if let Some(data) = slot {
            return data.clone();
        }
        let face = &self.faces[font.index()];
        let data = match face.bundled {
            Some(bytes) => Some(FontData {
                bytes: Bytes::Static(bytes),
                index: 0,
            }),
            None => self.db.with_face_data(face.db_id, |bytes, index| FontData {
                bytes: Bytes::Shared(Arc::from(bytes)),
                index,
            }),
        }
        .filter(|d| ttf_parser::Face::parse(d.as_slice(), d.index).is_ok())
        .unwrap_or(FontData::FALLBACK);
        *slot = Some(data.clone());
        data
    }
}

impl Default for FontContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FontContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontContext")
            .field("faces", &self.faces.len())
            .finish_non_exhaustive()
    }
}

fn face_info(info: &fontdb::FaceInfo) -> FaceInfo {
    FaceInfo {
        family: info
            .families
            .first()
            .map(|(name, _)| name.clone())
            .unwrap_or_default(),
        weight: info.weight.0,
        style: match info.style {
            Style::Normal => FontStyle::Normal,
            Style::Italic => FontStyle::Italic,
            Style::Oblique => FontStyle::Oblique,
        },
        monospace: info.monospaced,
    }
}

fn fallback_face() -> ttf_parser::Face<'static> {
    ttf_parser::Face::parse(BUNDLED[0], 0).expect("bundled DejaVu Sans parses")
}

/// Lock a mutex, ignoring poisoning: the caches stay consistent even if a
/// panic interrupted a previous holder.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Clamp a font size to `0..=MAX_SIZE`, mapping NaN and negatives to 0.
pub(crate) fn sanitize_size(size_px: f32) -> f32 {
    if size_px.is_finite() && size_px > 0.0 {
        size_px.min(MAX_SIZE)
    } else {
        0.0
    }
}

/// Font units → pixels.
pub(crate) fn scale(face: &ttf_parser::Face<'_>, size_px: f32) -> f32 {
    size_px / f32::from(face.units_per_em().max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn families(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    fn pick(fonts: &FontContext, names: &[&str], weight: u16, style: FontStyle) -> FaceInfo {
        fonts
            .face_info(fonts.select(&families(names), weight, style))
            .clone()
    }

    #[test]
    fn bundled_faces_all_load() {
        let fonts = FontContext::new();
        assert_eq!(fonts.face_count(), 8);
        assert_eq!(fonts.face_info(FontId(0)).family, SANS);
        assert_eq!(fonts.face_info(FontId(0)).weight, 400);
        assert_eq!(fonts.face_info(FontId(0)).style, FontStyle::Normal);
        for i in 0..8 {
            let data = fonts.data(FontId(i));
            assert!(ttf_parser::Face::parse(data.as_slice(), data.index).is_ok());
        }
    }

    #[test]
    fn generic_families() {
        let fonts = FontContext::new();
        for generic in ["serif", "sans-serif", "system-ui", "cursive", "fantasy"] {
            assert_eq!(
                pick(&fonts, &[generic], 400, FontStyle::Normal).family,
                SANS
            );
        }
        let mono = pick(&fonts, &["monospace"], 400, FontStyle::Normal);
        assert_eq!(mono.family, MONO);
        assert!(mono.monospace);
    }

    #[test]
    fn names_match_case_insensitively() {
        let fonts = FontContext::new();
        assert_eq!(
            pick(&fonts, &["dejavu sans MONO"], 400, FontStyle::Normal).family,
            MONO
        );
        assert_eq!(
            pick(&fonts, &[" DejaVu Sans Mono "], 400, FontStyle::Normal).family,
            MONO
        );
    }

    #[test]
    fn first_available_family_wins() {
        let fonts = FontContext::new();
        let face = pick(
            &fonts,
            &["No Such Font", "monospace", "serif"],
            400,
            FontStyle::Normal,
        );
        assert_eq!(face.family, MONO);
        assert_eq!(
            pick(&fonts, &["No Such Font"], 400, FontStyle::Normal).family,
            SANS
        );
        assert_eq!(pick(&fonts, &[], 400, FontStyle::Normal).family, SANS);
    }

    #[test]
    fn weight_matching_follows_css_direction_rules() {
        let fonts = FontContext::new();
        let weight = |w| pick(&fonts, &["sans-serif"], w, FontStyle::Normal).weight;
        // DejaVu has 400 and 700 only.
        assert_eq!(weight(100), 400);
        assert_eq!(weight(400), 400);
        // 500 checks 500 first, then lighter weights, before heavier ones.
        assert_eq!(weight(500), 400);
        // 600 and up prefer heavier faces.
        assert_eq!(weight(600), 700);
        assert_eq!(weight(700), 700);
        assert_eq!(weight(1000), 700);
        assert_eq!(weight(0), 400);
        assert_eq!(weight(u16::MAX), 700);
    }

    #[test]
    fn italic_and_oblique_pick_the_slanted_face() {
        let fonts = FontContext::new();
        for style in [FontStyle::Italic, FontStyle::Oblique] {
            let face = pick(&fonts, &["sans-serif"], 700, style);
            assert_ne!(face.style, FontStyle::Normal);
            assert_eq!(face.weight, 700);
            assert_eq!(face.family, SANS);
        }
        let mono = pick(&fonts, &["monospace"], 400, FontStyle::Italic);
        assert_eq!(mono.family, MONO);
        assert_ne!(mono.style, FontStyle::Normal);
    }

    #[test]
    fn selection_is_cached_and_stable() {
        let fonts = FontContext::new();
        let f = families(&["monospace"]);
        let a = fonts.select(&f, 700, FontStyle::Normal);
        assert_eq!(fonts.select(&f, 700, FontStyle::Normal), a);
        assert_eq!(FontContext::new().select(&f, 700, FontStyle::Normal), a);
    }

    #[test]
    fn foreign_font_id_falls_back() {
        let fonts = FontContext::new();
        assert_eq!(fonts.face_info(FontId(9_999)).family, SANS);
        assert!(fonts.shape(FontId(9_999), 16.0, "hi").advance > 0.0);
    }

    #[test]
    fn system_fonts_load() {
        let fonts = FontContext::with_system_fonts();
        assert!(fonts.face_count() >= 8);
        let id = fonts.select(&families(&["sans-serif"]), 400, FontStyle::Normal);
        assert!(fonts.shape(id, 16.0, "Hello").advance > 0.0);
        let id = fonts.select(&families(&["DejaVu Sans Mono"]), 400, FontStyle::Normal);
        assert!(fonts.face_info(id).monospace);
    }

    #[test]
    fn sizes_are_sanitised() {
        assert_eq!(sanitize_size(16.0), 16.0);
        assert_eq!(sanitize_size(f32::NAN), 0.0);
        assert_eq!(sanitize_size(-3.0), 0.0);
        assert_eq!(sanitize_size(f32::INFINITY), 0.0);
        assert_eq!(sanitize_size(1e9), MAX_SIZE);
    }
}
