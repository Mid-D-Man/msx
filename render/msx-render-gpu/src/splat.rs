// render/msx-render-gpu/src/splat.rs
//! `GaussianSplat` rendering: one instanced billboard quad per splat,
//! gaussian falloff evaluated per-pixel in the fragment shader — the same
//! math as `msx-splat::gaussian::evaluate`, transcribed to WGSL. Far
//! simpler than `sdf.rs`'s flattened-tree machine: splats have no
//! recursive structure, so one instance buffer covers the whole scene's
//! flat/gradient-filled splats (minus anything inside a `Layer` — see
//! below) in a single draw call regardless of count.
//!
//! Quad corners come from `@builtin(vertex_index)` rather than a vertex
//! buffer — every instance shares the same four corners, only the
//! per-instance transform differs.
//!
//! A splat whose `fill` resolves to a real `Def::Shader` is NOT part of
//! that batch — see `splat_shader.rs` for why (short version: the batch
//! can't accommodate per-instance shader execution, so shader-filled
//! splats are collected separately and drawn one at a time via the same
//! mask+color+composite technique `sdf_shader.rs` uses for SDF nodes).
//!
//! `collect_splats` does NOT recurse into `Element::Layer` children, same
//! reason as `sdf.rs::collect_sdf_nodes` — `layer.rs` handles those in its
//! own isolated pass.

use wgpu::util::DeviceExt;

use msx_ast::{Color, Element, GaussianSplat, Matrix2D, Scene};

