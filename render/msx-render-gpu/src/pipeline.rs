// render/msx-render-gpu/src/pipeline.rs
//! Render pipeline for tessellated vector geometry: one pipeline, one
//! vertex buffer, one index buffer, one draw call per render. Buffers are
//! rebuilt fresh each frame (no caching or reuse), since correctness
//! matters more than per-frame allocation cost at this stage.
//!
//! Every draw also binds a storage buffer of gradient records at group 0,
//! binding 0. Flat geometry never reads it, but the layout is fixed, so a
//! draw with no gradients still binds a one-record placeholder.

use bytemuck::Zeroable;
use wgpu::util::DeviceExt;

use crate::gradient::GradientDesc;
use crate::vector::{VectorGeometry, Vertex};

pub struct VectorPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl VectorPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx vector shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/vector.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx vector bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx vector pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            // Confirmed against wgpu 26.0.1 source (wgpu/src/api/pipeline_layout.rs):
            // PipelineLayoutDescriptor has label / bind_group_layouts /
            // push_constant_ranges, nothing named immediate_size in this
            // generation of the API.
            push_constant_ranges: &[],
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 8, shader_location: 1 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 24, shader_location: 2 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 32, shader_location: 3 },
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

        VectorPipeline { pipeline, bind_group_layout }
    }

    /// Clears `view` to `clear_color`, then draws `geometry` over it in one
    /// pass. Empty geometry still gets the clear, just no draw call.
    pub fn draw(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, geometry: &VectorGeometry, clear_color: wgpu::Color) {
        self.record(device, encoder, view, geometry, wgpu::LoadOp::Clear(clear_color), "msx vector pass");
    }

    /// Same as `draw`, minus the clear (`LoadOp::Load`).
    ///
    /// Used by `layer::render_ordered`'s interleaved walk: once something
    /// has painted onto `view`, a later run's vector draw must layer on top
    /// of it, not wipe it back to the clear color.
    pub fn draw_loaded(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, geometry: &VectorGeometry) {
        self.record(device, encoder, view, geometry, wgpu::LoadOp::Load, "msx vector pass (loaded)");
    }

    /// Paints `positions`/`indices` with a single solid color, layered over
    /// whatever is already in `view`. A flat fill is just every vertex
    /// sharing one color, so this reuses the same pipeline.
    ///
    /// Used by `shader.rs`'s graceful-fallback path: when a shader-def's
    /// `source_ref` fails to resolve at render time, the shape paints with
    /// its declared `fallback_color` instead.
    pub fn draw_fallback_fill(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        positions: &[[f32; 2]],
        indices: &[u32],
        color: msx_ast::Color,
    ) {
        if indices.is_empty() {
            return;
        }
        let rgba = [color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0, color.a as f32 / 255.0];
        let geometry = VectorGeometry {
            vertices: positions.iter().map(|&position| Vertex::flat(position, rgba)).collect(),
            indices: indices.to_vec(),
            gradients: Vec::new(),
        };
        self.record(device, encoder, view, &geometry, wgpu::LoadOp::Load, "msx shader-fallback pass");
    }

    fn record(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, geometry: &VectorGeometry, load: wgpu::LoadOp<wgpu::Color>, label: &str) {
        let draw = if geometry.indices.is_empty() {
            None
        } else {
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
            let placeholder = [GradientDesc::zeroed()];
            let records: &[GradientDesc] = if geometry.gradients.is_empty() { &placeholder } else { &geometry.gradients };
            let gradient_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx gradient buffer"),
                contents: bytemuck::cast_slice(records),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("msx vector bind group"),
                layout: &self.bind_group_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: gradient_buffer.as_entire_binding() }],
            });
            Some((vertex_buffer, index_buffer, bind_group))
        };

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                // depth_slice only applies to 3D texture views; this target
                // is always a plain 2D color texture, so None.
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        if let Some((vertex_buffer, index_buffer, bind_group)) = &draw {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..geometry.indices.len() as u32, 0, 0..1);
        }
    }
}
