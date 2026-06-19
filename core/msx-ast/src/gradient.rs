// core/msx-ast/src/gradient.rs

use crate::color::Color;
use crate::primitives::fmt_f64;

/// A single gradient color stop.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub offset: f64,  // 0.0..=1.0
    pub color:  Color,
}

impl Stop {
    pub fn new(offset: f64, color: Color) -> Self { Stop { offset, color } }

    /// 8-byte wire format: [f32 offset][u8 r g b a].
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0..4].copy_from_slice(&(self.offset as f32).to_le_bytes());
        b[4..8].copy_from_slice(&self.color.to_bytes());
        b
    }

    pub fn from_bytes(b: &[u8; 8]) -> Self {
        Stop {
            offset: f32::from_le_bytes(b[0..4].try_into().unwrap()) as f64,
            color:  Color::from_bytes(b[4..8].try_into().unwrap()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    pub id:    String,
    pub x1:    f64,
    pub y1:    f64,
    pub x2:    f64,
    pub y2:    f64,
    pub stops: Vec<Stop>,
}

impl LinearGradient {
    pub fn new(id: String, x1: f64, y1: f64, x2: f64, y2: f64, stops: Vec<Stop>) -> Self {
        LinearGradient { id, x1, y1, x2, y2, stops }
    }

    pub fn to_svg(&self) -> String {
        let stops_svg: String = self.stops.iter().map(|s| format!(
            r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
            fmt_f64(s.offset), s.color.to_svg_hex(), fmt_f64(s.color.opacity()),
        )).collect::<Vec<_>>().join("");
        format!(
            r#"<linearGradient id="{}" x1="{}" y1="{}" x2="{}" y2="{}" gradientUnits="objectBoundingBox">{}</linearGradient>"#,
            self.id, fmt_f64(self.x1), fmt_f64(self.y1), fmt_f64(self.x2), fmt_f64(self.y2), stops_svg,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadialGradient {
    pub id:    String,
    pub cx:    f64,
    pub cy:    f64,
    pub r:     f64,
    pub fx:    f64,
    pub fy:    f64,
    pub stops: Vec<Stop>,
}

impl RadialGradient {
    pub fn new(id: String, cx: f64, cy: f64, r: f64, fx: f64, fy: f64, stops: Vec<Stop>) -> Self {
        RadialGradient { id, cx, cy, r, fx, fy, stops }
    }

    pub fn to_svg(&self) -> String {
        let stops_svg: String = self.stops.iter().map(|s| format!(
            r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
            fmt_f64(s.offset), s.color.to_svg_hex(), fmt_f64(s.color.opacity()),
        )).collect::<Vec<_>>().join("");
        format!(
            r#"<radialGradient id="{}" cx="{}" cy="{}" r="{}" fx="{}" fy="{}" gradientUnits="objectBoundingBox">{}</radialGradient>"#,
            self.id, fmt_f64(self.cx), fmt_f64(self.cy), fmt_f64(self.r),
            fmt_f64(self.fx), fmt_f64(self.fy), stops_svg,
        )
    }
}

/// Conic gradient — native to CSS, approximated in SVG 1.1 export.
#[derive(Debug, Clone, PartialEq)]
pub struct ConicGradient {
    pub id:    String,
    pub cx:    f64,
    pub cy:    f64,
    /// Start angle in degrees.
    pub angle: f64,
    pub stops: Vec<Stop>,
}

impl ConicGradient {
    pub fn new(id: String, cx: f64, cy: f64, angle: f64, stops: Vec<Stop>) -> Self {
        ConicGradient { id, cx, cy, angle, stops }
    }

    pub fn to_svg(&self) -> String {
        // SVG 1.1 has no conic gradient; emit a comment.
        format!("<!-- conic gradient '{}': use SVG 2 / CSS for native support -->", self.id)
    }
}

/// Scene-level definition — referenced by paint `"url(#id)"`.
#[derive(Debug, Clone)]
pub enum Def {
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
    ConicGradient(ConicGradient),
}

impl Def {
    pub fn id(&self) -> &str {
        match self {
            Def::LinearGradient(g) => &g.id,
            Def::RadialGradient(g) => &g.id,
            Def::ConicGradient(g)  => &g.id,
        }
    }

    pub fn to_svg(&self) -> String {
        match self {
            Def::LinearGradient(g) => g.to_svg(),
            Def::RadialGradient(g) => g.to_svg(),
            Def::ConicGradient(g)  => g.to_svg(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    #[test]
    fn stop_roundtrip() {
        let s = Stop::new(0.5, Color::rgb(255, 128, 0));
        let s2 = Stop::from_bytes(&s.to_bytes());
        assert!((s.offset - s2.offset).abs() < 1e-4);
        assert_eq!(s.color, s2.color);
    }
               }