use crate::sdf::paint_color;
use crate::splat_shader::{draw_splat_shader_fill, SplatShaderContext};
use crate::vector::{resolve_shader_def, Defs};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SplatInstance {
    pub center: [f32; 2],
    pub half_extents: [f32; 2],
    pub rotation: f32,
    pub sigma: [f32; 2],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CanvasParams {
    width: f32,
    height: f32,
    _pad0: f32,
    _pad1: f32,
}

pub struct SplatPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl SplatPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx splat shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/splat.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx splat bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx splat pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[], // confirmed field name — see pipeline.rs's note
        });

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SplatInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 16, shader_location: 2 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 20, shader_location: 3 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 28, shader_location: 4 },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx splat pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[instance_layout],
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

        SplatPipeline { pipeline, bind_group_layout }
    }

    /// `pub(crate)`, not `pub` (despite `SplatPipeline` itself being
    /// re-exported publicly via `lib.rs`'s `pub use splat::SplatPipeline;`)
    /// — same reasoning as `SdfPipeline::draw_all`: `shader_ctx:
    /// Option<&SplatShaderContext>` takes a `pub(crate)` type, so external
    /// code could only ever call this with `None` anyway, and rustc's
    /// `private_interfaces` lint correctly flags that mismatch as a hard
    /// error under this project's `-D warnings` CI (this is the exact
    /// warning a real CI run surfaced). Nothing outside this crate
    /// constructs an `SplatPipeline` directly today.
    pub(crate) fn draw_all(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, scene: &Scene, shader_ctx: Option<&SplatShaderContext>) {
        let canvas = (scene.canvas.width as f32, scene.canvas.height as f32);
        let defs = Defs::build(&scene.defs);
        self.draw_all_elements(device, encoder, view, &scene.elements, Matrix2D::identity(), canvas, &defs, shader_ctx);
    }

    /// Entry point for `layer.rs`: draw just the `Splat` shapes within one
    /// layer's children, scoped to its own buffer. Always passes
    /// `shader_ctx = None` at that call site — shader-filled splats inside
    /// a `Layer` aren't wired up yet, consistent with `sdf.rs`'s identical
    /// gap for shader-filled SDF nodes in a `Layer`.
    ///
    /// Ordering note: shader-routed splats draw individually, in document
    /// order, as they're found; flat/gradient splats are gathered and
    /// drawn as ONE batched instanced call at the end, for the same
    /// performance reason the whole batching scheme exists. A scene
    /// mixing flat and shader-filled splats can therefore end up with the
    /// shader-filled ones rendering as if they were all behind (or ahead
    /// of) the flat batch, rather than precisely interleaved in document
    /// order — the same coarse, already-pre-existing granularity this
    /// crate's fixed "vector → shader fills → SDF → splat" pass order
    /// (see `lib.rs`) already has between element *types*, just now
    /// visible within splats specifically too. Not worth a more complex
    /// multi-batch scheme for effects that are typically atmospheric/
    /// particle-like, where exact inter-splat z-order rarely matters.
    ///
    /// `pub(crate)`, not `pub` — mirrors `SdfPipeline::draw_all_elements`'s
    /// own reasoning exactly: this takes both `Defs` and
    /// `Option<&SplatShaderContext>`, both `pub(crate)` types, so a `pub`
    /// signature here is a `private_interfaces` violation under this
    /// project's `-D warnings` CI (the actual warning a real CI run
    /// surfaced) regardless of `SplatPipeline` itself being re-exported
    /// publicly.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_all_elements(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, elements: &[Element], base_transform: Matrix2D, canvas: (f32, f32), defs: &Defs, shader_ctx: Option<&SplatShaderContext>) {
        let mut splats = Vec::new();
        collect_splats(elements, base_transform, &mut splats);

        let mut flat_instances = Vec::new();
        for (splat, transform) in &splats {
            // Mirrors sdf.rs's own routing decision exactly: `Option::zip`
            // instead of a manual `and_then(..map)` (clippy's
            // `manual_option_zip`) — every real call site today
            // (`draw_all`, `render_layer`) always passes `Some`, so this
            // costs nothing extra in practice; `splat.fill.as_ref()` being
            // `None` (every splat that existed before this session, and
            // any that still just use `color` today) is still the actual
            // short-circuit that matters, and `.and_then` there is
            // unchanged.
            let routed = shader_ctx.zip(splat.fill.as_ref().and_then(|paint| resolve_shader_def(paint, defs)));
            match routed {
                Some((ctx, shader_def)) => {
                    draw_splat_shader_fill(device, encoder, view, self, splat, shader_def, *transform, canvas, ctx);
                }
                None => flat_instances.push(to_instance(splat, *transform, Some(defs))),
            }
        }

        if !flat_instances.is_empty() {
            self.draw_instances(device, encoder, view, &flat_instances, canvas, wgpu::LoadOp::Load);
        }
    }

    /// Renders one splat's own Gaussian falloff as a standalone mask into
    /// `mask_view`, for compositing against a shader's output afterward
    /// (see `splat_shader::draw_splat_shader_fill`, the only caller).
    /// Reuses `splat.wgsl` completely unmodified — forcing `color =
    /// (1,1,1,1)` means its existing `gaussian * color.a` output alpha IS
    /// exactly the falloff shape, same trick `sdf.rs::draw_mask` uses for
    /// SDF nodes. `mask_view` is cleared to transparent first — a fresh
    /// per-splat scratch texture, never the shared scene buffer
    /// `draw_instances` draws onto with `LoadOp::Load`.
    pub(crate) fn draw_mask(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, mask_view: &wgpu::TextureView, splat: &GaussianSplat, transform: Matrix2D, canvas: (f32, f32)) {
        let mut instance = to_instance(splat, transform, None);
        instance.color = [1.0, 1.0, 1.0, 1.0];
        self.draw_instances(device, encoder, mask_view, &[instance], canvas, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
    }

    /// Shared tail for the batched draw and `draw_mask`: uploads the
    /// instance buffer + canvas params, builds the bind group, and runs
    /// the render pass. The only thing that ever differs between callers
    /// is `load_op` (batched drawing composites onto the existing scene
    /// buffer, `draw_mask` starts a fresh scratch texture) and the
    /// instance count — so those are the two things pulled out as
    /// parameters rather than duplicating this whole function body.
    /// `pub(crate)`, not private — `splat_shader.rs`'s fallback path (a
    /// shader whose `source_ref` failed to resolve) reuses this directly
    /// for a single precomputed instance, same reasoning as `draw_mask`'s
    /// own visibility.
    pub(crate) fn draw_instances(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, instances: &[SplatInstance], canvas: (f32, f32), load_op: wgpu::LoadOp<wgpu::Color>) {
        let canvas_params = CanvasParams { width: canvas.0, height: canvas.1, _pad0: 0.0, _pad1: 0.0 };

        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx splat instance buffer"),
            contents: bytemuck::cast_slice(instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let canvas_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx splat canvas params buffer"),
            contents: bytemuck::bytes_of(&canvas_params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx splat bind group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: canvas_buffer.as_entire_binding() }],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx splat pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: load_op, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, instance_buffer.slice(..));
        pass.draw(0..6, 0..instances.len() as u32);
    }
}

