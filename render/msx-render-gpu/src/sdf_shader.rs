// render/msx-render-gpu/src/sdf_shader.rs
//! Composites a `Def::Shader` fill onto an SDF node's true (antialiased)
//! silhouette, rather than the rectangular bounding-box quad
//! `ShaderFillPipeline::draw` would otherwise paint wall-to-wall.
//!
//! ## Why this needs its own file, not just a branch inside sdf.rs
//!
//! SDF shapes aren't triangulated geometry like vector.rs's — the "is
//! this pixel inside the shape" test is itself computed per-pixel, inside
//! sdf.wgsl, from the flattened distance-field ops. vector.rs's shader
//! routing works because the shape's own tessellated triangles already
//! constrain which pixels the rasterizer even considers; an SDF shape has
//! no such geometry to hand `ShaderFillPipeline::draw` — only a
//! rectangular bounding-box quad, which for anything that isn't itself a
//! plain axis-aligned rectangle (a circle, a rounded box, a smooth union
//! of several shapes...) would paint the shader's output into the
//! bounding box's corners too, well outside the shape's real silhouette.
//!
//! The fix is a three-pass composite, all within the caller's existing
//! encoder:
//! 1. `SdfPipeline::draw_mask` renders the node's own antialiased fill
//!    coverage into a fresh scratch texture — reusing sdf.wgsl completely
//!    unmodified (see that method's doc comment for exactly how).
//! 2. `ShaderFillPipeline::draw` (also completely unmodified — this crate
//!    already designed `PendingShaderShape` as position-only/shape-
//!    agnostic, not tied to vector.rs specifically) renders the shader's
//!    real output over the node's bounding-box quad into a second scratch
//!    texture.
//! 3. `MaskedShaderComposite` (shared with `splat_shader.rs` — see that
//!    module) multiplies the two together — shader color, clipped to true
//!    SDF coverage instead of the quad's rectangular extent — into the
//!    real destination view.
//!
//! Both scratch textures are full canvas size, not cropped to the node's
//! actual bounding box — simpler (identical clip-space math to every
//! other draw call in this crate, no separate sub-rectangle offset
//! bookkeeping) at a bandwidth cost this project's typical canvas sizes
//! make irrelevant.
//!
//! ## Known gaps, flagged rather than hidden — same spirit as shader.rs's
//!
//! - **Fill only**, exactly like vector.rs's shader routing. A node's
//!   stroke, if it has one, is drawn separately by
//!   `SdfPipeline::draw_stroke_only` with the fill forced transparent —
//!   see `draw_all_elements`'s routing logic in `sdf.rs`.
//! - **Shapes inside a `Layer` are covered too, now.** `layer.rs` passes
//!   a real `SdfShaderContext` (the same shader pipeline/composite as the
//!   top level) rather than `None`, so an SDF node's shader fill executes
//!   for real inside a `Layer` as well — see `layer.rs`'s module doc.
//! - **Opacity isn't applied**, same as vector.rs's shader routing (see
//!   shader.rs's own "known gaps" for why — a uniform-layout change or a
//!   wrapping composite pass, deferred rather than bolted on here too).
//! - **Malformed-but-readable WGSL isn't caught gracefully** — same
//!   `device.push_error_scope`/`pop_error_scope` gap `shader.rs`
//!   documents; `ShaderFillPipeline::draw`'s `Result` only catches an
//!   unreadable `source_ref`, and that's the only failure this path
//!   catches too, for the same reason.
//! - **Two extra full-canvas textures per shader-filled SDF node, every
//!   frame.** Not cached across frames (unlike `ShaderFillPipeline`'s own
//!   per-def pipeline cache) — fine at this project's scale, worth
//!   revisiting if a scene ever has many shader-filled SDF nodes at once.

use msx_ast::{Matrix2D, SdfNode, ShaderDef};

use crate::masked_shader_composite::{clear_transparent, MaskedShaderComposite};
use crate::sdf::{node_bounding_quad, SdfPipeline};
use crate::shader::{PendingShaderShape, ShaderFillPipeline};
use crate::target::OffscreenTarget;

/// Bundles everything `SdfPipeline::draw_all_elements` needs to route a
/// shader-def-filled node through real WGSL execution instead of the flat
/// fallback every renderer used unconditionally before this feature
/// existed. Passing `None` at a call site is what keeps that call site
/// on the old flat-fallback behavior — there's no separate flag to
/// remember, the presence of this context *is* the on/off switch, same
/// pattern `shader_shapes: Option<&mut Vec<..>>` already established for
/// vector.rs's own shader routing. Both real call sites (`lib.rs`'s
/// top-level pass and `layer.rs`'s per-layer pass) pass
/// `Some(&SdfShaderContext {..})` today; `None` only shows up in this
/// crate's own unit tests now.
pub(crate) struct SdfShaderContext<'a> {
    pub shader_pipeline: &'a ShaderFillPipeline,
    pub composite: &'a MaskedShaderComposite,
    pub shader_base_dir: &'a std::path::Path,
    pub time: f32,
}

/// Draws one SDF node whose fill resolved to a real `Def::Shader` — see
/// this module's doc comment for the three-pass technique. Falls back to
/// a flat `fallback_color` fill (via `SdfPipeline::draw_fallback_fill`) if
/// the shader's `source_ref` doesn't resolve, same "always paint
/// something sane" contract every other shader-fill path in this crate
/// already has.
pub(crate) fn draw_sdf_shader_fill(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    sdf_pipeline: &SdfPipeline,
    node: &SdfNode,
    shader_def: &ShaderDef,
    transform: Matrix2D,
    canvas: (f32, f32),
    ctx: &SdfShaderContext,
) {
    let canvas_u = (canvas.0.round().max(1.0) as u32, canvas.1.round().max(1.0) as u32);
    let scratch_usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;

    // Pass 1: the node's own real mask.
    let mask_target = OffscreenTarget::new(device, canvas_u.0, canvas_u.1, scratch_usage);
    if !sdf_pipeline.draw_mask(device, encoder, &mask_target.view, node, transform, canvas) {
        return; // empty tree / non-invertible transform — draw_one would bail the same way
    }

    let Some((vertices, indices)) = node_bounding_quad(node, transform, canvas) else { return };
    let shape = PendingShaderShape { shader: shader_def.clone(), vertices, indices };

    // Pass 2: the shader's real output, over the bounding-box quad.
    let color_target = OffscreenTarget::new(device, canvas_u.0, canvas_u.1, scratch_usage);
    clear_transparent(encoder, &color_target.view);

    match ctx.shader_pipeline.draw(device, encoder, &color_target.view, ctx.shader_base_dir, &shape, ctx.time) {
        // Pass 3: multiply color × mask into the real destination.
        Ok(()) => ctx.composite.composite(device, encoder, view, &color_target.view, &mask_target.view),
        Err(e) => {
            eprintln!("msx-render-gpu: {e} — falling back to fallback_color for SDF node");
            sdf_pipeline.draw_fallback_fill(device, encoder, view, node, transform, canvas, shader_def.fallback_color);
        }
    }
}
