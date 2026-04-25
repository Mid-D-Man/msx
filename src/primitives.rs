// src/primitives.rs

/// A 2D point in user-unit space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self { Point { x, y } }
    pub fn zero() -> Self { Point { x: 0.0, y: 0.0 } }

    pub fn distance_to(self, other: Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn lerp(self, other: Point, t: f64) -> Point {
        Point {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", fmt_f64(self.x), fmt_f64(self.y))
    }
}

/// Canvas / element size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width:  f64,
    pub height: f64,
}

impl Size {
    pub fn new(width: f64, height: f64) -> Self { Size { width, height } }
    pub fn area(self) -> f64 { self.width * self.height }
    pub fn is_valid(self) -> bool { self.width > 0.0 && self.height > 0.0 }
}

/// An axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x:      f64,
    pub y:      f64,
    pub width:  f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Rect { x, y, width, height }
    }

    pub fn from_corners(min: Point, max: Point) -> Self {
        Rect {
            x:      min.x,
            y:      min.y,
            width:  max.x - min.x,
            height: max.y - min.y,
        }
    }

    pub fn min_x(self) -> f64 { self.x }
    pub fn min_y(self) -> f64 { self.y }
    pub fn max_x(self) -> f64 { self.x + self.width }
    pub fn max_y(self) -> f64 { self.y + self.height }
    pub fn center(self) -> Point { Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0) }

    pub fn contains(self, p: Point) -> bool {
        p.x >= self.min_x() && p.x <= self.max_x()
            && p.y >= self.min_y() && p.y <= self.max_y()
    }

    pub fn union(self, other: Rect) -> Rect {
        let min_x = self.min_x().min(other.min_x());
        let min_y = self.min_y().min(other.min_y());
        let max_x = self.max_x().max(other.max_x());
        let max_y = self.max_y().max(other.max_y());
        Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }
}

/// SVG viewBox — maps a region of user-space onto the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBox {
    pub min_x:  f64,
    pub min_y:  f64,
    pub width:  f64,
    pub height: f64,
}

impl ViewBox {
    pub fn new(min_x: f64, min_y: f64, width: f64, height: f64) -> Self {
        ViewBox { min_x, min_y, width, height }
    }

    /// Serialize to the 16-byte binary form `[f32 min_x min_y w h]`.
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&(self.min_x as f32).to_le_bytes());
        buf[4..8].copy_from_slice(&(self.min_y as f32).to_le_bytes());
        buf[8..12].copy_from_slice(&(self.width as f32).to_le_bytes());
        buf[12..16].copy_from_slice(&(self.height as f32).to_le_bytes());
        buf
    }

    pub fn from_bytes(b: &[u8; 16]) -> Self {
        ViewBox {
            min_x:  f32::from_le_bytes(b[0..4].try_into().unwrap())  as f64,
            min_y:  f32::from_le_bytes(b[4..8].try_into().unwrap())  as f64,
            width:  f32::from_le_bytes(b[8..12].try_into().unwrap()) as f64,
            height: f32::from_le_bytes(b[12..16].try_into().unwrap()) as f64,
        }
    }

    /// SVG attribute string: `"min_x min_y width height"`.
    pub fn to_svg_attr(&self) -> String {
        format!(
            "{} {} {} {}",
            fmt_f64(self.min_x), fmt_f64(self.min_y),
            fmt_f64(self.width), fmt_f64(self.height)
        )
    }
}

/// Axis-aligned bounding box computed from element geometry.
/// Used internally for culling and hit-testing (post-v1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min: Point,
    pub max: Point,
}

impl BoundingBox {
    pub fn new(min: Point, max: Point) -> Self { BoundingBox { min, max } }

    pub fn empty() -> Self {
        BoundingBox {
            min: Point::new(f64::INFINITY,  f64::INFINITY),
            max: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub fn expand_point(&mut self, p: Point) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    pub fn expand_box(&mut self, other: BoundingBox) {
        self.expand_point(other.min);
        self.expand_point(other.max);
    }

    pub fn width(self)  -> f64 { self.max.x - self.min.x }
    pub fn height(self) -> f64 { self.max.y - self.min.y }
    pub fn is_empty(self) -> bool { self.min.x > self.max.x || self.min.y > self.max.y }

    pub fn to_rect(self) -> Rect {
        Rect::new(self.min.x, self.min.y, self.width(), self.height())
    }
}

// ── Formatting helper ─────────────────────────────────────────────────────────

/// Format an f64 for SVG output: drop trailing zeros, cap at 4 decimal places.
pub fn fmt_f64(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e10 {
        format!("{}", v as i64)
    } else {
        // 4 decimal places, strip trailing zeros
        let s = format!("{:.4}", v);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_distance() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(3.0, 4.0);
        assert!((a.distance_to(b) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn rect_union() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        let u = a.union(b);
        assert_eq!(u.x, 0.0);
        assert_eq!(u.y, 0.0);
        assert_eq!(u.width, 15.0);
        assert_eq!(u.height, 15.0);
    }

    #[test]
    fn viewbox_roundtrip_bytes() {
        let vb = ViewBox::new(10.0, 20.0, 300.0, 200.0);
        let b  = vb.to_bytes();
        let vb2 = ViewBox::from_bytes(&b);
        assert!((vb.min_x - vb2.min_x).abs() < 1e-4);
        assert!((vb.width  - vb2.width).abs() < 1e-4);
    }

    #[test]
    fn fmt_f64_strips_zeros() {
        assert_eq!(fmt_f64(1.0),   "1");
        assert_eq!(fmt_f64(1.5),   "1.5");
        assert_eq!(fmt_f64(1.25),  "1.25");
        assert_eq!(fmt_f64(0.125), "0.125");
    }
}