/// Gathers every splat under `elements` into `(splat, transform)` pairs —
/// deliberately untouched by `defs`/shader-routing at collection time
/// (mirrors `sdf.rs::collect_sdf_nodes` exactly): a `GaussianSplat`
/// reference is tied to `elements`' own lifetime, but a resolved
/// `&ShaderDef` would be tied to `Defs`'s own, separate lifetime (the
/// `&[Def]` slice `Defs::build` was constructed from) — mixing the two in
/// one stored tuple would require unifying two lifetimes that have no
/// reason to actually be the same. Routing happens per-item instead, in
/// `draw_all_elements`'s loop, resolved and used within the same
/// iteration rather than carried in a longer-lived collection.
///
/// Does NOT recurse into `Element::Layer` — see module docs.
fn collect_splats<'a>(elements: &'a [Element], transform: Matrix2D, out: &mut Vec<(&'a GaussianSplat, Matrix2D)>) {
    for el in elements {
        match el {
            Element::Splat(s) => out.push((s, transform)),
            Element::Group(g) => {
                let local = g.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                collect_splats(&g.children, transform.concat(local), out);
            }
            _ => {}
        }
    }
}

/// `GaussianSplat` has no `transform` field of its own — only the ambient
/// group/layer transform applies. Anisotropic scale or shear in that
/// ambient transform isn't accounted for here: center moves correctly, but
/// sigma/rotation stay in the splat's own local frame. A reasonable gap —
/// getting this fully right means transforming the gaussian's covariance,
/// not just its center.
///
/// `defs`, when given, resolves `s.fill` (flat color / gradient average —
/// never a shader ref, `collect_splats` already routed those to `shader`
/// before this is ever called for them) the same way every other
/// element's `Paint::Ref` is resolved in this crate. `None` is only ever
/// passed by `draw_mask`, which immediately overwrites `.color` anyway —
/// resolving `fill` there would be wasted work.
pub(crate) fn to_instance(s: &GaussianSplat, transform: Matrix2D, defs: Option<&Defs>) -> SplatInstance {
    let (cx, cy) = apply_matrix(transform, (s.x as f32, s.y as f32));
    let radius_x = effective_radius_axis(s.sigma_x, 0.02);
    let radius_y = effective_radius_axis(s.sigma_y, 0.02);

    let resolved: Color = match (defs, &s.fill) {
        (Some(defs), Some(paint)) => paint_color(paint, defs),
        _ => s.color,
    };

    SplatInstance {
        center: [cx, cy],
        half_extents: [radius_x, radius_y],
        rotation: s.rotation as f32,
        sigma: [s.sigma_x as f32, s.sigma_y as f32],
        color: [
            resolved.r as f32 / 255.0,
            resolved.g as f32 / 255.0,
            resolved.b as f32 / 255.0,
            (resolved.a as f64 / 255.0 * s.opacity) as f32,
        ],
    }
}

/// Per-axis version of `msx_splat::gaussian::effective_radius`, which uses
/// `max(sigma_x, sigma_y)` for a single combined cull radius — here each
/// axis gets its own extent so a strongly anisotropic splat gets a snugly
/// rotated quad instead of an oversized square.
pub(crate) fn effective_radius_axis(sigma: f64, threshold: f64) -> f32 {
    (sigma * (-2.0 * threshold.ln()).sqrt()) as f32
}

