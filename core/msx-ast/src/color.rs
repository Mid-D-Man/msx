// core/msx-ast/src/color.rs
// Gradient types (Stop, LinearGradient, RadialGradient, ConicGradient, Def)
// live in gradient.rs to keep this file focused.

/// sRGB color with premultiplied-alpha, u8 channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    #[inline] pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Color { r, g, b, a } }
    #[inline] pub fn rgb(r: u8, g: u8, b: u8)          -> Self { Color { r, g, b, a: 255 } }

    pub fn is_opaque(self)       -> bool { self.a == 255 }
    pub fn is_transparent(self)  -> bool { self.a == 0 }
    pub fn opacity(self)         -> f64  { self.a as f64 / 255.0 }

    pub fn to_bytes(self)        -> [u8; 4] { [self.r, self.g, self.b, self.a] }
    pub fn from_bytes(b: [u8; 4]) -> Self   { Color { r: b[0], g: b[1], b: b[2], a: b[3] } }

    /// `#rrggbb` or `#rrggbbaa` depending on alpha.
    pub fn to_svg_hex(self) -> String {
        if self.is_opaque() {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Parse `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r,g,b)`, `rgba(r,g,b,a)`, named.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#')                                     { return parse_hex(hex); }
        if let Some(i) = s.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) { return parse_rgba_fn(i); }
        if let Some(i) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')'))  { return parse_rgb_fn(i); }
        match s {
            "black" => Some(Color::BLACK),
            "white" => Some(Color::WHITE),
            "none"  => Some(Color::TRANSPARENT),
            "red"   => Some(Color::rgb(255, 0,   0)),
            "green" => Some(Color::rgb(0,   128, 0)),
            "blue"  => Some(Color::rgb(0,   0,   255)),
            _ => None,
        }
    }

    /// Linear interpolate between two colors.
    pub fn lerp(a: Color, b: Color, t: f64) -> Color {
        let ch = |ca: u8, cb: u8| (ca as f64 + (cb as f64 - ca as f64) * t).round() as u8;
        Color::rgba(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b), ch(a.a, b.a))
    }

    /// Linearise sRGB channel (gamma expand). Returns [r,g,b,a] as f32 linear.
    pub fn to_linear(self) -> [f32; 4] {
        let lin = |c: u8| -> f32 {
            let v = c as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        [lin(self.r), lin(self.g), lin(self.b), self.a as f32 / 255.0]
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
        6 => Some(Color::rgb(
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        )),
        8 => Some(Color::rgba(
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        )),
        _ => None,
    }
}

fn parse_rgb_fn(inner: &str) -> Option<Color> {
    let p: Vec<&str> = inner.split(',').collect();
    if p.len() != 3 { return None; }
    Some(Color::rgb(p[0].trim().parse().ok()?, p[1].trim().parse().ok()?, p[2].trim().parse().ok()?))
}

fn parse_rgba_fn(inner: &str) -> Option<Color> {
    let p: Vec<&str> = inner.split(',').collect();
    if p.len() != 4 { return None; }
    let af = p[3].trim().parse::<f64>().ok()?;
    let a  = if af <= 1.0 { (af * 255.0).round() as u8 } else { af as u8 };
    Some(Color::rgba(p[0].trim().parse().ok()?, p[1].trim().parse().ok()?, p[2].trim().parse().ok()?, a))
}

/// Fill or stroke paint value.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    None,
    Color(Color),
    /// `"url(#id)"` reference to a gradient or pattern def.
    Ref(String),
    CurrentColor,
}

impl Paint {
    pub const TAG_NONE:         u8 = 0x00;
    pub const TAG_COLOR:        u8 = 0x01;
    pub const TAG_GRADIENT_REF: u8 = 0x02;
    pub const TAG_PATTERN_REF:  u8 = 0x03;

    pub fn is_none(&self) -> bool { matches!(self, Paint::None) }

    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        if s == "none" || s.is_empty() { return Paint::None; }
        if s == "currentColor"         { return Paint::CurrentColor; }
        if s.starts_with("url(") && s.ends_with(')') { return Paint::Ref(s.to_string()); }
        Color::parse(s).map(Paint::Color).unwrap_or(Paint::None)
    }

    pub fn to_svg_value(&self) -> String {
        match self {
            Paint::None         => "none".to_string(),
            Paint::Color(c)     => c.to_svg_hex(),
            Paint::Ref(r)       => r.clone(),
            Paint::CurrentColor => "currentColor".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn hex6()  { assert_eq!(Color::parse("#e94560").unwrap(), Color::rgb(0xe9, 0x45, 0x60)); }
    #[test] fn hex3()  { assert_eq!(Color::parse("#fff").unwrap(), Color::WHITE); }
    #[test] fn rgba_f() { assert_eq!(Color::parse("rgba(0,0,0,0.5)").unwrap().a, 128); }
    #[test] fn bytes() { let c = Color::rgba(10,20,30,200); assert_eq!(Color::from_bytes(c.to_bytes()), c); }
    #[test] fn url_preserved() { assert_eq!(Paint::parse("url(#g)"), Paint::Ref("url(#g)".into())); }
                                          }
