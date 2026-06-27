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

use wgpu::util::DeviceExt;

use msx_ast::{Element, Layer, Matrix2D};

use crate::sdf::SdfPipeline;
use crate::splat::SplatPipeline;
use crate::target::OffscreenTarget;
use crate::vector;
use crate::VectorPipeline;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeParams {
    opacity: f32,
    _pad: [f32; 3],
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
    /// SDF + splat passes, same as the top-level render), then composites
    /// that buffer onto `view` at the layer's opacity.
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
    ) {
        let local = layer.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
        let combined = parent_transform.concat(local);

        let buffer = OffscreenTarget::new(device, canvas.0, canvas.1);
        let canvas_f = (canvas.0 as f32, canvas.1 as f32);
        let geometry = vector::tessellate_elements(&layer.children, combined, canvas_f);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx layer encoder"),
        });
        vector_pipeline.draw(
            device,
            &mut encoder,
            &buffer.view,
            &geometry,
            wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }, // transparent, not the canvas background
        );
        sdf_pipeline.draw_all_elements(device, &mut encoder, &buffer.view, &layer.children, combined, canvas_f);
        splat_pipeline.draw_all_elements(device, &mut encoder, &buffer.view, &layer.children, combined, canvas_f);
        queue.submit(std::iter::once(encoder.finish()));

        self.composite(device, queue, view, &buffer.view, layer.opacity as f32);
    }

    fn composite(&self, device: &wgpu::Device, queue: &wgpu::Queue, dst_view: &wgpu::TextureView, src_view: &wgpu::TextureView, opacity: f32) {
        let params = CompositeParams { opacity, _pad: [0.0; 3] };
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
