// render/msx-render-cpu/src/lib.rs
//! Software rasterizer: `tiny-skia` for every SVG-native vector shape,
//! `msx-sdf`/`msx-splat` for the per-pixel math the v0.2 primitives need,
//! `rayon` for parallelizing the parts that are actually slow.
//!
//! `Sdf`, `Splat`, and `Layer` (offscreen buffer + blend mode + opacity +
//! effects, via `composite.rs`/`effects.rs`) are all wired up now.
//!
//! `Text` is still a deliberate no-op — no font shaping/rasterization
//! dependency is wired in (`tiny-skia` doesn't do text at all). Gradient
//! *refs* (`fill = "url(#id)"`) render as a flat average-of-stops color —
//! see the `TODO` in `rasterizer.rs::resolve_paint` for why.

mod composite;
mod effects;
mod geom;
mod image;
mod pixel;
mod rasterizer;
mod sdf_raster;
mod splat_raster;

pub use rasterizer::{Defs, ElementIndex};

use msx_ast::{Matrix2D, Scene};
use msx_render_core::{RenderTarget, Renderer};
use tiny_skia::Pixmap;

pub struct CpuRenderer;

impl CpuRenderer {
    pub fn new() -> Self {
        CpuRenderer
    }
}

impl Default for CpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for CpuRenderer {
    fn render(&self, scene: &Scene, target: &mut RenderTarget) {
        let pixmap = render_to_pixmap(scene);
        copy_into_target(&pixmap, target);
    }
}

/// Render straight to a `tiny_skia::Pixmap` — the natural type for callers
/// already in `tiny-skia` (e.g. `.save_png()` directly), skipping the
/// premultiplied → straight-alpha conversion `RenderTarget` needs.
///
/// A `Def::Shader::source_ref`-style `Element::Image::MediaSource::FileRef`
/// path resolves relative to the current working directory — use
/// `render_to_pixmap_with_base_dir` instead when that's not correct for
/// the caller (this mirrors `msx-render-gpu`'s own `render`/
/// `render_with_shader_dir` split — the base `Renderer` trait doesn't
/// carry a base directory, so a `Def::Shader`/`Element::Image` file
/// reference needs a crate-specific entry point either way).
pub fn render_to_pixmap(scene: &Scene) -> Pixmap {
    render_to_pixmap_with_base_dir(scene, std::path::Path::new("."))
}

/// Same as `render_to_pixmap`, but `Element::Image::MediaSource::FileRef`
/// paths resolve against `base_dir` instead of the current working
/// directory.
pub fn render_to_pixmap_with_base_dir(scene: &Scene, base_dir: &std::path::Path) -> Pixmap {
    let width = scene.canvas.width.round().max(1.0) as u32;
    let height = scene.canvas.height.round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(width, height).expect("non-zero canvas dimensions");

    let bg = scene.canvas.background;
    pixmap.fill(tiny_skia::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a));

    let defs = Defs::build(&scene.defs);
    let index = ElementIndex::build(&scene.elements);

    // `layer_reordered`, not `&scene.elements` directly — see msx-ast's
    // own doc for exactly what this does and doesn't reorder (sibling-
    // scoped by z_index, non-Layer elements untouched).
    for element in msx_ast::layer_reordered(&scene.elements) {
        rasterizer::render_element(&mut pixmap, element, Matrix2D::identity(), &defs, &index, base_dir);
    }

    pixmap
}

fn copy_into_target(pixmap: &Pixmap, target: &mut RenderTarget) {
    let (w, h) = (pixmap.width(), pixmap.height());
    if target.width != w || target.height != h {
        *target = RenderTarget::new(w, h);
    }
    let data = pixmap.data();
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            target.set_pixel(x, y, unpremultiply(data[idx], data[idx + 1], data[idx + 2], data[idx + 3]));
        }
    }
}

fn unpremultiply(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    if a == 0 {
        [0, 0, 0, 0]
    } else {
        let af = a as f32 / 255.0;
        let un = |c: u8| ((c as f32 / 255.0 / af).min(1.0) * 255.0).round() as u8;
        [un(r), un(g), un(b), a]
    }
        }

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Canvas, Circle, Color, Element, Layer, Paint, Rect, Scene, Style};

    fn opaque(color: Color) -> Style {
        Style {
            fill: Some(Paint::Color(color)),
            stroke: Some(Paint::None),
            stroke_width: Some(0.0),
            opacity: Some(1.0),
            ..Default::default()
        }
    }

    #[test]
    fn z_index_flips_which_overlapping_layer_wins_the_pixel() {
        // Two fully-opaque, fully-overlapping rects, each in its own
        // Layer. "front" is authored FIRST (would lose the overlap to
        // "back" under plain document order) but has the higher
        // z_index — the shared center pixel must end up "front"'s
        // color, the opposite of what document order alone would give.
        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));

        let front_rect = Element::Rect(Rect {
            x: 0.0, y: 0.0, width: 20.0, height: 20.0,
            rx: None, ry: None, id: None, transform: None,
            style: opaque(Color::rgb(0, 255, 0)), // green
        });
        let mut front_layer = Layer::new(vec![front_rect]);
        front_layer.z_index = 5.0;

        let back_rect = Element::Rect(Rect {
            x: 0.0, y: 0.0, width: 20.0, height: 20.0,
            rx: None, ry: None, id: None, transform: None,
            style: opaque(Color::rgb(255, 0, 0)), // red
        });
        let mut back_layer = Layer::new(vec![back_rect]);
        back_layer.z_index = 1.0;

        // Document order: front (green, high z) first, back (red, low
        // z) second — the opposite of what should actually win.
        scene.elements.push(Element::Layer(front_layer));
        scene.elements.push(Element::Layer(back_layer));

        let pixmap = render_to_pixmap(&scene);
        let idx = ((10 * 20 + 10) * 4) as usize; // center pixel, premultiplied RGBA
        let px = &pixmap.data()[idx..idx + 4];
        assert_eq!(
            [px[0], px[1], px[2], px[3]],
            [0, 255, 0, 255],
            "expected green (\"front\", the higher z_index) to win the overlap, got {:?}",
            px
        );
    }

    #[test]
    fn equal_z_index_falls_back_to_plain_document_order() {
        // Sanity check for the "no one set z_index" common case: with
        // both Layers at the default 0.0, whichever is defined SECOND
        // still simply paints on top, same as before this feature
        // existed — layer_reordered's stable sort must not perturb
        // untouched z_index ties.
        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));
        let first = Element::Rect(Rect {
            x: 0.0, y: 0.0, width: 20.0, height: 20.0,
            rx: None, ry: None, id: None, transform: None,
            style: opaque(Color::rgb(0, 255, 0)),
        });
        let second = Element::Circle(Circle {
            cx: 10.0, cy: 10.0, r: 10.0,
            id: None, transform: None,
            style: opaque(Color::rgb(255, 0, 0)),
        });
        scene.elements.push(Element::Layer(Layer::new(vec![first])));
        scene.elements.push(Element::Layer(Layer::new(vec![second])));

        let pixmap = render_to_pixmap(&scene);
        let idx = ((10 * 20 + 10) * 4) as usize;
        let px = &pixmap.data()[idx..idx + 4];
        assert_eq!([px[0], px[1], px[2], px[3]], [255, 0, 0, 255], "document-last (red circle) should still win with no z_index set, got {:?}", px);
    }
}
