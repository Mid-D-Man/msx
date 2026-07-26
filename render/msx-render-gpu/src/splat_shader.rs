// render/msx-render-gpu/src/splat_shader.rs
//! Composites a `Def::Shader` fill onto a `GaussianSplat`'s true Gaussian
//! falloff, rather than the rectangular bounding-box quad
//! `ShaderFillPipeline::draw` would otherwise paint wall-to-wall — the
//! exact same problem `sdf_shader.rs` solves for SDF nodes, and the exact
//! same three-pass fix:
//!
//! 1. `SplatPipeline::draw_mask` renders the splat's own Gaussian falloff
//!    into a fresh scratch texture, reusing `splat.wgsl` completely
//!    unmodified (forcing `color = (1,1,1,1)` makes its existing `gaussian
//!    * color.a` output alpha exactly the falloff shape).
//! 2. `ShaderFillPipeline::draw` renders the shader's real output over the
//!    splat's own rotated bounding-box quad (`splat_bounding_quad` below —
//!    NOT axis-aligned, since a splat can be rotated; see that function's
//!    doc comment for why it can't reuse an axis-aligned-bounds helper).
//! 3. `MaskedShaderComposite` (shared with `sdf_shader.rs`) multiplies the
//!    two together into the real destination.
//!
//! Why splats can't just join `sdf_shader.rs`'s existing machinery
//! directly: everything there is keyed on `SdfNode` specifically (its own
//! bounding-box math, its own mask-forcing params). The *technique* is
//! identical — masked shader compositing — which is exactly why the one
//! genuinely SDF-specific piece (`MaskedShaderComposite` itself) was
//! pulled into its own module the moment a second, unrelated caller
//! needed it, rather than this file depending on `sdf_shader.rs` directly
//! or duplicating that pipeline a second time.
//!
//! ## Known gaps, same spirit as sdf_shader.rs's
//!
//! - **Splats have no stroke concept at all**, so there's no
//!   `draw_stroke_only`-equivalent needed here — unlike SDF nodes, a
//!   shader-filled splat has nothing left over to draw separately.
//! - **Splats inside a `Layer` are covered too, now.** `layer.rs` passes
//!   a real `SplatShaderContext` rather than `None`, same as it now does
//!   for SDF nodes and ordinary vector shapes — see `layer.rs`'s module
//!   doc.
//! - **Opacity isn't applied** — the mask-forcing step in `draw_mask`
//!   overwrites `color` (including its alpha) entirely, so a shader-filled
//!   splat's `opacity` field is currently inert. Same class of gap as
//!   every other shader-fill path in this crate.
//! - **Malformed-but-readable WGSL isn't caught gracefully** — identical
//!   reason to `sdf_shader.rs` and `shader.rs`'s own documented gap.

use msx_ast::{GaussianSplat, Matrix2D, ShaderDef};

use crate::masked_shader_composite::{clear_transparent, MaskedShaderComposite};
use crate::shader::{PendingShaderShape, ShaderFillPipeline};
use crate::splat::{apply_matrix, effective_radius_axis, to_instance, SplatPipeline};
use crate::target::OffscreenTarget;

/// Bundles everything `SplatPipeline::draw_all_elements` needs to route a
/// shader-def-filled splat through real WGSL execution — same role
/// `sdf_shader::SdfShaderContext` plays for SDF nodes, kept as a separate
/// type rather than a shared one since the two pipelines it bundles
/// (`ShaderFillPipeline`, `MaskedShaderComposite`) are the only things in
/// common; `SdfPipeline`/`SplatPipeline` themselves are passed as
/// ordinary function arguments instead, not part of either context.
pub(crate) struct SplatShaderContext<'a> {
    pub shader_pipeline: &'a ShaderFillPipeline,
    pub composite: &'a MaskedShaderComposite,
    pub shader_base_dir: &'a std::path::Path,
    pub time: f32,
}

