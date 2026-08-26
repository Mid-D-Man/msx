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
    /// Paint order among sibling top-level Layers — higher draws later
    /// (i.e. on top), ties broken by document order (a stable sort, same
    /// tie-break convention as CSS `z-index`). Purely relative: only
    /// compared against other Layers, not against ordinary (non-Layer)
    /// elements — those still always draw before every Layer regardless
    /// of z_index, see this crate's renderer-side module docs for why
    /// that's a separate, larger gap this field doesn't attempt to close.
    /// Animatable via `AnimatedProperty::ZIndex` — see `core/msx-anim`'s
    /// `AnimatedDelta` for why it composes as a plain override rather
    /// than the additive/multiplicative model every other channel uses.
    pub z_index:    f64,
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
            z_index:    0.0,
        }
    }

    pub fn with_blend(mut self, mode: BlendMode)   -> Self { self.blend_mode = mode; self }
    pub fn with_opacity(mut self, op: f64)          -> Self { self.opacity = op; self }
    pub fn with_effect(mut self, fx: Effect)        -> Self { self.effects.push(fx); self }
    pub fn with_z_index(mut self, z: f64)           -> Self { self.z_index = z; self }
  }

/// Returns `elements` in the order a renderer should actually draw them:
/// every non-`Layer` element stays in its original slot, but the *set*
/// of `Layer`-occupied slots gets refilled with those same Layers
/// stable-sorted by `z_index` (ties keep their original relative order —
/// same tie-break convention CSS `z-index` uses).
///
/// Deliberately scoped to exactly this one slice, not the whole element
/// tree: a `Layer` only reorders relative to its *immediate* siblings in
/// `elements`, the same list a caller would otherwise have iterated
/// directly. `msx-render-cpu` and `msx-render-svg` both recurse through
/// `Group` children with plain per-element dispatch, calling this once
/// per sibling list — `Scene::elements` itself, and again inside every
/// `Group::children` — which gives each renderer the right per-level
/// ordering without needing to know about tree depth at all.
/// `msx-render-gpu` recurses the same way, just through its own
/// `paint_order`/`render_ops` (its `PaintOp::GroupSplit`, specifically —
/// see that crate's `layer.rs` module doc), since a GPU `Layer` needs a
/// real isolated-buffer-plus-composite pass rather than the plain
/// recursive dispatch SVG/CPU can get away with; all three renderers
/// agree on the same nesting model as a result: a nested `Layer`'s
/// `z_index` resolves purely against its own sibling list, at its own
/// nesting level, before the level containing it does.
///
/// A single Layer (or none) short-circuits without allocating a sorted
/// copy, since there's nothing to reorder relative to.
pub fn layer_reordered(elements: &[Element]) -> Vec<&Element> {
    let mut out: Vec<&Element> = elements.iter().collect();

    let layer_slots: Vec<usize> = elements
        .iter()
        .enumerate()
        .filter_map(|(i, e)| matches!(e, Element::Layer(_)).then_some(i))
        .collect();
    if layer_slots.len() <= 1 {
        return out;
    }

    let mut layers_by_z: Vec<&Element> = layer_slots.iter().map(|&i| &elements[i]).collect();
    layers_by_z.sort_by(|a, b| {
        let za = if let Element::Layer(l) = a { l.z_index } else { 0.0 };
        let zb = if let Element::Layer(l) = b { l.z_index } else { 0.0 };
        za.partial_cmp(&zb).unwrap_or(std::cmp::Ordering::Equal)
    });

    for (slot, sorted_layer) in layer_slots.into_iter().zip(layers_by_z) {
        out[slot] = sorted_layer;
    }
    out
}

#[cfg(test)]
mod layer_reorder_tests {
    use super::*;
    use crate::element::Element;

    fn layer_with(id: &str, z: f64) -> Element {
        let mut l = Layer::new(vec![]).with_z_index(z);
        l.id = Some(id.to_string());
        Element::Layer(l)
    }

    fn id_of(e: &Element) -> &str {
        match e {
            Element::Layer(l) => l.id.as_deref().unwrap(),
            _ => panic!("expected Layer"),
        }
    }

    #[test]
    fn zero_or_one_layer_is_returned_unchanged() {
        let elements = vec![layer_with("only", 5.0)];
        let out = layer_reordered(&elements);
        assert_eq!(out.len(), 1);
        assert_eq!(id_of(out[0]), "only");
    }

    #[test]
    fn two_layers_reorder_by_z_index_regardless_of_document_order() {
        // "front" is defined first (would paint under "back" in plain
        // document order) but has the higher z_index, so it must end up
        // in the later slot instead.
        let elements = vec![layer_with("front", 5.0), layer_with("back", 1.0)];
        let out = layer_reordered(&elements);
        assert_eq!(id_of(out[0]), "back");
        assert_eq!(id_of(out[1]), "front");
    }

    #[test]
    fn equal_z_index_keeps_original_document_order() {
        let elements = vec![layer_with("a", 3.0), layer_with("b", 3.0), layer_with("c", 3.0)];
        let out = layer_reordered(&elements);
        assert_eq!([id_of(out[0]), id_of(out[1]), id_of(out[2])], ["a", "b", "c"]);
    }

    #[test]
    fn non_layer_elements_keep_their_exact_slots() {
        use crate::{Rect, Style};
        let rect_a = Element::Rect(Rect::new(0.0, 0.0, 1.0, 1.0, Style::default()));
        let rect_b = Element::Rect(Rect::new(0.0, 0.0, 2.0, 2.0, Style::default()));
        let elements = vec![rect_a, layer_with("front", 9.0), rect_b, layer_with("back", 0.0)];

        let out = layer_reordered(&elements);
        assert!(matches!(out[0], Element::Rect(r) if r.width == 1.0), "slot 0 (non-layer) must be untouched");
        assert!(matches!(out[2], Element::Rect(r) if r.width == 2.0), "slot 2 (non-layer) must be untouched");
        // The two layer slots (1 and 3) swap to reflect z_index: "back"
        // (z=0.0) now sits where "front" (z=9.0, document-first) was,
        // and vice versa.
        assert_eq!(id_of(out[1]), "back");
        assert_eq!(id_of(out[3]), "front");
    }
}
