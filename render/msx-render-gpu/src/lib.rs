// render/msx-render-gpu/src/lib.rs
//! GPU renderer — wgpu-backed, offscreen-only (no window `Surface`; that's
//! `apps/msx-viewer`'s job once it wires up a GPU path).
//!
//! ## Honesty about scope and risk
//!
//! This is the highest API-risk crate in the project, by a wide margin.
//! wgpu ships a breaking release roughly every three months. Every wgpu
//! signature in this crate (`context.rs`, `pipeline.rs`, `sdf.rs`,
//! `splat.rs`, `layer.rs`, `target.rs`, `shader.rs`) has been checked
//! directly against the published wgpu 26.0.1 source (`gfx-rs/wgpu`, tag
//! `wgpu-v26.0.1`) — not docs, not training-data memory, the actual
//! struct definitions — after the first version of this crate, written
//! against an aspirational/incorrect guess at the 26.x API, failed to
//! build with 19 distinct errors. Notably this API generation renamed the
//! texture/buffer copy descriptors (`ImageCopyTexture` → `TexelCopyTextureInfo`,
//! etc., see `target.rs`), uses `Device::poll(PollType) -> Result<PollStatus,
//! PollError>` rather than an older `Maintain`-based signature, dropped
//! `PipelineLayoutDescriptor::immediate_size`/`push_constant_ranges`
//! confusion in favor of just `push_constant_ranges`, and added a
//! mandatory `depth_slice` field to `RenderPassColorAttachment` and
//! mandatory `occlusion_query_set`/`timestamp_writes` fields to
//! `RenderPassDescriptor`. `lyon`'s tessellation API needed one similar
//! fix: `StrokeOptions` is `#[non_exhaustive]`, so it has to be built via
//! its `.with_line_width()` builder method, not struct-update syntax.
//! `begin_render_pass<'encoder>(&'encoder mut self, desc:
//! &RenderPassDescriptor<'_>) -> RenderPass<'encoder>` is worth flagging
//! specifically: the descriptor's own lifetime is independent of
//! `'encoder` (`RenderPass` only guards reuse of the parent encoder, it
//! never borrows from the descriptor's contents) — get this wrong the
//! other way (tying them together) and every one of this crate's
//! ordinary inline `color_attachments: &[Some(RenderPassColorAttachment
//! { .. })]` call sites spuriously fails to borrow-check, even though
//! that exact pattern is completely standard, correct wgpu usage.
//!
//! `Vector`, `Sdf`, `Splat`, and `Layer` (opacity + isolated buffering;
//! Normal blend only — see `layer.rs`) are all wired up now. Rendering
//! order: all non-layer content first (vector → shader fills → SDF →
//! splat, painted into one shared buffer), then every top-level `Layer`
//! composites on top, regardless of its position in document order — see
//! `layer.rs`'s module doc for why. `Text` is a deliberate no-op
//! everywhere in this project.
//!
//! `shader.rs` executes real WGSL for `Def::Shader` fills on top-level
//! (non-`Layer`) shapes — see its module doc for the full contract and
//! the gaps still scoped out (stroke fills, shapes inside a `Layer`,
//! opacity, and graceful handling of syntactically-invalid-but-readable
//! WGSL).

mod context;
mod layer;
mod pipeline;
mod sdf;
mod shader;
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

use std::path::Path;

use msx_ast::{Matrix2D, Scene};
use msx_render_core::{RenderTarget, Renderer};
use shader::ShaderFillPipeline;

pub struct GpuRenderer {
    context: GpuContext,
    vector_pipeline: VectorPipeline,
    sdf_pipeline: SdfPipeline,
    splat_pipeline: SplatPipeline,
    shader_pipeline: ShaderFillPipeline,
    layer_compositor: LayerCompositor,
}

impl GpuRenderer {
    pub fn new() -> Result<Self, String> {
        let context = GpuContext::new()?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let vector_pipeline = VectorPipeline::new(&context.device, format);
        let sdf_pipeline = SdfPipeline::new(&context.device, format);
        let splat_pipeline = SplatPipeline::new(&context.device, format);
        let shader_pipeline = ShaderFillPipeline::new(&context.device, format);
        let layer_compositor = LayerCompositor::new(&context.device, format);
        Ok(GpuRenderer { context, vector_pipeline, sdf_pipeline, splat_pipeline, shader_pipeline, layer_compositor })
    }

