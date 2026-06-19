// core/msx-ast/src/layer.rs
//! Layer — Group variant with explicit blend mode and post-processing effects.
//!
//! Design directly mirrors Grease Pencil's layer system:
//!   - Each layer renders to its own offscreen buffer.
//!   - Blend mode is applied when compositing onto the parent buffer.
//!   - Layers that need a signed buffer (Subtract, Divide, Difference) set
//!     `requires_signed_buffer()`, matching Grease Pencil's `use_signed_fb`.
//!   - Effects are applied after compositing, using a ping-pong FBO chain
//!     (Grease Pencil's vfx swapchain pattern).

use crate::effect::Effect;
use crate::element::Element;
use crate::transform::Transform;

/// Layer compositing blend mode.
/// Values match DixScript @ENUMS(BlendMode) in std/enums.msx.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BlendMode {
    #[default]
    Normal     = 0,
    Multiply   = 1,
    Screen     = 2,
    Overlay    = 3,
    Add        = 4,
    SoftLight  = 5,
    HardLight  = 6,
    Difference = 7,
    Exclusion  = 8,
    Darken     = 9,
    Lighten    = 10,
    Subtract   = 11,  // Requires signed buffer — mirrors GP's subtract blend
    Divide     = 12,  // Requires signed buffer — mirrors GP's divide blend
}

impl BlendMode {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1  => BlendMode::Multiply,
            2  => BlendMode::Screen,
            3  => BlendMode::Overlay,
            4  => BlendMode::Add,
            5  => BlendMode::SoftLight,
            6  => BlendMode::HardLight,
            7  => BlendMode::Difference,
            8  => BlendMode::Exclusion,
            9  => BlendMode::Darken,
            10 => BlendMode::Lighten,
            11 => BlendMode::Subtract,
            12 => BlendMode::Divide,
            _  => BlendMode::Normal,
        }
    }

    pub fn to_byte(self) -> u8 { self as u8 }

    /// CSS `mix-blend-mode` value for SVG export (None = use default/normal).
    pub fn to_css_blend_mode(self) -> Option<&'static str> {
        match self {
            BlendMode::Normal     => None,
            BlendMode::Multiply   => Some("multiply"),
            BlendMode::Screen     => Some("screen"),
            BlendMode::Overlay    => Some("overlay"),
            BlendMode::Add        => Some("plus-lighter"),
            BlendMode::SoftLight  => Some("soft-light"),
            BlendMode::HardLight  => Some("hard-light"),
            BlendMode::Difference => Some("difference"),
            BlendMode::Exclusion  => Some("exclusion"),
            BlendMode::Darken     => Some("darken"),
            BlendMode::Lighten    => Some("lighten"),
            BlendMode::Subtract   => None, // No SVG 1.1 equivalent
            BlendMode::Divide     => None,
        }
    }

    /// Whether this mode requires a signed (SFLOAT) intermediate buffer.
    /// Taken from Grease Pencil's `use_signed_fb` logic.
    pub fn requires_signed_buffer(self) -> bool {
        matches!(self, BlendMode::Subtract | BlendMode::Divide | BlendMode::Difference | BlendMode::Exclusion)
    }
}

/// A compositing layer in the element tree.
///
/// Renders children to an offscreen buffer, then composites using blend_mode
/// and opacity. Effects are applied to the composited result in order via a
/// ping-pong FBO chain before final compositing.
#[derive(Debug, Clone)]
pub struct Layer {
    pub children:   Vec<Element>,
    pub blend_mode: BlendMode,
    pub opacity:    f64,
    /// Clip children to this layer's pixel footprint.
    pub clip:       bool,
    /// Post-processing effects (blur, shadow, glow) applied in order.
    pub effects:    Vec<Effect>,
    pub id:         Option<String>,
    pub transform:  Option<Transform>,
}

impl Layer {
    pub fn new(children: Vec<Element>) -> Self {
        Layer {
            children,
            blend_mode: BlendMode::Normal,
            opacity:    1.0,
            clip:       false,
            effects:    Vec::new(),
            id:         None,
            transform:  None,
        }
    }

    pub fn with_blend(mut self, mode: BlendMode) -> Self { self.blend_mode = mode; self }
    pub fn with_opacity(mut self, op: f64)        -> Self { self.opacity = op; self }
    pub fn with_effect(mut self, fx: Effect)      -> Self { self.effects.push(fx); self }
  }
