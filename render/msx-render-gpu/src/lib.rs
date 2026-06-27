// render/msx-render-gpu/src/lib.rs
//! GPU renderer — wgpu-backed, offscreen-only (no window `Surface`; that's
//! `apps/msx-viewer`'s job once it wires up a GPU path).
//!
//! ## Honesty about scope and risk
//!
//! This is the highest API-risk crate in the project, by a wide margin.
//! wgpu ships a breaking release roughly every three months. Every wgpu
//! signature in this crate (`context.rs`, `pipeline.rs`, `sdf.rs`,
//! `splat.rs`, `layer.rs`, `target.rs`) has been checked directly against
//! the published wgpu 26.0.1 source (`gfx-rs/wgpu`, tag `wgpu-v26.0.1`) —
//! not docs, not training-data memory, the actual struct definitions —
//! after the first version of this crate, written against an
//! aspirational/incorrect guess at the 26.x API, failed to build with 19
//! distinct errors. Notably this API generation renamed the texture/buffer
//! copy descriptors (`ImageCopyTexture` → `TexelCopyTextureInfo`, etc., see
//! `target.rs`), uses `Device::poll(PollType) -> Result<PollStatus,
//! PollError>` rather than an older `Maintain`-based signature, dropped
//! `PipelineLayoutDescriptor::immediate_size`/`push_constant_ranges`
//! confusion in favor of just `push_constant_ranges`, and added a
//! mandatory `depth_slice` field to `RenderPassColorAttachment` and
//! mandatory `occlusion_query_set`/`timestamp_writes` fields to
//! `RenderPassDescriptor`. `lyon`'s tessellation API needed one similar
//! fix: `StrokeOptions` is `#[non_exhaustive]`, so it has to be built via
//! its `.with_line_width()` builder method, not struct-update syntax.
//!
//! `Vector`, `Sdf`, `Splat`, and `Layer` (opacity + isolated buffering;
//! Normal blend only — see `layer.rs`) are all wired up now. Rendering
//! order: all non-layer content first (vector → SDF → splat, painted into
//! one shared buffer), then every top-level `Layer` composites on top,
//! regardless of its position in document order — see `layer.rs`'s module
//! doc for why. `Text` is a deliberate no-op everywhere in this project.

mod context;
mod layer;
mod pipeline;
mod sdf;
mod splat;
mod target;
mod vector;

pub use context::GpuContext;
pub use layer::LayerCompositor;
pub use pipeline::VectorPipeline;
pub use sdf::SdfPipeline;
pub use splat::SplatPipeline;
pub use target::OffscreenTarget;
pub use vector::{tessellate_elements, tessellate_scene, Vertex, VectorGeometry};

use msx_ast::{Matrix2D, Scene};
use msx_render_core::{RenderTarget, Renderer};

pub struct GpuRenderer {
    context: GpuContext,
    vector_pipeline: VectorPipeline,
    sdf_pipeline: SdfPipeline,
    splat_pipeline: SplatPipeline,
    layer_compositor: LayerCompositor,
}

impl GpuRenderer {
    pub fn new() -> Result<Self, String> {
        let context = GpuContext::new()?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let vector_pipeline = VectorPipeline::new(&context.device, format);
        let sdf_pipeline = SdfPipeline::new(&context.device, format);
        let splat_pipeline = SplatPipeline::new(&context.device, format);
        let layer_compositor = LayerCompositor::new(&context.device, format);
        Ok(GpuRenderer { context, vector_pipeline, sdf_pipeline, splat_pipeline, layer_compositor })
    }
}

impl Renderer for GpuRenderer {
    fn render(&self, scene: &Scene, target: &mut RenderTarget) {
        let width = scene.canvas.width.round().max(1.0) as u32;
        let height = scene.canvas.height.round().max(1.0) as u32;
        let canvas_f = (width as f32, height as f32);

        let offscreen = OffscreenTarget::new(&self.context.device, width, height);

        // Pass 1: every non-layer element — vector, SDF, splat — into the
        // shared buffer.
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
        self.splat_pipeline.draw_all(&self.context.device, &mut encoder, &offscreen.view, scene);
        self.context.queue.submit(std::iter::once(encoder.finish()));

        // Pass 2: every top-level Layer, composited on top — see
        // layer.rs's module doc for the document-order caveat.
        let mut layers = Vec::new();
        layer::collect_layers(&scene.elements, Matrix2D::identity(), &mut layers);
        for (layer, transform) in &layers {
            self.layer_compositor.render_layer(
                &self.context.device,
                &self.context.queue,
                &offscreen.view,
                layer,
                *transform,
                (width, height),
                &self.vector_pipeline,
                &self.sdf_pipeline,
                &self.splat_pipeline,
            );
        }

        *target = offscreen.read_back(&self.context.device, &self.context.queue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{BlendMode, Canvas, Circle, Color, Element, Layer, Paint, Rect, SdfNode, SdfTree, Style};

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

        assert_eq!(target.get_pixel(20, 20), [220, 80, 40, 255]);
        assert_eq!(target.get_pixel(1, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn renders_a_layer_at_half_opacity_if_a_gpu_adapter_is_available() {
        let Ok(renderer) = GpuRenderer::new() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let mut style = Style::default();
        style.fill = Some(Paint::Color(Color::rgb(255, 0, 0)));
        style.stroke = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity = Some(1.0);

        let circle = Element::Circle(Circle { cx: 10.0, cy: 10.0, r: 8.0, id: None, transform: None, style });
        let mut layer = Layer::new(vec![circle]);
        layer.blend_mode = BlendMode::Normal;
        layer.opacity = 0.5;

        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));
        scene.elements.push(Element::Layer(layer));

        let mut target = RenderTarget::new(20, 20);
        renderer.render(&scene, &mut target);

        // Half-opacity red over black background → roughly half red.
        let px = target.get_pixel(10, 10);
        assert!(px[0] > 100 && px[0] < 180, "expected a half-strength red, got {:?}", px);
        assert_eq!(px[3], 255);
    }
}