    /// Renders exactly like the `Renderer` trait's `render`, but with
    /// real WGSL execution for any top-level shape whose fill references
    /// a `Def::Shader` — `shader_base_dir` is the directory `source_ref`
    /// paths are resolved relative to (the original `.msx` source file's
    /// own directory, matching `msx-cli`'s already-existing
    /// `validate_shader_refs` convention), and `time` drives every
    /// shader-def's free-running `time: f32` uniform (see `shader.rs`'s
    /// module doc for the full layout contract). Kept separate from the
    /// `Renderer` trait itself since that shared trait (used identically
    /// by the CPU/SVG backends) has no way to pass either of these
    /// through — the trait impl below calls this with the current
    /// working directory and `time = 0.0` as a reasonable default for
    /// callers that don't have anything more specific.
    pub fn render_with_shader_dir(&self, scene: &Scene, target: &mut RenderTarget, shader_base_dir: &Path, time: f32) {
        let width  = scene.canvas.width.round().max(1.0) as u32;
        let height = scene.canvas.height.round().max(1.0) as u32;

        // RENDER_ATTACHMENT so every pipeline in this crate can draw into
        // it, COPY_SRC so read_back can copy it out to a buffer for CPU
        // readback — nothing ever samples the top-level target itself
        // (unlike layer.rs's per-layer buffer below), so no
        // TEXTURE_BINDING here.
        let offscreen = OffscreenTarget::new(&self.context.device, width, height, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);

        // Pass 1: every non-layer element — vector, shader fills, SDF,
        // splat — into the shared buffer/view.
        let (geometry, shader_shapes) = vector::tessellate_scene_with_shaders(scene);
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
        for shape in &shader_shapes {
            // A shape whose source_ref doesn't resolve (missing file,
            // moved since compile time, etc.) falls back to its
            // fallback_color rather than silently vanishing or aborting
            // the whole render — same "always paint something sane"
            // principle ShaderDef's own doc comment describes for every
            // other renderer.
            if let Err(e) = self.shader_pipeline.draw(&self.context.device, &mut encoder, &offscreen.view, shader_base_dir, shape, time) {
                eprintln!("msx-render-gpu: {e} — falling back to fallback_color");
                self.vector_pipeline.draw_fallback_fill(&self.context.device, &mut encoder, &offscreen.view, &shape.vertices, &shape.indices, shape.shader.fallback_color);
            }
        }
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

impl Renderer for GpuRenderer {
    fn render(&self, scene: &Scene, target: &mut RenderTarget) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        self.render_with_shader_dir(scene, target, &cwd, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{BlendMode, Canvas, Circle, Color, Element, GaussianSplat, Layer, Paint, Rect, SdfNode, SdfTree, Style};

    #[test]
    fn renders_a_filled_rect_if_a_gpu_adapter_is_available() {
        let Ok(renderer) = GpuRenderer::new() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let style = Style {
            fill: Some(Paint::Color(Color::rgb(10, 20, 30))),
            stroke: Some(Paint::None),
            stroke_width: Some(0.0),
            opacity: Some(1.0),
            ..Default::default()
        };

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

    /// `renders_a_filled_rect`/`renders_an_sdf_circle`/
    /// `renders_a_layer_at_half_opacity` cover vector, SDF, and layer
    /// compositing against a real adapter — splat had no equivalent test
    /// at all, meaning `splat.rs`'s GPU path (its own pipeline, its own
    /// `CanvasParams` uniform, its own instanced-vertex-buffer setup) had
    /// never actually run against real validation. The other three found
    /// two real bugs (`TEXTURE_BINDING`, `CompositeParams`'s size) that
    /// no amount of type-checking caught — worth closing this specific
    /// gap rather than assuming splat is fine just because it wasn't
    /// hand-audited to have the exact same vec3-alignment mistake.
    #[test]
    fn renders_a_splat_if_a_gpu_adapter_is_available() {
        let Ok(renderer) = GpuRenderer::new() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let mut scene = Scene::new(Canvas::new(40.0, 40.0, Color::BLACK));
        scene.elements.push(Element::Splat(GaussianSplat::new(
            20.0, 20.0, 10.0, 10.0, Color::rgb(30, 200, 120), 1.0,
        )));

        let mut target = RenderTarget::new(40, 40);
        renderer.render(&scene, &mut target);

        // Dead center of an opacity-1.0 splat should read very close to
        // the splat's own peak color, not the black background — a loose
        // tolerance here since this is checking "the pipeline actually
        // drew something recognizable", not re-deriving the gaussian
        // falloff math (already covered by splat.rs's own unit tests).
        let px = target.get_pixel(20, 20);
        assert!(px[0] < 60 && px[1] > 160 && px[2] > 90, "expected something close to (30,200,120) at the splat center, got {:?}", px);
        // Far corner, well outside the splat's effective radius, should
        // still be the untouched black background.
        assert_eq!(target.get_pixel(1, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn renders_a_layer_at_half_opacity_if_a_gpu_adapter_is_available() {
        let Ok(renderer) = GpuRenderer::new() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let style = Style {
            fill: Some(Paint::Color(Color::rgb(255, 0, 0))),
            stroke: Some(Paint::None),
            stroke_width: Some(0.0),
            opacity: Some(1.0),
            ..Default::default()
        };

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
