// render/msx-render-cpu/src/composite.rs
//! `Layer` rendering: render children to an offscreen pixmap the same size
//! as the canvas, run any effects over that buffer (`effects.rs`), then
//! composite it back onto the parent pixmap using the layer's blend mode
//! and opacity — via `msx-render-core::composite`, the same blend math
//! `msx-render-svg`'s CSS fallback is approximating.
//!
//! `clip_path` (the union of a list of shapes) is a coverage mask, built from
//! the shapes in the layer's own coordinate space and multiplied into the
//! buffer after the effects. `clip` (clip children to the layer's pixel
//! footprint) has no implementation here yet, same gap `msx-render-svg` has.

use msx_ast::{BlendMode, Layer, Matrix2D};
use rayon::prelude::*;
use tiny_skia::Pixmap;

use crate::effects::apply_effects;
use crate::pixel::{read_premul, write_premul};
use crate::rasterizer::{clip_mask, render_element, Defs, ElementIndex};
use msx_render_core::PremulColor;

pub fn render_layer(pixmap: &mut Pixmap, layer: &Layer, parent: Matrix2D, defs: &Defs, index: &ElementIndex, base_dir: &std::path::Path) {
    let local = layer.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    let combined = parent.concat(local);

    let Some(mut buffer) = Pixmap::new(pixmap.width(), pixmap.height()) else { return };

    for child in msx_ast::layer_reordered(&layer.children) {
        render_element(&mut buffer, child, combined, defs, index, base_dir);
    }

    apply_effects(&mut buffer, &layer.effects);

    if let Some(shapes) = &layer.clip_path {
        apply_clip(&mut buffer, shapes, combined, index);
    }

    composite_onto(pixmap, &buffer, layer.blend_mode, layer.opacity.clamp(0.0, 1.0) as f32);
}

