// render/msx-render-gpu/src/lib.rs
//! GPU renderer — wgpu-backed, offscreen-only (no window `Surface`; that's
//! `apps/msx-viewer`'s job once it exists).
//!
//! ## Honesty about scope and risk
//!
//! This is the highest API-risk crate in the project, by a wide margin.
//! wgpu ships a breaking release roughly every three months, and the
//! project's original plan pinned `wgpu = "0.19"` — current is **29.0.3**,
//! over two years and a full versioning-scheme change later. Every wgpu
//! signature here was checked against current (~April 2026) docs/tutorials
//! rather than pulled from training-data memory; `pipeline.rs`/`sdf.rs`'s
//! `immediate_size` field is flagged inline as the one spot I'm least sure
//! about. `lyon`'s tessellation API, by contrast, checked out essentially
//! unchanged from what I expected.
//!
//! `Vector` shapes and `Sdf` are both wired up now — `Sdf` via a flattened
//! postorder op list evaluated with an explicit stack in WGSL, since the
//! shader language has no recursion for the actual recursive `SdfTree`.
//! `Splat` and `Layer` are still accepted by the dispatch match in
//! `vector.rs` (keeps it exhaustive) but render nothing. `Text` is a
//! deliberate no-op everywhere in this project — no font backend wired in.

mod context;
mod pipeline;
mod sdf;
mod target;
mod vector;

pub use context::GpuContext;
pub use pipeline::VectorPipeline;
pub use sdf::SdfPipeline;
pub use target::OffscreenTarget;
pub use vector::{tessellate_scene, Vertex, VectorGeometry};

use msx_ast::Scene;
use msx_render_core::{RenderTarget, Renderer};

pub struct GpuRenderer {
    context: GpuContext,
    vector_pipeline: VectorPipeline,
    sdf_pipeline: SdfPipeline,
}

impl GpuRenderer {
    pub fn new() -> Result<Self, String> {
        let context = GpuContext::new()?;
        let vector_pipeline = VectorPipeline::new(&context.device, wgpu::TextureFormat::Rgba8Unorm);
        let sdf_pipeline = SdfPipeline::new(&context.device, wgpu::TextureFormat::Rgba8Unorm);
        Ok(GpuRenderer { context, vector_pipeline, sdf_pipeline })
    }
}

impl Renderer for GpuRenderer {
    fn render(&self, scene: &Scene, target: &mut RenderTarget) {
        let width = scene.canvas.width.round().max(1.0) as u32;
        let height = scene.canvas.height.round().max(1.0) as u32;

        let offscreen = OffscreenTarget::new(&self.context.device, width, height);
        let geometry = vector::tessellate_scene(scene);

        let bg = scene.canvas.background;
        let clear_color = wgpu::Color {
            r: bg.r as f64 / 255.0,
            g: bg.g as f64 / 255.0,
            b: bg.b as f64 / 255.0,
            a: bg.a as f64 / 255.0,
        };

        let mut encoder = self.context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx render encoder"),
        });
        self.vector_pipeline.draw(&self.context.device, &mut encoder, &offscreen.view, &geometry, clear_color);
        self.sdf_pipeline.draw_all(&self.context.device, &mut encoder, &offscreen.view, scene);
        self.context.queue.submit(std::iter::once(encoder.finish()));

        *target = offscreen.read_back(&self.context.device, &self.context.queue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Canvas, Color, Element, Paint, Rect, SdfNode, SdfTree, Style};

    #[test]
    fn renders_a_filled_rect_if_a_gpu_adapter_is_available() {
        let Ok(renderer) = GpuRenderer::new() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let mut style = Style::default();
        style.fill = Some(Paint::Color(Color::rgb(10, 20, 30)));
        style.stroke = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity = Some(1.0);

        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::WHITE));
        scene.elements.push(Element::Rect(Rect {
            x: 0.0, y: 0.0, width: 20.0, height: 20.0, rx: None, ry: None,
            id: None, transform: None, style,
        }));

        let mut target = RenderTarget::new(20, 20);
        renderer.render(&scene, &mut target);

        assert_eq!(target.get_pixel(10, 10), [10, 20, 30, 255]);
    }

    #[test]
    fn renders_an_sdf_circle_if_a_gpu_adapter_is_available() {
        let Ok(renderer) = GpuRenderer::new() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let mut scene = Scene::new(Canvas::new(40.0, 40.0, Color::BLACK));
        scene.elements.push(Element::Sdf(SdfNode::new(
            SdfTree::Circle { cx: 20.0, cy: 20.0, r: 15.0 },
            Paint::Color(Color::rgb(220, 80, 40)),
        )));

        let mut target = RenderTarget::new(40, 40);
        renderer.render(&scene, &mut target);

        // Center of the circle should be the fill color; a far corner,
        // well outside the circle, should still be the background.
        assert_eq!(target.get_pixel(20, 20), [220, 80, 40, 255]);
        assert_eq!(target.get_pixel(1, 1), [0, 0, 0, 255]);
    }
        }
