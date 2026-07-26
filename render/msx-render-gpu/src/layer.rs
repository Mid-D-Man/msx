// render/msx-render-gpu/src/layer.rs
//! `Layer` rendering: children render into an isolated offscreen texture
//! (cleared to transparent, not the canvas background), then that buffer
//! composites onto the parent target at the layer's opacity.
//!
//! ## Known gaps, both flagged rather than hidden
//!
//! **Blend modes other than Normal don't work yet.** Proper
//! `Multiply`/`Screen`/etc. compositing needs the fragment shader to read
//! BOTH the existing backdrop and the new source per pixel — but a render
//! pass can't sample the texture it's currently writing to (no
//! framebuffer-fetch in WebGPU). The correct technique is a backdrop
//! snapshot (copy the current target to a separate readable texture) plus
//! a dual-texture-sampling pass implementing the W3C blend formulas in
//! WGSL — a genuinely separate, focused piece of work. Every blend mode
//! currently composites as `Normal` (plain alpha blending): visually
//! wrong for non-Normal modes, but opacity/isolation/draw-order are all
//! correct, and it's a contained fix once the snapshot pass exists.
//!
//! **`effects` (blur, drop shadow, glow) aren't applied on this path.**
//! `msx-render-cpu` already has all of these; the GPU path doesn't have
//! texture-sampling/blur infrastructure built yet.
//!
//! **Layers always draw after all non-layer content**, regardless of
//! document order — proper interleaving needs depth tracking that doesn't
//! exist yet. **Nested layers** (a `Layer` inside another `Layer`) aren't
//! rendered at all — `collect_layers` stops descending once it finds one.
//!
//! ## Shader and gradient fills inside a Layer
//!
//! `render_layer` is now given the real scene `defs` and the same
//! `ShaderFillPipeline`/`MaskedShaderComposite` the top-level render path
//! uses, so a vector/SDF/splat shape inside a `Layer` resolves
//! `Def::Shader`/gradient refs exactly like a top-level shape does —
//! vector shapes route through `vector::tessellate_elements_with_shaders`
//! (see that function's doc comment), SDF/splat shapes route through
//! their own `draw_all_elements`'s existing `Option<&...ShaderContext>`
//! parameter, which was already fully generic and only ever received
//! `None` from here. Opacity-on-shader-output and stroke-shader-fills
//! remain out of scope everywhere in this crate, Layer or not — see
//! `shader.rs`'s module doc.

use wgpu::util::DeviceExt;

use msx_ast::{Def, Element, Layer, Matrix2D};

use crate::masked_shader_composite::MaskedShaderComposite;
use crate::sdf::SdfPipeline;
use crate::sdf_shader::SdfShaderContext;
use crate::shader::ShaderFillPipeline;
use crate::splat::SplatPipeline;
use crate::splat_shader::SplatShaderContext;
use crate::target::OffscreenTarget;
use crate::vector;
use crate::VectorPipeline;

// `#[repr(C)]` alone is not enough to match WGSL's memory layout here.
// The WGSL struct is:
//     struct CompositeParams { opacity: f32, _pad: vec3<f32> }
// WGSL's `vec3<f32>` requires its own field to start at a 16-byte-aligned
// offset (not just occupy 12 bytes) — so `_pad` actually starts at byte
// 16, not byte 4, and the struct's overall size then rounds up to a
// multiple of its own alignment (16, from the vec3 member): 4 (opacity)
// + 12 (implicit padding before the vec3 can start) + 12 (the vec3
// itself) + 4 (trailing round-up to 32) = 32 bytes total.
//
// A plain Rust `[f32; 3]` has alignment 4, not 16 — `#[repr(C)]` never
// inserts WGSL's mandatory pre-vec3 padding or the final round-up, so
// the naive `opacity: f32, _pad: [f32; 3]` shape below produces an
// honestly-computed but wrong 16-byte struct. This was a real, live
// instance of the exact bug class `shader.rs::pack_uniforms` was written
// carefully to avoid (see its module doc) — caught only because a real
// wgpu validation error ("Buffer is bound with size 16 where the shader
// expects 32") surfaced it, the same way the TEXTURE_BINDING bug above
// only surfaced against a real adapter.
//
// `_pad`'s actual value is never read by the shader (only `params.opacity`
// is) — the padding only needs to be BIG ENOUGH (32 bytes total), not
// shaped to mirror the WGSL side field-by-field, since none of it is
// individually meaningful either way.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeParams {
    opacity: f32,
    _pad: [f32; 7],
}