/// Multiplies every pixel of `buffer` by the coverage of `shapes`. The buffer
/// is premultiplied, so scaling all four channels by the same factor keeps it
/// premultiplied.
fn apply_clip(buffer: &mut Pixmap, shapes: &[msx_ast::Element], matrix: Matrix2D, index: &ElementIndex) {
    let Some(mask) = clip_mask(buffer.width(), buffer.height(), shapes, matrix, index) else { return };
    for (px, &cover) in buffer.data_mut().chunks_exact_mut(4).zip(mask.data()) {
        if cover == 255 {
            continue;
        }
        let f = cover as u32;
        for c in px {
            *c = ((*c as u32 * f + 127) / 255) as u8;
        }
    }
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
        render_element(&mut direct, &circle_a, Matrix2D::identity(), &defs, &empty_index, std::path::Path::new("."));

        let circle_b = solid_circle(20.0, 20.0, 10.0, Color::rgb(200, 50, 50));
        let layer = Layer::new(vec![circle_b]);
        render_layer(&mut via_layer, &layer, Matrix2D::identity(), &defs, &empty_index, std::path::Path::new("."));

        assert_eq!(direct.data(), via_layer.data());
    }

    fn rect_el(x: f64, y: f64, w: f64, h: f64, color: Color) -> Element {
        let style = Style { fill: Some(Paint::Color(color)), stroke: Some(Paint::None), stroke_width: Some(0.0), opacity: Some(1.0), ..Default::default() };
        Element::Rect(msx_ast::Rect { x, y, width: w, height: h, rx: None, ry: None, id: None, transform: None, style })
    }

    fn alpha_at(p: &Pixmap, x: u32, y: u32) -> u8 {
        p.pixel(x, y).map(|c| c.alpha()).unwrap_or(0)
    }

    fn render_clipped(layer: &Layer) -> Pixmap {
        let mut pm = Pixmap::new(40, 40).unwrap();
        let defs = Defs::build(&[]);
        let index = ElementIndex::build(&[]);
        render_layer(&mut pm, layer, Matrix2D::identity(), &defs, &index, std::path::Path::new("."));
        pm
    }

    #[test]
    fn clip_path_keeps_only_the_covered_part_of_the_layer() {
        // A red layer over the whole canvas, clipped to a 20 by 20 square.
        let layer = Layer::new(vec![rect_el(0.0, 0.0, 40.0, 40.0, Color::rgb(255, 0, 0))]).with_clip_path(vec![rect_el(10.0, 10.0, 20.0, 20.0, Color::BLACK)]);
        let pm = render_clipped(&layer);
        assert_eq!(pm.pixel(20, 20).unwrap().red(), 255);
        assert_eq!(alpha_at(&pm, 20, 20), 255);
        assert_eq!(alpha_at(&pm, 5, 5), 0);
        assert_eq!(alpha_at(&pm, 35, 20), 0);
    }

    #[test]
    fn clip_path_shapes_union_and_ignore_their_own_paint() {
        // The second square is stroke only and fully transparent; it still adds area.
        let mut ghost = rect_el(25.0, 0.0, 10.0, 10.0, Color::BLACK);
        if let Element::Rect(r) = &mut ghost {
            r.style.fill = Some(Paint::None);
            r.style.opacity = Some(0.0);
        }
        let layer = Layer::new(vec![rect_el(0.0, 0.0, 40.0, 40.0, Color::rgb(0, 0, 255))])
            .with_clip_path(vec![rect_el(0.0, 0.0, 10.0, 10.0, Color::BLACK), ghost]);
        let pm = render_clipped(&layer);
        assert_eq!(alpha_at(&pm, 5, 5), 255);
        assert_eq!(alpha_at(&pm, 30, 5), 255, "the second shape's own paint does not matter");
        assert_eq!(alpha_at(&pm, 15, 5), 0, "the gap between the two shapes stays clipped");
    }

    #[test]
    fn empty_clip_path_clips_everything_and_no_clip_path_clips_nothing() {
        let kids = vec![rect_el(0.0, 0.0, 40.0, 40.0, Color::rgb(0, 200, 0))];
        let none = render_clipped(&Layer::new(kids.clone()).with_clip_path(vec![]));
        assert!(none.data().iter().all(|&b| b == 0));
        let all = render_clipped(&Layer::new(kids));
        assert_eq!(alpha_at(&all, 0, 0), 255);
        assert_eq!(alpha_at(&all, 39, 39), 255);
    }

    #[test]
    fn clip_path_uses_the_layer_transform_and_clips_the_edge_softly() {
        // The layer moves by (10, 10); the clip circle is centered at (10, 10) in layer space.
        let mut layer = Layer::new(vec![rect_el(-10.0, -10.0, 60.0, 60.0, Color::rgb(255, 0, 0))])
            .with_clip_path(vec![solid_circle(10.0, 10.0, 8.0, Color::BLACK)]);
        layer.transform = Some(msx_ast::Transform::Translate { x: 10.0, y: 10.0 });
        let pm = render_clipped(&layer);
        assert_eq!(alpha_at(&pm, 20, 20), 255, "the clip center lands at (20, 20)");
        assert_eq!(alpha_at(&pm, 5, 5), 0);
        let edge = alpha_at(&pm, 25, 25);
        assert!(edge > 0 && edge < 255, "the circle edge is antialiased, got {edge}");
    }

    #[test]
    fn clip_path_with_even_odd_leaves_a_hole() {
        let mut ring = vec![];
        let style = Style { fill_rule: Some(msx_ast::FillRule::EvenOdd), ..Style::default() };
        ring.push(Element::Path(msx_ast::Path {
            commands: msx_ast::path::parse_d("M0 0H40V40H0Z M10 10H30V30H10Z").unwrap(),
            d_raw: "M0 0H40V40H0Z M10 10H30V30H10Z".into(),
            id: None,
            transform: None,
            style,
        }));
        let layer = Layer::new(vec![rect_el(0.0, 0.0, 40.0, 40.0, Color::rgb(255, 0, 0))]).with_clip_path(ring);
        let pm = render_clipped(&layer);
        assert_eq!(alpha_at(&pm, 3, 3), 255);
        assert_eq!(alpha_at(&pm, 20, 20), 0, "even-odd leaves the inner square out");
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

        render_layer(&mut pixmap, &layer, Matrix2D::identity(), &defs, &index, std::path::Path::new("."));
        assert_eq!(pixmap.data(), before.as_slice());
    }
        }
