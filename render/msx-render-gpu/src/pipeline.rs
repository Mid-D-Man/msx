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
                // depth_slice only applies to 3D texture views; this target
                // is always a plain 2D color texture, so None.
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        if let Some((vertex_buffer, index_buffer)) = &buffers {
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..geometry.indices.len() as u32, 0, 0..1);
        }
    }

    /// Same as `draw` above, minus the clear — `LoadOp::Load` instead of
    /// `LoadOp::Clear(clear_color)`, everything else identical (same
    /// pipeline, same vertex/index buffer setup, same draw call).
    ///
    /// Exists for `layer::render_ordered`'s interleaved walk: once a
    /// Layer composite has painted onto `view` between two runs of
    /// non-Layer siblings, any *later* run's own vector draw must layer
    /// on top of what's already there, not wipe it back to
    /// `clear_color` — the same reason `draw_fallback_fill` above never
    /// clears either, just for a full `VectorGeometry` batch instead of
    /// a single fallback color. `draw` itself is untouched and still
    /// used exactly as before for the first thing painted into a given
    /// target (top-level render, and the first run inside a fresh
    /// `Layer` buffer) — this is purely additive.
    pub fn draw_loaded(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        geometry: &VectorGeometry,
    ) {
        let buffers = if !geometry.indices.is_empty() {
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx vertex buffer (loaded)"),
                contents: bytemuck::cast_slice(&geometry.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx index buffer (loaded)"),
                contents: bytemuck::cast_slice(&geometry.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            Some((vertex_buffer, index_buffer))
        } else {
            None
        };

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx vector pass (loaded)"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        if let Some((vertex_buffer, index_buffer)) = &buffers {
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..geometry.indices.len() as u32, 0, 0..1);
        }
    }
    /// single solid color, layered atop whatever's already in `view`
    /// (`LoadOp::Load`, not `Clear` — this is never the first draw in a
    /// pass). Reuses this same pipeline (it already accepts position+
    /// color vertices; a flat fill is just every vertex sharing one
    /// color), so no second pipeline/shader module is needed.
    ///
    /// Exists specifically for `shader.rs`'s graceful-fallback path:
    /// `lib.rs` calls this when a shader-def's `source_ref` fails to
    /// resolve at render time, painting the shape with its declared
    /// `fallback_color` instead — same "always paint something sane"
    /// principle every renderer applies to shader-filled shapes, just
    /// reached via a different call path (a *runtime* failure rather
    /// than "no WGSL-executing renderer exists at all").
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
        let vertices: Vec<Vertex> = positions.iter().map(|&position| Vertex { position, color: rgba }).collect();

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx shader-fallback vertex buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx shader-fallback index buffer"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx shader-fallback pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
    }
    }