pub struct LayerCompositor {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl LayerCompositor {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx layer composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx layer composite bind group layout"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx layer composite pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx layer composite pipeline"),
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
            label: Some("msx layer composite sampler"),
            ..Default::default()
        });

        LayerCompositor { pipeline, bind_group_layout, sampler }
    }

    /// Renders `layer`'s children into a fresh offscreen buffer (vector +
    /// shader fills + SDF + splat passes, same order and same real `defs`
    /// as the top-level render), then composites that buffer onto `view`
    /// at the layer's opacity.
    ///
    /// `scene_defs` is the *whole scene's* `defs` (i.e. `&scene.defs`,
    /// not something scoped to just this layer) — a `Layer`'s children
    /// can reference the same gradients/shader-defs as any top-level
    /// shape, there's no separate per-layer defs concept in the format.
    /// `shader_base_dir`/`time` are threaded straight through from
    /// `lib.rs`'s own call, unchanged, to every shader-def resolved here.
    #[allow(clippy::too_many_arguments)]
    pub fn render_layer(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        layer: &Layer,
        parent_transform: Matrix2D,
        canvas: (u32, u32),
        vector_pipeline: &VectorPipeline,
        sdf_pipeline: &SdfPipeline,
        splat_pipeline: &SplatPipeline,
        shader_pipeline: &ShaderFillPipeline,
        masked_shader_composite: &MaskedShaderComposite,
        scene_defs: &[Def],
        shader_base_dir: &std::path::Path,
        time: f32,
    ) {
        let local = layer.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
        let combined = parent_transform.concat(local);

        // RENDER_ATTACHMENT + COPY_SRC for the same reasons as the
        // top-level target (lib.rs), PLUS TEXTURE_BINDING — unlike that
        // top-level target, this buffer gets *sampled* by composite()'s
        // shader afterward (see the "msx layer composite bind group"
        // below), so it must be usable as a shader-bound texture too.
        // Missing this exact flag was a real, previously-undetected bug:
        // it type-checks fine either way (both are just
        // `wgpu::TextureUsages` values), so only a real wgpu validation
        // error against a real adapter ever caught it.
        let buffer_usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING;
        let buffer = OffscreenTarget::new(device, canvas.0, canvas.1, buffer_usage);
        let canvas_f = (canvas.0 as f32, canvas.1 as f32);

        // Real defs, not an empty one — this is the fix. Closes both the
        // shader-fill gap and the pre-existing (undocumented until now)
        // "gradient Paint::Ref resolves to nothing inside a Layer" gap,
        // since both draw from the exact same underlying defs a Layer's
        // children were never given access to before.
        let defs = vector::Defs::build(scene_defs);

        let (geometry, shader_shapes) = vector::tessellate_elements_with_shaders(&layer.children, combined, canvas_f, &defs);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx layer encoder"),
        });
        vector_pipeline.draw(
            device,
            &mut encoder,
            &buffer.view,
            &geometry,
            wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
        );
        // Same "always paint something sane" fallback as the top-level
        // path in lib.rs: a shader-ref shape whose source_ref doesn't
        // resolve falls back to its flat fallback_color rather than
        // vanishing or aborting the rest of this layer's render.
        for shape in &shader_shapes {
            if let Err(e) = shader_pipeline.draw(device, &mut encoder, &buffer.view, shader_base_dir, shape, time) {
                eprintln!("msx-render-gpu: {e} — falling back to fallback_color (inside layer)");
                vector_pipeline.draw_fallback_fill(device, &mut encoder, &buffer.view, &shape.vertices, &shape.indices, shape.shader.fallback_color);
            }
        }
        sdf_pipeline.draw_all_elements(
            device, &mut encoder, &buffer.view, &layer.children, combined, canvas_f, &defs,
            Some(&SdfShaderContext { shader_pipeline, composite: masked_shader_composite, shader_base_dir, time }),
        );
        splat_pipeline.draw_all_elements(
            device, &mut encoder, &buffer.view, &layer.children, combined, canvas_f, &defs,
            Some(&SplatShaderContext { shader_pipeline, composite: masked_shader_composite, shader_base_dir, time }),
        );
        queue.submit(std::iter::once(encoder.finish()));

        self.composite(device, queue, view, &buffer.view, layer.opacity as f32);
    }

    fn composite(&self, device: &wgpu::Device, queue: &wgpu::Queue, dst_view: &wgpu::TextureView, src_view: &wgpu::TextureView, opacity: f32) {
        let params = CompositeParams { opacity, _pad: [0.0; 7] };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx layer composite params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx layer composite bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: params_buffer.as_entire_binding() },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx layer composite encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("msx layer composite pass"),
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
        queue.submit(std::iter::once(encoder.finish()));
    }
}

