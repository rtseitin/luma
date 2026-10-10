//! Rectangles and length sanitising.

use luma_style::{Edges, LengthPct, MaxSize, Size};

/// Lengths are clamped to this magnitude in pixels, so that sums of
/// absurd style values stay finite.
pub(crate) const MAX_LENGTH: f32 = 1.0e7;

/// An axis-aligned rectangle in CSS pixels, y down.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width; never negative.
    pub width: f32,
    /// Height; never negative.
    pub height: f32,
}

impl Rect {
    /// A rectangle from its position and size.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge (`x + width`).
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge (`y + height`).
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Whether the rectangle has no area.
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Whether two rectangles overlap (touching edges do not count).
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The rectangle grown outwards by `edges` on each side.
    pub fn outset(&self, edges: &Edges<f32>) -> Rect {
        Rect {
            x: self.x - edges.left,
            y: self.y - edges.top,
            width: (self.width + edges.left + edges.right).max(0.0),
            height: (self.height + edges.top + edges.bottom).max(0.0),
        }
    }

    /// The rectangle moved by `(dx, dy)`.
    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            ..*self
        }
    }
}

/// Clamp a length to something finite and sane: NaN becomes 0 and the
/// magnitude is capped at [`MAX_LENGTH`].
pub(crate) fn sanitize(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-MAX_LENGTH, MAX_LENGTH)
    }
}

/// Resolve a length-or-percentage against a containing block size.
pub(crate) fn resolve_lp(v: LengthPct, container: f32) -> f32 {
    sanitize(v.resolve(container))
}

/// Resolve a `Size`; `auto` is `None`.
pub(crate) fn resolve_size(v: Size, container: f32) -> Option<f32> {
    match v {
        Size::Auto => None,
        Size::Length(l) => Some(resolve_lp(l, container)),
    }
}

/// Resolve a `Size` whose percentages depend on a possibly indefinite
/// container (heights): a percentage of an indefinite size is `auto`.
pub(crate) fn resolve_size_indefinite(v: Size, container: Option<f32>) -> Option<f32> {
    match (v, container) {
        (Size::Auto, _) => None,
        (Size::Length(LengthPct::Px(px)), _) => Some(sanitize(px)),
        (Size::Length(l @ LengthPct::Percent(_)), Some(c)) => Some(resolve_lp(l, c)),
        (Size::Length(LengthPct::Percent(_)), None) => None,
    }
}

/// Resolve a `MaxSize`; `none` is `None`. Percentages of an indefinite
/// container are `none`.
pub(crate) fn resolve_max(v: MaxSize, container: Option<f32>) -> Option<f32> {
    match (v, container) {
        (MaxSize::None, _) => None,
        (MaxSize::Length(LengthPct::Px(px)), _) => Some(sanitize(px)),
        (MaxSize::Length(l @ LengthPct::Percent(_)), Some(c)) => Some(resolve_lp(l, c)),
        (MaxSize::Length(LengthPct::Percent(_)), None) => None,
    }
}

/// Clamp `v` by optional `min` and `max`; `min` wins when they conflict
/// (CSS 2.1 §10.4).
pub(crate) fn clamp_min_max(v: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let mut v = v;
    if let Some(max) = max {
        v = v.min(max);
    }
    if let Some(min) = min {
        v = v.max(min);
    }
    v.max(0.0)
}

/// Resolve the four sides of a padding box against the containing block
/// width (all four sides use the width; CSS 2.1 §8.4).
pub(crate) fn resolve_edges(e: &Edges<LengthPct>, container_width: f32) -> Edges<f32> {
    Edges {
        top: resolve_lp(e.top, container_width).max(0.0),
        right: resolve_lp(e.right, container_width).max(0.0),
        bottom: resolve_lp(e.bottom, container_width).max(0.0),
        left: resolve_lp(e.left, container_width).max(0.0),
    }
}

/// Border widths, sanitised and non-negative.
pub(crate) fn border_edges(e: &Edges<f32>) -> Edges<f32> {
    Edges {
        top: sanitize(e.top).max(0.0),
        right: sanitize(e.right).max(0.0),
        bottom: sanitize(e.bottom).max(0.0),
        left: sanitize(e.left).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_edges_and_outset() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(r.right(), 40.0);
        assert_eq!(r.bottom(), 60.0);
        let o = r.outset(&Edges {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        });
        assert_eq!(o, Rect::new(6.0, 19.0, 36.0, 44.0));
        assert!(r.intersects(&Rect::new(39.0, 59.0, 5.0, 5.0)));
        assert!(!r.intersects(&Rect::new(40.0, 20.0, 5.0, 5.0)));
        assert!(Rect::new(0.0, 0.0, 0.0, 5.0).is_empty());
    }

    #[test]
    fn sanitising() {
        assert_eq!(sanitize(f32::NAN), 0.0);
        assert_eq!(sanitize(f32::INFINITY), MAX_LENGTH);
        assert_eq!(sanitize(-1e30), -MAX_LENGTH);
        assert_eq!(sanitize(12.5), 12.5);
    }

    #[test]
    fn indefinite_percentages() {
        let pct = Size::Length(LengthPct::Percent(50.0));
        assert_eq!(resolve_size_indefinite(pct, Some(200.0)), Some(100.0));
        assert_eq!(resolve_size_indefinite(pct, None), None);
        assert_eq!(
            resolve_max(MaxSize::Length(LengthPct::Percent(50.0)), None),
            None
        );
        assert_eq!(
            clamp_min_max(50.0, Some(80.0), Some(60.0)),
            80.0,
            "min wins"
        );
        assert_eq!(clamp_min_max(-5.0, None, None), 0.0);
    }
}
