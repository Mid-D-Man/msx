// core/msx-ast/src/effect.rs
//! Post-processing effects applied to Layer output.
//!
//! Designed after Grease Pencil's VFX pipeline:
//!   blur → drop shadow → glow are implemented as convolution + offset
//!   compositing passes. CPU path in msx-render-cpu; GPU path in
//!   msx-render-gpu (dedicated render passes, ping-pong FBOs).

use crate::color::Color;
use crate::transform::Matrix2D;

/// Discriminant for binary encoding. Maps to DixScript @ENUMS(EffectType)
/// in std/enums.msx.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EffectType {
    Blur        = 0,
    DropShadow  = 1,
    InnerShadow = 2,
    OuterGlow   = 3,
    InnerGlow   = 4,
}

impl EffectType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(EffectType::Blur),
            1 => Some(EffectType::DropShadow),
            2 => Some(EffectType::InnerShadow),
            3 => Some(EffectType::OuterGlow),
            4 => Some(EffectType::InnerGlow),
            _ => None,
        }
    }
}

/// A post-processing effect applied to a composited Layer's pixel output.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Gaussian blur — applied to the entire layer output.
    Blur { radius: f64 },

    /// Shadow rendered behind the layer via offset + blur + alpha-composite.
    DropShadow {
        offset_x:    f64,
        offset_y:    f64,
        blur_radius: f64,
        color:       Color,
        opacity:     f64,
    },

    /// Shadow rendered inside the layer using an inverted alpha mask.
    InnerShadow {
        offset_x:    f64,
        offset_y:    f64,
        blur_radius: f64,
        color:       Color,
        opacity:     f64,
    },

    /// Glow bloom composited *outside* the layer boundary.
    OuterGlow {
        color:       Color,
        blur_radius: f64,
        spread:      f64,
        opacity:     f64,
    },

    /// Glow bloom composited *inside* the layer boundary.
    InnerGlow {
        color:       Color,
        blur_radius: f64,
        opacity:     f64,
    },
}

impl Effect {
    pub fn effect_type(&self) -> EffectType {
        match self {
            Effect::Blur { .. }        => EffectType::Blur,
            Effect::DropShadow { .. }  => EffectType::DropShadow,
            Effect::InnerShadow { .. } => EffectType::InnerShadow,
            Effect::OuterGlow { .. }   => EffectType::OuterGlow,
            Effect::InnerGlow { .. }   => EffectType::InnerGlow,
        }
    }

    /// The effect in device units, for a layer whose coordinate system maps to
    /// the device through `m`. Distances (radii, spread) are multiplied by the
    /// uniform scale `sqrt(|det|)` and offsets go through the linear part of
    /// the matrix, so a shadow of an element rotated by 90 degrees falls the
    /// way the element sees it. Colors and opacities are unchanged.
    pub fn scaled(&self, m: &Matrix2D) -> Effect {
        let s = (m.a * m.d - m.b * m.c).abs().sqrt();
        let off = |dx: f64, dy: f64| (m.a * dx + m.c * dy, m.b * dx + m.d * dy);
        match self {
            Effect::Blur { radius } => Effect::Blur { radius: radius * s },
            Effect::DropShadow { offset_x, offset_y, blur_radius, color, opacity } => {
                let (ox, oy) = off(*offset_x, *offset_y);
                Effect::DropShadow { offset_x: ox, offset_y: oy, blur_radius: blur_radius * s, color: *color, opacity: *opacity }
            }
            Effect::InnerShadow { offset_x, offset_y, blur_radius, color, opacity } => {
                let (ox, oy) = off(*offset_x, *offset_y);
                Effect::InnerShadow { offset_x: ox, offset_y: oy, blur_radius: blur_radius * s, color: *color, opacity: *opacity }
            }
            Effect::OuterGlow { color, blur_radius, spread, opacity } => {
                Effect::OuterGlow { color: *color, blur_radius: blur_radius * s, spread: spread * s, opacity: *opacity }
            }
            Effect::InnerGlow { color, blur_radius, opacity } => {
                Effect::InnerGlow { color: *color, blur_radius: blur_radius * s, opacity: *opacity }
            }
        }
    }

    /// Pixel extent the effect can reach outside the layer bounds (for bounds expansion).
    pub fn max_extent(&self) -> f64 {
        match self {
            Effect::Blur { radius }                                                => radius * 3.0,
            Effect::DropShadow { offset_x, offset_y, blur_radius, .. }           => offset_x.abs().max(offset_y.abs()) + blur_radius * 3.0,
            Effect::OuterGlow { blur_radius, spread, .. }                         => blur_radius * 3.0 + spread,
            Effect::InnerShadow { .. } | Effect::InnerGlow { .. }                 => 0.0,
        }
    }
      }

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn identity_leaves_every_effect_alone() {
        let fx = Effect::DropShadow { offset_x: 3.0, offset_y: -2.0, blur_radius: 4.0, color: Color::BLACK, opacity: 0.5 };
        assert_eq!(fx.scaled(&Matrix2D::identity()), fx);
        let glow = Effect::OuterGlow { color: Color::WHITE, blur_radius: 2.0, spread: 1.0, opacity: 1.0 };
        assert_eq!(glow.scaled(&Matrix2D::identity()), glow);
    }

    #[test]
    fn uniform_scale_multiplies_distances() {
        let m = Matrix2D::scale(2.0, 2.0);
        assert_eq!(Effect::Blur { radius: 3.0 }.scaled(&m), Effect::Blur { radius: 6.0 });
        let Effect::OuterGlow { blur_radius, spread, .. } = (Effect::OuterGlow { color: Color::WHITE, blur_radius: 2.0, spread: 1.0, opacity: 1.0 }).scaled(&m) else { unreachable!() };
        assert!(close(blur_radius, 4.0) && close(spread, 2.0));
    }

    #[test]
    fn translation_does_not_move_the_blur() {
        assert_eq!(Effect::Blur { radius: 3.0 }.scaled(&Matrix2D::translate(50.0, 20.0)), Effect::Blur { radius: 3.0 });
    }

    #[test]
    fn rotation_turns_the_offset_but_keeps_the_radius() {
        let m = Matrix2D::rotate_deg(90.0);
        let fx = Effect::DropShadow { offset_x: 10.0, offset_y: 0.0, blur_radius: 2.0, color: Color::BLACK, opacity: 1.0 };
        let Effect::DropShadow { offset_x, offset_y, blur_radius, .. } = fx.scaled(&m) else { unreachable!() };
        assert!(close(offset_x, 0.0) && close(offset_y, 10.0) && close(blur_radius, 2.0));
    }
}