pub(crate) fn apply_matrix(m: Matrix2D, p: (f32, f32)) -> (f32, f32) {
    (m.a as f32 * p.0 + m.c as f32 * p.1 + m.e as f32, m.b as f32 * p.0 + m.d as f32 * p.1 + m.f as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Color, Def, Element as MsxElement, Group, Layer, LinearGradient, Paint, Stop};

    #[test]
    fn to_instance_bakes_opacity_into_alpha() {
        let splat = GaussianSplat::circle(10.0, 20.0, 5.0, Color::rgba(255, 0, 0, 200), 0.5);
        let instance = to_instance(&splat, Matrix2D::identity(), None);
        assert_eq!(instance.center, [10.0, 20.0]);
        let expected_alpha = (200.0 / 255.0) * 0.5;
        assert!((instance.color[3] - expected_alpha as f32).abs() < 1e-4);
    }

    #[test]
    fn effective_radius_grows_with_sigma() {
        let small = effective_radius_axis(5.0, 0.02);
        let large = effective_radius_axis(20.0, 0.02);
        assert!(large > small * 3.0);
    }

    #[test]
    fn collect_splats_recurses_through_groups() {
        let inner = GaussianSplat::circle(1.0, 1.0, 2.0, Color::WHITE, 1.0);
        let group = Group::new(vec![MsxElement::Splat(inner)]);
        let elements = vec![MsxElement::Group(group)];

        let mut out = Vec::new();
        collect_splats(&elements, Matrix2D::identity(), &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn collect_splats_skips_layer_children() {
        let inner = GaussianSplat::circle(1.0, 1.0, 2.0, Color::WHITE, 1.0);
        let layer = Layer::new(vec![MsxElement::Splat(inner)]);
        let elements = vec![MsxElement::Layer(layer)];

        let mut out = Vec::new();
        collect_splats(&elements, Matrix2D::identity(), &mut out);
        assert!(out.is_empty(), "splats inside a Layer must not be collected by the main pass");
    }

    /// A splat's `fill`, when it's a gradient ref (not a shader), must
    /// resolve through `paint_color`'s gradient-average path when built
    /// into a `SplatInstance` for the flat batch — not silently keep
    /// using the plain `color` field just because `fill` is also present.
    /// This exercises `to_instance` directly (the function
    /// `draw_all_elements` actually calls for anything that *isn't*
    /// routed to the shader path), since `resolve_shader_def` itself now
    /// lives in `draw_all_elements`'s own loop, not in `collect_splats`.
    #[test]
    fn to_instance_resolves_a_gradient_fill_instead_of_the_plain_color_field() {
        let gradient = LinearGradient::new("g".to_string(), 0.0, 0.0, 10.0, 0.0, vec![
            Stop::new(0.0, Color::rgb(0, 0, 0)),
            Stop::new(1.0, Color::rgb(255, 255, 255)),
        ]);
        let defs_vec = vec![Def::LinearGradient(gradient)];
        let defs = Defs::build(&defs_vec);

        let mut splat = GaussianSplat::circle(5.0, 5.0, 2.0, Color::rgb(255, 0, 0), 1.0);
        splat.fill = Some(Paint::Ref("url(#g)".to_string()));

        let instance = to_instance(&splat, Matrix2D::identity(), Some(&defs));
        // Average of opaque black (0,0,0) and opaque white (255,255,255)
        // is mid-gray — (127,127,127) on EVERY channel, not just red.
        // `splat.color` (unused here) is red (255,0,0), so what actually
        // distinguishes "gradient average was used" from "color leaked
        // through instead" is r == g == b, not any single channel's
        // absolute value. An earlier version of this test asserted the
        // green channel specifically should be ~0, which was simply
        // wrong arithmetic for a black/white gradient (gray's green
        // channel is 127, not 0) — caught by the real CI run this test
        // was written for, not by re-reading the test itself, which is
        // exactly why it's worth noting here rather than quietly fixing.
        for (channel, value) in ["red", "green", "blue"].iter().zip(instance.color) {
            assert!((value - 127.0 / 255.0).abs() < 1e-3, "{channel} channel should be the gradient average (~127/255), got {value}");
        }
    }

    /// `resolve_shader_def` (used by `draw_all_elements`'s own routing
    /// loop, not `collect_splats`) must correctly identify a `fill`
    /// pointing at a real `Def::Shader` — confirms the exact lookup
    /// `draw_all_elements` relies on to decide "this splat needs the
    /// individual mask+color+composite path, not the batch".
    #[test]
    fn resolve_shader_def_finds_a_shader_ref_on_a_splats_fill() {
        let shader_def = msx_ast::ShaderDef::new("s".to_string(), "s.wgsl".to_string(), Color::rgb(1, 2, 3));
        let defs_vec = vec![Def::Shader(shader_def)];
        let defs = Defs::build(&defs_vec);

        let mut splat = GaussianSplat::circle(5.0, 5.0, 2.0, Color::rgb(255, 0, 0), 1.0);
        splat.fill = Some(Paint::Ref("url(#s)".to_string()));

        let found = splat.fill.as_ref().and_then(|paint| resolve_shader_def(paint, &defs));
        assert!(found.is_some(), "a fill referencing a real Def::Shader must resolve via resolve_shader_def");
    }
}
