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
//! 3. `SdfShaderComposite` multiplies the two together — shader color,
//!    clipped to true SDF coverage instead of the quad's rectangular
//!    extent — into the real destination view.
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
//! - **Top-level elements only.** `layer.rs` doesn't pass an
//!   `SdfShaderContext` at all yet, so shader fills on an SDF node inside
//!   a `Layer` keep painting flat — consistent with vector.rs's identical
//!   gap for ordinary shapes inside a `Layer`, not a new limitation.
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

use crate::sdf::{node_bounding_quad, SdfPipeline};
use crate::shader::{PendingShaderShape, ShaderFillPipeline};
use crate::target::OffscreenTarget;

/// Bundles everything `SdfPipeline::draw_all_elements` needs to route a
/// shader-def-filled node through real WGSL execution instead of the flat
/// fallback every renderer used unconditionally before this feature
/// existed. Passing `None` at a call site (`layer.rs` today) is what
/// keeps that call site on the old flat-fallback behavior — there's no
/// separate flag to remember, the presence of this context *is* the
/// on/off switch, same pattern `shader_shapes: Option<&mut Vec<..>>`
/// already established for vector.rs's own shader routing.
pub(crate) struct SdfShaderContext<'a> {
    pub shader_pipeline: &'a ShaderFillPipeline,
    pub composite: &'a SdfShaderComposite,
    pub shader_base_dir: &'a std::path::Path,
    pub time: f32,
}

pub(crate) struct SdfShaderComposite {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl SdfShaderComposite {
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx sdf-shader composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sdf_shader_composite.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx sdf-shader composite bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx sdf-shader composite pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx sdf-shader composite pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("msx sdf-shader composite sampler"),
            ..Default::default()
        });

        SdfShaderComposite { pipeline, bind_group_layout, sampler }
    }

    fn composite(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, dst_view: &wgpu::TextureView, color_view: &wgpu::TextureView, mask_view: &wgpu::TextureView) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx sdf-shader composite bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(color_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(mask_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx sdf-shader composite pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

/// Clears `view` to transparent with an otherwise-empty render pass.
/// Needed before `ShaderFillPipeline::draw`, which always composites with
/// `LoadOp::Load` — correct for its normal job (drawing directly onto the
/// shared scene buffer) but wrong for the fresh per-node scratch texture
/// used here, which needs to start from nothing. Not worth adding a
/// separate clear-then-draw mode to shader.rs itself for this one caller.
fn clear_transparent(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("msx sdf-shader scratch clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
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
