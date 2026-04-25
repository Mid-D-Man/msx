// src/color.rs

use crate::primitives::fmt_f64;

// ── Color ─────────────────────────────────────────────────────────────────────

/// An sRGB color with premultiplied-alpha stored as separate channels (0..=255).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK:       Color = Color { r: 0,   g: 0,   b: 0,   a: 255 };
    pub const WHITE:       Color = Color { r: 255, g: 255, b: 255, a: 255 };
    pub const TRANSPARENT: Color = Color { r: 0,   g: 0,   b: 0,   a: 0   };

    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Color { r, g, b, a } }
    pub fn rgb(r: u8, g: u8, b: u8)         -> Self { Color { r, g, b, a: 255 } }

    pub fn is_opaque(self) -> bool     { self.a == 255 }
    pub fn is_transparent(self) -> bool { self.a == 0 }

    pub fn opacity(self) -> f64 { self.a as f64 / 255.0 }

    /// Serialize to 4 bytes `[r g b a]`.
    pub fn to_bytes(self) -> [u8; 4] { [self.r, self.g, self.b, self.a] }

    pub fn from_bytes(b: [u8; 4]) -> Self { Color { r: b[0], g: b[1], b: b[2], a: b[3] } }

    /// SVG hex string: `#rrggbb` when fully opaque, `#rrggbbaa` otherwise.
    pub fn to_svg_hex(self) -> String {
        if self.is_opaque() {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Parse `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r,g,b)`, `rgba(r,g,b,a)`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        if let Some(hex) = s.strip_prefix('#') {
            return parse_hex(hex);
        }

        if let Some(inner) = s.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
            return parse_rgba_fn(inner);
        }

        if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            return parse_rgb_fn(inner);
        }

        // Named colors (minimal set — extend as needed)
        match s {
            "black"   => Some(Color::BLACK),
            "white"   => Some(Color::WHITE),
            "none"    => Some(Color::TRANSPARENT),
            "red"     => Some(Color::rgb(255, 0,   0)),
            "green"   => Some(Color::rgb(0,   128, 0)),
            "blue"    => Some(Color::rgb(0,   0,   255)),
            _ => None,
        }
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some(Color::rgb(r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::rgb(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color::rgba(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgb_fn(inner: &str) -> Option<Color> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 { return None; }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some(Color::rgb(r, g, b))
}

fn parse_rgba_fn(inner: &str) -> Option<Color> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 4 { return None; }
    let r  = parts[0].trim().parse::<u8>().ok()?;
    let g  = parts[1].trim().parse::<u8>().ok()?;
    let b  = parts[2].trim().parse::<u8>().ok()?;
    // alpha may be float 0.0..1.0 or integer 0..255
    let af = parts[3].trim().parse::<f64>().ok()?;
    let a  = if af <= 1.0 { (af * 255.0).round() as u8 } else { af as u8 };
    Some(Color::rgba(r, g, b, a))
}

// ── Paint ─────────────────────────────────────────────────────────────────────

/// The fill or stroke paint of an element.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    None,
    Color(Color),
    /// References a gradient or pattern by id string, e.g. `"url(#sunset)"`.
    Ref(String),
    CurrentColor,
}

impl Paint {
    /// Binary type tags (matches format-spec).
    pub const TAG_NONE:    u8 = 0x00;
    pub const TAG_COLOR:   u8 = 0x01;
    pub const TAG_GRADIENT_REF: u8 = 0x02;
    pub const TAG_PATTERN_REF:  u8 = 0x03;

    pub fn is_none(&self) -> bool { matches!(self, Paint::None) }

    /// Parse from an MSX source string value.
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        if s == "none" || s.is_empty() { return Paint::None; }
        if s == "currentColor"         { return Paint::CurrentColor; }
        if s.starts_with("url(") && s.ends_with(')') {
            let id = s[4..s.len() - 1].to_string();
            return Paint::Ref(id);
        }
        Color::parse(s).map(Paint::Color).unwrap_or(Paint::None)
    }

    /// SVG attribute string.
    pub fn to_svg_value(&self) -> String {
        match self {
            Paint::None             => "none".to_string(),
            Paint::Color(c)         => c.to_svg_hex(),
            Paint::Ref(r)           => r.clone(),
            Paint::CurrentColor     => "currentColor".to_string(),
        }
    }
}

// ── Gradient stop ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub offset:  f64,   // 0.0..1.0
    pub color:   Color,
}

impl Stop {
    pub fn new(offset: f64, color: Color) -> Self { Stop { offset, color } }

    /// 8 bytes: [f32 offset][u8 r g b a]
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&(self.offset as f32).to_le_bytes());
        buf[4..8].copy_from_slice(&self.color.to_bytes());
        buf
    }

    pub fn from_bytes(b: &[u8; 8]) -> Self {
        Stop {
            offset: f32::from_le_bytes(b[0..4].try_into().unwrap()) as f64,
            color:  Color::from_bytes(b[4..8].try_into().unwrap()),
        }
    }
}

// ── Gradient types ────────────────────────────────────────────────────────────

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
        let stops_svg: String = self.stops.iter().map(|s| {
            format!(
                r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
                fmt_f64(s.offset),
                s.color.to_svg_hex(),
                fmt_f64(s.color.opacity()),
            )
        }).collect::<Vec<_>>().join("");

        format!(
            r#"<linearGradient id="{}" x1="{}" y1="{}" x2="{}" y2="{}" gradientUnits="objectBoundingBox">{}</linearGradient>"#,
            self.id,
            fmt_f64(self.x1), fmt_f64(self.y1),
            fmt_f64(self.x2), fmt_f64(self.y2),
            stops_svg,
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
        let stops_svg: String = self.stops.iter().map(|s| {
            format!(
                r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
                fmt_f64(s.offset),
                s.color.to_svg_hex(),
                fmt_f64(s.color.opacity()),
            )
        }).collect::<Vec<_>>().join("");

        format!(
            r#"<radialGradient id="{}" cx="{}" cy="{}" r="{}" fx="{}" fy="{}" gradientUnits="objectBoundingBox">{}</radialGradient>"#,
            self.id,
            fmt_f64(self.cx), fmt_f64(self.cy), fmt_f64(self.r),
            fmt_f64(self.fx), fmt_f64(self.fy),
            stops_svg,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex6() {
        let c = Color::parse("#e94560").unwrap();
        assert_eq!(c, Color::rgb(0xe9, 0x45, 0x60));
    }

    #[test]
    fn parse_hex3() {
        let c = Color::parse("#fff").unwrap();
        assert_eq!(c, Color::rgb(255, 255, 255));
    }

    #[test]
    fn parse_rgba_fn_float_alpha() {
        let c = Color::parse("rgba(0, 0, 0, 0.5)").unwrap();
        assert_eq!(c.a, 128);
    }

    #[test]
    fn color_roundtrip_bytes() {
        let c = Color::rgba(10, 20, 30, 200);
        assert_eq!(Color::from_bytes(c.to_bytes()), c);
    }

    #[test]
    fn paint_parse_url() {
        let p = Paint::parse("url(#sunset)");
        assert_eq!(p, Paint::Ref("url(#sunset)".to_string()));
    }

    #[test]
    fn stop_roundtrip_bytes() {
        let s = Stop::new(0.5, Color::rgb(255, 128, 0));
        let b = s.to_bytes();
        let s2 = Stop::from_bytes(&b);
        assert!((s.offset - s2.offset).abs() < 1e-4);
        assert_eq!(s.color, s2.color);
    }
}
