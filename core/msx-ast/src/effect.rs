// core/msx-ast/src/effect.rs
//! Post-processing effects applied to Layer output.
//!
//! Designed after Grease Pencil's VFX pipeline:
//!   blur → drop shadow → glow are implemented as convolution + offset
//!   compositing passes. CPU path in msx-render-cpu; GPU path in
//!   msx-render-gpu (dedicated render passes, ping-pong FBOs).

use crate::color::Color;

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
