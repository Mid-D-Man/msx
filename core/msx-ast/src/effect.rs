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
    /// Wire version 3.
    BlurXY      = 5,
    /// Wire version 3.
    ColorMatrix = 6,
}

impl EffectType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(EffectType::Blur),
            1 => Some(EffectType::DropShadow),
            2 => Some(EffectType::InnerShadow),
            3 => Some(EffectType::OuterGlow),
            4 => Some(EffectType::InnerGlow),
            5 => Some(EffectType::BlurXY),
            6 => Some(EffectType::ColorMatrix),
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

    /// Gaussian blur with a standard deviation for each axis (`feGaussianBlur`
    /// with two values). `Blur` is the case where both are equal. A zero on
    /// one axis blurs only the other. Wire version 3.
    BlurXY { rx: f64, ry: f64 },

    /// A 4x5 color matrix on the layer output (`feColorMatrix`, and the CSS
    /// functions `grayscale()`, `sepia()`, `saturate()`, `hue-rotate()`,
    /// `brightness()`, `contrast()`, `invert()` and `opacity()`). `values` is
    /// row major: the rows give the new red, green, blue and alpha, and the
    /// columns weigh the old red, green, blue, alpha and a constant, so
    /// `r' = v[0] r + v[1] g + v[2] b + v[3] a + v[4]`. It works on straight
    /// (not premultiplied) channels in 0 to 1, and the results are held to 0
    /// to 1. `linear` runs it on linear light instead of sRGB values, which is
    /// the default of `feColorMatrix`. Wire version 3.
    ColorMatrix { values: [f64; 20], linear: bool },
}

impl Effect {
    pub fn effect_type(&self) -> EffectType {
        match self {
            Effect::Blur { .. }        => EffectType::Blur,
            Effect::DropShadow { .. }  => EffectType::DropShadow,
            Effect::InnerShadow { .. } => EffectType::InnerShadow,
            Effect::OuterGlow { .. }   => EffectType::OuterGlow,
            Effect::InnerGlow { .. }   => EffectType::InnerGlow,
            Effect::BlurXY { .. }      => EffectType::BlurXY,
            Effect::ColorMatrix { .. } => EffectType::ColorMatrix,
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
            Effect::BlurXY { rx, ry } => {
                // The blur runs on the device axes. A matrix that keeps the
                // axes (scale, flip, quarter turn) scales each amount by the
                // length of the axis it lands on. Any other matrix has no
                // axis aligned answer, so both amounts use the mean scale.
                let (sx, sy) = (m.a.hypot(m.b), m.c.hypot(m.d));
                let keeps_axes = (m.b.abs() < 1e-9 && m.c.abs() < 1e-9) || (m.a.abs() < 1e-9 && m.d.abs() < 1e-9);
                if !keeps_axes {
                    Effect::BlurXY { rx: rx * s, ry: ry * s }
                } else if m.b.abs() < 1e-9 {
                    Effect::BlurXY { rx: rx * sx, ry: ry * sy }
                } else {
                    Effect::BlurXY { rx: ry * sy, ry: rx * sx }
                }
            }
            Effect::ColorMatrix { .. } => self.clone(),
        }
    }

    /// Pixel extent the effect can reach outside the layer bounds (for bounds expansion).
    pub fn max_extent(&self) -> f64 {
        match self {
            Effect::Blur { radius }                                                => radius * 3.0,
            Effect::DropShadow { offset_x, offset_y, blur_radius, .. }           => offset_x.abs().max(offset_y.abs()) + blur_radius * 3.0,
            Effect::OuterGlow { blur_radius, spread, .. }                         => blur_radius * 3.0 + spread,
            Effect::InnerShadow { .. } | Effect::InnerGlow { .. }                 => 0.0,
            Effect::BlurXY { rx, ry }                                              => rx.max(*ry) * 3.0,
            Effect::ColorMatrix { .. }                                             => 0.0,
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

    #[test]
    fn blur_xy_scales_each_axis_by_the_axis_it_lands_on() {
        let fx = Effect::BlurXY { rx: 2.0, ry: 5.0 };
        let Effect::BlurXY { rx, ry } = fx.scaled(&Matrix2D::scale(3.0, 2.0)) else { unreachable!() };
        assert!(close(rx, 6.0) && close(ry, 10.0));
        // A quarter turn puts the user y axis on the device x axis.
        let Effect::BlurXY { rx, ry } = fx.scaled(&Matrix2D::rotate_deg(90.0)) else { unreachable!() };
        assert!(close(rx, 5.0) && close(ry, 2.0), "{rx} {ry}");
        // No axis aligned answer for a turn of 30 degrees: the mean scale.
        let Effect::BlurXY { rx, ry } = fx.scaled(&Matrix2D::rotate_deg(30.0)) else { unreachable!() };
        assert!(close(rx, 2.0) && close(ry, 5.0));
    }

    #[test]
    fn color_matrix_is_not_moved_by_the_layer_matrix() {
        let fx = Effect::ColorMatrix { values: [0.5; 20], linear: true };
        assert_eq!(fx.scaled(&Matrix2D::scale(4.0, 4.0)), fx);
        assert_eq!(fx.max_extent(), 0.0);
        assert_eq!(Effect::BlurXY { rx: 1.0, ry: 4.0 }.max_extent(), 12.0);
    }

    #[test]
    fn effect_types_round_trip_through_bytes() {
        for b in 0..=6u8 {
            assert_eq!(EffectType::from_byte(b).unwrap() as u8, b);
        }
        assert_eq!(EffectType::from_byte(7), None);
        assert_eq!(Effect::BlurXY { rx: 1.0, ry: 2.0 }.effect_type(), EffectType::BlurXY);
        assert_eq!(Effect::ColorMatrix { values: [0.0; 20], linear: false }.effect_type(), EffectType::ColorMatrix);
    }
}