/// Recurses through `Group`, collects `Layer` elements with their
/// accumulated transform — does NOT descend into a found `Layer`'s own
/// children (no nested-layer support; see module docs).
pub fn collect_layers<'a>(elements: &'a [Element], transform: Matrix2D, out: &mut Vec<(&'a Layer, Matrix2D)>) {
    for el in elements {
        match el {
            Element::Layer(l) => out.push((l, transform)),
            Element::Group(g) => {
                let local = g.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                collect_layers(&g.children, transform.concat(local), out);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Element as MsxElement, Group};

    /// `composite.wgsl`'s `struct CompositeParams { opacity: f32, _pad:
    /// vec3<f32> }` is 32 bytes under WGSL's own uniform-address-space
    /// layout rules (the `vec3<f32>` field needs a 16-byte-aligned start
    /// offset, not just its own 12 bytes — see the type's doc comment for
    /// the full arithmetic). This Rust-side type has to match that exact
    /// byte size or `create_bind_group`/the draw call fails a real wgpu
    /// validation check at runtime — which is exactly what happened
    /// (`Buffer is bound with size 16 where the shader expects 32`)
    /// before this test existed. A plain `size_of` assertion here is
    /// cheap, needs no GPU adapter to run, and would have caught this
    /// before it ever reached real hardware.
    #[test]
    fn composite_params_matches_the_wgsl_struct_size() {
        assert_eq!(std::mem::size_of::<CompositeParams>(), 32);
    }

    #[test]
    fn collect_layers_finds_top_level_layer() {
        let layer = Layer::new(vec![]);
        let elements = vec![MsxElement::Layer(layer)];
        let mut out = Vec::new();
        collect_layers(&elements, Matrix2D::identity(), &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn collect_layers_recurses_through_groups() {
        let layer = Layer::new(vec![]);
        let group = Group::new(vec![MsxElement::Layer(layer)]);
        let elements = vec![MsxElement::Group(group)];
        let mut out = Vec::new();
        collect_layers(&elements, Matrix2D::identity(), &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn collect_layers_does_not_descend_into_nested_layers() {
        let nested = Layer::new(vec![]);
        let outer = Layer::new(vec![MsxElement::Layer(nested)]);
        let elements = vec![MsxElement::Layer(outer)];
        let mut out = Vec::new();
        collect_layers(&elements, Matrix2D::identity(), &mut out);
        assert_eq!(out.len(), 1, "only the outer layer should be found");
    }
                    }
