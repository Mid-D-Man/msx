// render/msx-render-cpu/src/composite.rs
//! `Layer` rendering: render children to an offscreen pixmap the same size
//! as the canvas, run any effects over that buffer (`effects.rs`), then
//! composite it back onto the parent pixmap using the layer's blend mode
//! and opacity — via `msx-render-core::composite`, the same blend math
//! `msx-render-svg`'s CSS fallback is approximating.
//!
//! `clip` (clip children to the layer's pixel footprint) has no
//! implementation here yet — same gap `msx-render-svg` has, same reason
//! (needs a bbox/clipPath mechanism this pass doesn't build).

use msx_ast::{BlendMode, Layer, Matrix2D};
use rayon::prelude::*;
use tiny_skia::Pixmap;

use crate::effects::apply_effects;
use crate::pixel::{read_premul, write_premul};
use crate::rasterizer::{render_element, Defs, ElementIndex};
use msx_render_core::PremulColor;

pub fn render_layer(pixmap: &mut Pixmap, layer: &Layer, parent: Matrix2D, defs: &Defs, index: &ElementIndex) {
    let local = layer.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    let combined = parent.concat(local);

    let Some(mut buffer) = Pixmap::new(pixmap.width(), pixmap.height()) else { return };

    for child in &layer.children {
        render_element(&mut buffer, child, combined, defs, index);
    }

    apply_effects(&mut buffer, &layer.effects);

    composite_onto(pixmap, &buffer, layer.blend_mode, layer.opacity.clamp(0.0, 1.0) as f32);
}

fn composite_onto(parent: &mut Pixmap, layer_buffer: &Pixmap, blend_mode: BlendMode, opacity: f32) {
    let layer_data = layer_buffer.data();
    let parent_data = parent.data_mut();

    parent_data.par_chunks_mut(4).zip(layer_data.par_chunks(4)).for_each(|(dst, src)| {
        let backdrop = read_premul(dst, 0);
        let mut source = read_premul(src, 0);
        source.r *= opacity;
        source.g *= opacity;
        source.b *= opacity;
        source.a *= opacity;

        let (r, g, b, a) = msx_render_core::composite(blend_mode, backdrop.to_straight(), source.to_straight());
        write_premul(dst, 0, PremulColor::from_straight(r, g, b, a));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Circle, Color, Element, Paint, Style};

    fn solid_circle(cx: f64, cy: f64, r: f64, color: Color) -> Element {
        let style = Style {
            fill: Some(Paint::Color(color)),
            stroke: Some(Paint::None),
            stroke_width: Some(0.0),
            opacity: Some(1.0),
            ..Default::default()
        };
        Element::Circle(Circle { cx, cy, r, id: None, transform: None, style })
    }

    #[test]
    fn layer_at_full_opacity_normal_blend_matches_direct_draw() {
        let mut direct = Pixmap::new(40, 40).unwrap();
        let mut via_layer = Pixmap::new(40, 40).unwrap();
        let defs = Defs::build(&[]);
        let empty_index = ElementIndex::build(&[]);

        let circle_a = solid_circle(20.0, 20.0, 10.0, Color::rgb(200, 50, 50));
        render_element(&mut direct, &circle_a, Matrix2D::identity(), &defs, &empty_index);

        let circle_b = solid_circle(20.0, 20.0, 10.0, Color::rgb(200, 50, 50));
        let layer = Layer::new(vec![circle_b]);
        render_layer(&mut via_layer, &layer, Matrix2D::identity(), &defs, &empty_index);

        assert_eq!(direct.data(), via_layer.data());
    }

    #[test]
    fn zero_opacity_layer_leaves_backdrop_untouched() {
        let mut pixmap = Pixmap::new(20, 20).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        let before = pixmap.data().to_vec();

        let defs = Defs::build(&[]);
        let index = ElementIndex::build(&[]);
        let circle = solid_circle(10.0, 10.0, 5.0, Color::WHITE);
        let mut layer = Layer::new(vec![circle]);
        layer.opacity = 0.0;

        render_layer(&mut pixmap, &layer, Matrix2D::identity(), &defs, &index);
        assert_eq!(pixmap.data(), before.as_slice());
    }
        }
