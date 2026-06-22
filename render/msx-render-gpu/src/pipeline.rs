// render/msx-render-gpu/src/pipeline.rs
//! Render pipeline for tessellated vector geometry: one pipeline, one
//! vertex buffer, one index buffer, one draw call per render — rebuilt
//! fresh each frame for now (no buffer caching/reuse), since correctness
//! matters more than per-frame allocation cost at this stage.

use wgpu::util::DeviceExt;

use crate::vector::{Vertex, VectorGeometry};

pub struct VectorPipeline {
    pipeline: wgpu::RenderPipeline,
}

impl VectorPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx vector shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/vector.wgsl").into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx vector pipeline layout"),
            bind_group_layouts: &[],
            // NOTE: current wgpu appears to have renamed push constants to
            // "immediate data" — this field used to be (and may still be,
            // depending on exactly which patch resolves)
            // `push_constant_ranges: &[]`. One-line fix either way if this
            // doesn't compile.
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 8, shader_location: 1 },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx vector pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
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

        VectorPipeline { pipeline }
    }

    /// Clears `view` to `clear_color`, then draws `geometry` over it in one
    /// pass — empty geometry still gets the clear, just no draw call.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        geometry: &VectorGeometry,
        clear_color: wgpu::Color,
    ) {
        let buffers = if !geometry.indices.is_empty() {
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx vertex buffer"),
                contents: bytemuck::cast_slice(&geometry.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx index buffer"),
                contents: bytemuck::cast_slice(&geometry.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            Some((vertex_buffer, index_buffer))
        } else {
            None
        };

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx vector pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
        });

        if let Some((vertex_buffer, index_buffer)) = &buffers {
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..geometry.indices.len() as u32, 0, 0..1);
        }
    }
                                             }
