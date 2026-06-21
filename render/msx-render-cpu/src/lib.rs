// render/msx-render-cpu/src/lib.rs
//! Software rasterizer: `tiny-skia` for every SVG-native vector shape,
//! `msx-sdf`/`msx-splat` for the per-pixel math the v0.2 primitives need,
//! `rayon` for parallelizing the parts that are actually slow (per-pixel
//! SDF/splat evaluation — `tiny-skia`'s own path fills are already fast).
//!
//! `Sdf`/`Splat` are fully wired this pass. `Layer` is still accepted by
//! the dispatch match (keeps it exhaustive) but renders nothing — that's
//! `effects.rs`/`composite.rs`, next.
//!
//! `Text` is a deliberate no-op — no font shaping/rasterization dependency
//! is wired in (`tiny-skia` doesn't do text at all).

mod geom;
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
pub fn render_to_pixmap(scene: &Scene) -> Pixmap {
    let width = scene.canvas.width.round().max(1.0) as u32;
    let height = scene.canvas.height.round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(width, height).expect("non-zero canvas dimensions");

    let bg = scene.canvas.background;
    pixmap.fill(tiny_skia::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a));

    let defs = Defs::build(&scene.defs);
    let index = ElementIndex::build(&scene.elements);

    for element in &scene.elements {
        rasterizer::render_element(&mut pixmap, element, Matrix2D::identity(), &defs, &index);
    }

    pixmap
}

fn copy_into_target(pixmap: &Pixmap, target: &mut RenderTarget) {
    let (w, h) = (pixmap.width(), pixmap.height());
    if target.width != w || target.height != h {
        *target = RenderTarget::new(w, h);
    }
    let data = pixmap.data(); // premultiplied RGBA8, row-major
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