/// Computes the splat's actual on-screen rotated quad — matching
/// `splat.wgsl`'s `vs_main` exactly, corner for corner, rather than an
/// axis-aligned bounding box (`sdf.rs::node_bounding_quad`'s approach).
/// Unlike an SDF node's bounding box, a splat's quad can be rotated; using
/// its axis-aligned bounds instead would be a *looser* fit than the
/// billboard itself, and — same problem this whole file exists to solve —
/// the shader would paint into corners the mask never covers, just a
/// smaller version of the original bug.
fn splat_bounding_quad(splat: &GaussianSplat, transform: Matrix2D, canvas: (f32, f32)) -> (Vec<[f32; 2]>, Vec<u32>) {
    let (cx, cy) = apply_matrix(transform, (splat.x as f32, splat.y as f32));
    let half_extents = (effective_radius_axis(splat.sigma_x, 0.02), effective_radius_axis(splat.sigma_y, 0.02));
    let rotation = splat.rotation as f32;
    let (cos_r, sin_r) = (rotation.cos(), rotation.sin());

    let corners = [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let vertices = corners
        .iter()
        .map(|&(lx, ly)| {
            let local = (lx * half_extents.0, ly * half_extents.1);
            let rotated = (local.0 * cos_r - local.1 * sin_r, local.0 * sin_r + local.1 * cos_r);
            let world = (cx + rotated.0, cy + rotated.1);
            [(world.0 / canvas.0) * 2.0 - 1.0, 1.0 - (world.1 / canvas.1) * 2.0]
        })
        .collect();

    (vertices, vec![0, 1, 2, 0, 2, 3])
}

/// Draws one splat whose fill resolved to a real `Def::Shader` — see this
/// module's doc comment for the three-pass technique. Falls back to a
/// flat `fallback_color` fill (reusing the existing batched
/// `SplatPipeline` machinery for a single instance, rather than a
/// bespoke fallback path) if the shader's `source_ref` doesn't resolve —
/// same "always paint something sane" contract every other shader-fill
/// path in this crate already has.
pub(crate) fn draw_splat_shader_fill(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    splat_pipeline: &SplatPipeline,
    splat: &GaussianSplat,
    shader_def: &ShaderDef,
    transform: Matrix2D,
    canvas: (f32, f32),
    ctx: &SplatShaderContext,
) {
    let canvas_u = (canvas.0.round().max(1.0) as u32, canvas.1.round().max(1.0) as u32);
    let scratch_usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;

    // Pass 1: the splat's own real Gaussian mask.
    let mask_target = OffscreenTarget::new(device, canvas_u.0, canvas_u.1, scratch_usage);
    splat_pipeline.draw_mask(device, encoder, &mask_target.view, splat, transform, canvas);

    let (vertices, indices) = splat_bounding_quad(splat, transform, canvas);
    let shape = PendingShaderShape { shader: shader_def.clone(), vertices, indices };

    // Pass 2: the shader's real output, over the splat's own rotated quad.
    let color_target = OffscreenTarget::new(device, canvas_u.0, canvas_u.1, scratch_usage);
    clear_transparent(encoder, &color_target.view);

    match ctx.shader_pipeline.draw(device, encoder, &color_target.view, ctx.shader_base_dir, &shape, ctx.time) {
        // Pass 3: multiply color × mask into the real destination.
        Ok(()) => ctx.composite.composite(device, encoder, view, &color_target.view, &mask_target.view),
        Err(e) => {
            eprintln!("msx-render-gpu: {e} — falling back to fallback_color for splat");
            let mut fallback_instance = to_instance(splat, transform, None);
            fallback_instance.color = [
                shader_def.fallback_color.r as f32 / 255.0,
                shader_def.fallback_color.g as f32 / 255.0,
                shader_def.fallback_color.b as f32 / 255.0,
                shader_def.fallback_color.a as f32 / 255.0 * splat.opacity as f32,
            ];
            // A single-instance batched draw, straight onto the real
            // destination — reuses `SplatPipeline`'s own shared draw path
            // rather than a bespoke fallback implementation.
            splat_pipeline.draw_instances(device, encoder, view, &[fallback_instance], canvas, wgpu::LoadOp::Load);
        }
    }
                                          }
