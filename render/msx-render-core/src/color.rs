// render/msx-render-core/src/color.rs
use msx_ast::Color;

/// Premultiplied-alpha f32 RGBA — the representation blend math wants to
/// operate on, since straight-alpha blending is wrong wherever opacity < 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PremulColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl PremulColor {
    pub const TRANSPARENT: PremulColor = PremulColor { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub fn from_straight(r: f32, g: f32, b: f32, a: f32) -> Self {
        PremulColor { r: r * a, g: g * a, b: b * a, a }
    }

    pub fn from_color(c: Color) -> Self {
        let a = c.opacity() as f32;
        PremulColor::from_straight(c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, a)
    }

    pub fn to_straight(self) -> (f32, f32, f32, f32) {
        if self.a <= 0.0 {
            (0.0, 0.0, 0.0, 0.0)
        } else {
            (self.r / self.a, self.g / self.a, self.b / self.a, self.a)
        }
    }

    pub fn to_srgb8(self) -> [u8; 4] {
        let (r, g, b, a) = self.to_straight();
        [to_u8(r), to_u8(g), to_u8(b), to_u8(a)]
    }

    /// Standard Porter-Duff "source over destination", both already
    /// premultiplied.
    pub fn over(self, dst: PremulColor) -> PremulColor {
        let inv = 1.0 - self.a;
        PremulColor {
            r: self.r + dst.r * inv,
            g: self.g + dst.g * inv,
            b: self.b + dst.b * inv,
            a: self.a + dst.a * inv,
        }
    }
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_over_anything_replaces_it() {
        let src = PremulColor::from_straight(1.0, 0.0, 0.0, 1.0);
        let dst = PremulColor::from_straight(0.0, 1.0, 0.0, 1.0);
        let result = src.over(dst);
        assert!((result.r - 1.0).abs() < 1e-6);
        assert!((result.g - 0.0).abs() < 1e-6);
    }

    #[test]
    fn transparent_over_anything_is_a_no_op() {
        let dst = PremulColor::from_straight(0.2, 0.4, 0.6, 0.8);
        let result = PremulColor::TRANSPARENT.over(dst);
        assert_eq!(result, dst);
    }

    #[test]
    fn straight_alpha_roundtrip() {
        let c = PremulColor::from_straight(0.5, 0.25, 0.75, 0.4);
        let (r, g, b, a) = c.to_straight();
        assert!((r - 0.5).abs() < 1e-4);
        assert!((g - 0.25).abs() < 1e-4);
        assert!((b - 0.75).abs() < 1e-4);
        assert!((a - 0.4).abs() < 1e-4);
    }
      }
