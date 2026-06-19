// core/msx-ast/src/primitives.rs

/// 2D point in scene user-unit space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self { Point { x, y } }
    pub fn zero() -> Self { Point { x: 0.0, y: 0.0 } }

    pub fn distance_to(self, o: Point) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }

    pub fn lerp(self, o: Point, t: f64) -> Point {
        Point::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", fmt_f64(self.x), fmt_f64(self.y))
    }
}

/// SVG viewBox — maps a user-space region onto the canvas.
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

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&(self.min_x  as f32).to_le_bytes());
        b[4..8].copy_from_slice(&(self.min_y  as f32).to_le_bytes());
        b[8..12].copy_from_slice(&(self.width as f32).to_le_bytes());
        b[12..16].copy_from_slice(&(self.height as f32).to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8; 16]) -> Self {
        let f32_at = |i: usize| f32::from_le_bytes(b[i..i+4].try_into().unwrap()) as f64;
        ViewBox::new(f32_at(0), f32_at(4), f32_at(8), f32_at(12))
    }

    pub fn to_svg_attr(&self) -> String {
        format!("{} {} {} {}", fmt_f64(self.min_x), fmt_f64(self.min_y), fmt_f64(self.width), fmt_f64(self.height))
    }
}

/// Axis-aligned geometry rectangle (not the drawing Rect element).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoRect {
    pub x:      f64,
    pub y:      f64,
    pub width:  f64,
    pub height: f64,
}

impl GeoRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self { GeoRect { x, y, width: w, height: h } }
    pub fn min_x(self) -> f64 { self.x }
    pub fn min_y(self) -> f64 { self.y }
    pub fn max_x(self) -> f64 { self.x + self.width }
    pub fn max_y(self) -> f64 { self.y + self.height }
}

/// Axis-aligned bounding box (min/max corners).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min: Point,
    pub max: Point,
}

impl BoundingBox {
    pub fn new(min: Point, max: Point) -> Self { BoundingBox { min, max } }

    pub fn empty() -> Self {
        BoundingBox { min: Point::new(f64::INFINITY, f64::INFINITY), max: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY) }
    }

    pub fn expand_point(&mut self, p: Point) {
        self.min.x = self.min.x.min(p.x); self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x); self.max.y = self.max.y.max(p.y);
    }

    pub fn expand_by(&mut self, amount: f64) {
        self.min.x -= amount; self.min.y -= amount;
        self.max.x += amount; self.max.y += amount;
    }

    pub fn expand_box(&mut self, o: BoundingBox) { self.expand_point(o.min); self.expand_point(o.max); }
    pub fn width(self)    -> f64  { self.max.x - self.min.x }
    pub fn height(self)   -> f64  { self.max.y - self.min.y }
    pub fn is_empty(self) -> bool { self.min.x > self.max.x || self.min.y > self.max.y }
    pub fn to_geo_rect(self) -> GeoRect { GeoRect::new(self.min.x, self.min.y, self.width(), self.height()) }
}

/// Format f64 for SVG output: integer when exact, up to 4 decimal places otherwise.
pub fn fmt_f64(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e10 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.4}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn distance()      { assert!((Point::new(0.0,0.0).distance_to(Point::new(3.0,4.0)) - 5.0).abs() < 1e-9); }
    #[test] fn viewbox_rt()    { let vb = ViewBox::new(10.0,20.0,300.0,200.0); let vb2 = ViewBox::from_bytes(&vb.to_bytes()); assert!((vb.width - vb2.width).abs() < 1e-4); }
    #[test] fn fmt_strips()    { assert_eq!(fmt_f64(1.0), "1"); assert_eq!(fmt_f64(1.5), "1.5"); assert_eq!(fmt_f64(0.125), "0.125"); }
    #[test] fn bbox_expand()   { let mut b = BoundingBox::empty(); b.expand_point(Point::new(10.0,20.0)); b.expand_point(Point::new(-5.0,30.0)); assert!((b.width() - 15.0).abs() < 1e-9); }
  }
