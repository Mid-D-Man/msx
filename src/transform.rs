// src/transform.rs

use crate::primitives::{fmt_f64, Point};

// ── Transform type tags (binary) ──────────────────────────────────────────────
pub const TRANSFORM_NONE:      u8 = 0x00;
pub const TRANSFORM_MATRIX:    u8 = 0x01;
pub const TRANSFORM_TRANSLATE: u8 = 0x02;
pub const TRANSFORM_SCALE:     u8 = 0x03;
pub const TRANSFORM_ROTATE:    u8 = 0x04;
pub const TRANSFORM_SKEW_X:    u8 = 0x05;
pub const TRANSFORM_SKEW_Y:    u8 = 0x06;
pub const TRANSFORM_MULTIPLE:  u8 = 0x07;

// ── Matrix2D ──────────────────────────────────────────────────────────────────

/// Column-major 2D affine transform matrix.
/// Equivalent to SVG matrix(a, b, c, d, e, f).
/// [ a  c  e ]
/// [ b  d  f ]
/// [ 0  0  1 ]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix2D {
    pub a: f64, pub b: f64,
    pub c: f64, pub d: f64,
    pub e: f64, pub f: f64,
}

impl Matrix2D {
    pub fn identity() -> Self {
        Matrix2D { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }

    pub fn translate(tx: f64, ty: f64) -> Self {
        Matrix2D { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Matrix2D { a: sx, b: 0.0, c: 0.0, d: sy, e: 0.0, f: 0.0 }
    }

    pub fn rotate_deg(angle_deg: f64) -> Self {
        let r = angle_deg.to_radians();
        let (s, c) = (r.sin(), r.cos());
        Matrix2D { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    pub fn rotate_deg_around(angle_deg: f64, cx: f64, cy: f64) -> Self {
        // translate(-cx,-cy) * rotate * translate(cx,cy)
        let r   = Matrix2D::rotate_deg(angle_deg);
        let t1  = Matrix2D::translate(-cx, -cy);
        let t2  = Matrix2D::translate(cx, cy);
        t2.concat(r.concat(t1))
    }

    pub fn skew_x(angle_deg: f64) -> Self {
        let t = angle_deg.to_radians().tan();
        Matrix2D { a: 1.0, b: 0.0, c: t, d: 1.0, e: 0.0, f: 0.0 }
    }

    pub fn skew_y(angle_deg: f64) -> Self {
        let t = angle_deg.to_radians().tan();
        Matrix2D { a: 1.0, b: t, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }

    /// Matrix multiplication: self * other (other applied first).
    pub fn concat(self, other: Matrix2D) -> Self {
        Matrix2D {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    pub fn transform_point(self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    pub fn is_identity(self) -> bool {
        const E: f64 = 1e-10;
        (self.a - 1.0).abs() < E && self.b.abs() < E
            && self.c.abs() < E && (self.d - 1.0).abs() < E
            && self.e.abs() < E && self.f.abs() < E
    }

    /// SVG `transform="matrix(...)"` string.
    pub fn to_svg_attr(self) -> String {
        format!(
            "matrix({},{},{},{},{},{})",
            fmt_f64(self.a), fmt_f64(self.b), fmt_f64(self.c),
            fmt_f64(self.d), fmt_f64(self.e), fmt_f64(self.f),
        )
    }

    /// 24 bytes: [f32 a b c d e f]
    pub fn to_bytes(self) -> [u8; 24] {
        let mut buf = [0u8; 24];
        let vals = [self.a, self.b, self.c, self.d, self.e, self.f];
        for (i, v) in vals.iter().enumerate() {
            buf[i * 4..(i + 1) * 4].copy_from_slice(&(*v as f32).to_le_bytes());
        }
        buf
    }

    pub fn from_bytes(b: &[u8; 24]) -> Self {
        let f = |i: usize| f32::from_le_bytes(b[i*4..i*4+4].try_into().unwrap()) as f64;
        Matrix2D { a: f(0), b: f(1), c: f(2), d: f(3), e: f(4), f: f(5) }
    }
}

// ── Transform enum ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Transform {
    None,
    Matrix(Matrix2D),
    Translate { x: f64, y: f64 },
    Scale { x: f64, y: f64 },
    Rotate { angle: f64, cx: Option<f64>, cy: Option<f64> },
    SkewX(f64),
    SkewY(f64),
    /// Chain of transforms applied right-to-left (last element applied first).
    Multiple(Vec<Transform>),
}

impl Transform {
    pub fn is_none(&self) -> bool { matches!(self, Transform::None) }

    /// Collapse to a single Matrix2D for rendering.
    pub fn to_matrix(&self) -> Matrix2D {
        match self {
            Transform::None => Matrix2D::identity(),
            Transform::Matrix(m) => *m,
            Transform::Translate { x, y } => Matrix2D::translate(*x, *y),
            Transform::Scale { x, y } => Matrix2D::scale(*x, *y),
            Transform::Rotate { angle, cx, cy } => {
                match (cx, cy) {
                    (Some(cx), Some(cy)) => Matrix2D::rotate_deg_around(*angle, *cx, *cy),
                    _ => Matrix2D::rotate_deg(*angle),
                }
            }
            Transform::SkewX(a) => Matrix2D::skew_x(*a),
            Transform::SkewY(a) => Matrix2D::skew_y(*a),
            Transform::Multiple(v) => {
                v.iter().rev().fold(Matrix2D::identity(), |acc, t| {
                    acc.concat(t.to_matrix())
                })
            }
        }
    }

    /// SVG `transform` attribute value string.
    pub fn to_svg_attr(&self) -> String {
        match self {
            Transform::None => String::new(),
            Transform::Matrix(m)         => m.to_svg_attr(),
            Transform::Translate { x, y } => format!("translate({},{})", fmt_f64(*x), fmt_f64(*y)),
            Transform::Scale { x, y }     => {
                if (x - y).abs() < 1e-10 { format!("scale({})", fmt_f64(*x)) }
                else { format!("scale({},{})", fmt_f64(*x), fmt_f64(*y)) }
            }
            Transform::Rotate { angle, cx, cy } => {
                match (cx, cy) {
                    (Some(cx), Some(cy)) => format!("rotate({},{},{})", fmt_f64(*angle), fmt_f64(*cx), fmt_f64(*cy)),
                    _ => format!("rotate({})", fmt_f64(*angle)),
                }
            }
            Transform::SkewX(a) => format!("skewX({})", fmt_f64(*a)),
            Transform::SkewY(a) => format!("skewY({})", fmt_f64(*a)),
            Transform::Multiple(v) => {
                v.iter().map(|t| t.to_svg_attr()).collect::<Vec<_>>().join(" ")
            }
        }
    }

    /// Parse SVG transform string e.g. `"translate(10 20) rotate(45)"`.
    pub fn parse_svg(s: &str) -> Self {
        let s = s.trim();
        if s.is_empty() { return Transform::None; }

        let mut parts: Vec<Transform> = Vec::new();

        // Split on ')' boundaries
        for chunk in s.split(')') {
            let chunk = chunk.trim();
            if chunk.is_empty() { continue; }

            if let Some(t) = parse_single_transform(chunk) {
                parts.push(t);
            }
        }

        match parts.len() {
            0 => Transform::None,
            1 => parts.remove(0),
            _ => Transform::Multiple(parts),
        }
    }
}

fn parse_single_transform(chunk: &str) -> Option<Transform> {
    // chunk looks like `translate(10 20` or `rotate(45,100,100`
    let paren = chunk.find('(')?;
    let name  = chunk[..paren].trim().to_lowercase();
    let args_str = chunk[paren + 1..].trim();
    let args: Vec<f64> = args_str
        .split([',', ' '].as_ref())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .collect();

    match name.as_str() {
        "translate" => Some(Transform::Translate {
            x: *args.first()?,
            y: args.get(1).copied().unwrap_or(0.0),
        }),
        "scale" => Some(Transform::Scale {
            x: *args.first()?,
            y: args.get(1).copied().unwrap_or(*args.first()?),
        }),
        "rotate" => Some(Transform::Rotate {
            angle: *args.first()?,
            cx: args.get(1).copied(),
            cy: args.get(2).copied(),
        }),
        "skewx" => Some(Transform::SkewX(*args.first()?)),
        "skewy" => Some(Transform::SkewY(*args.first()?)),
        "matrix" => {
            if args.len() < 6 { return None; }
            Some(Transform::Matrix(Matrix2D {
                a: args[0], b: args[1], c: args[2],
                d: args[3], e: args[4], f: args[5],
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_identity_concat() {
        let m = Matrix2D::identity();
        let t = Matrix2D::translate(10.0, 20.0);
        let r = m.concat(t);
        assert!((r.e - 10.0).abs() < 1e-9);
        assert!((r.f - 20.0).abs() < 1e-9);
    }

    #[test]
    fn rotate_point() {
        // 90 degree rotation: (1,0) → (0,1)
        let m = Matrix2D::rotate_deg(90.0);
        let p = m.transform_point(Point::new(1.0, 0.0));
        assert!((p.x - 0.0).abs() < 1e-9);
        assert!((p.y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn matrix_roundtrip_bytes() {
        let m = Matrix2D { a: 1.0, b: 0.5, c: -0.5, d: 1.0, e: 100.0, f: 200.0 };
        let b = m.to_bytes();
        let m2 = Matrix2D::from_bytes(&b);
        assert!((m.a - m2.a).abs() < 1e-4);
        assert!((m.e - m2.e).abs() < 1e-4);
    }

    #[test]
    fn parse_svg_translate() {
        let t = Transform::parse_svg("translate(10, 20)");
        assert_eq!(t, Transform::Translate { x: 10.0, y: 20.0 });
    }

    #[test]
    fn parse_svg_rotate_with_center() {
        let t = Transform::parse_svg("rotate(45, 100, 100)");
        assert_eq!(t, Transform::Rotate { angle: 45.0, cx: Some(100.0), cy: Some(100.0) });
    }

    #[test]
    fn transform_svg_attr_roundtrip() {
        let t = Transform::Translate { x: 10.5, y: -20.0 };
        let s = t.to_svg_attr();
        let t2 = Transform::parse_svg(&s);
        assert_eq!(t, t2);
    }
}
