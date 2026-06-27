// render/msx-render-gpu/src/splat.rs
//! `GaussianSplat` rendering: one instanced billboard quad per splat,
//! gaussian falloff evaluated per-pixel in the fragment shader — the same
//! math as `msx-splat::gaussian::evaluate`, transcribed to WGSL. Far
//! simpler than `sdf.rs`'s flattened-tree machine: splats have no
//! recursive structure, so one instance buffer covers the whole scene's
//! splats (minus anything inside a `Layer` — see below) in a single draw
//! call regardless of count.
//!
//! Quad corners come from `@builtin(vertex_index)` rather than a vertex
//! buffer — every instance shares the same four corners, only the
//! per-instance transform differs.
//!
//! `collect_splats` does NOT recurse into `Element::Layer` children, same
//! reason as `sdf.rs::collect_sdf_nodes` — `layer.rs` handles those in its
//! own isolated pass.

use wgpu::util::DeviceExt;

use msx_ast::{Element, GaussianSplat, Matrix2D, Scene};

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

    pub fn draw_all(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, scene: &Scene) {
        let canvas = (scene.canvas.width as f32, scene.canvas.height as f32);
        self.draw_all_elements(device, encoder, view, &scene.elements, Matrix2D::identity(), canvas);
    }

    /// Entry point for `layer.rs`: draw just the `Splat` shapes within one
    /// layer's children, scoped to its own buffer.
    pub fn draw_all_elements(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, elements: &[Element], base_transform: Matrix2D, canvas: (f32, f32)) {
        let mut instances = Vec::new();
        collect_splats(elements, base_transform, &mut instances);
        if instances.is_empty() {
            return;
        }

        let canvas_params = CanvasParams { width: canvas.0, height: canvas.1, _pad0: 0.0, _pad1: 0.0 };

        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx splat instance buffer"),
            contents: bytemuck::cast_slice(&instances),
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
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
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

/// Does NOT recurse into `Element::Layer` — see module docs.
fn collect_splats(elements: &[Element], transform: Matrix2D, out: &mut Vec<SplatInstance>) {
    for el in elements {
        match el {
            Element::Splat(s) => out.push(to_instance(s, transform)),
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
fn to_instance(s: &GaussianSplat, transform: Matrix2D) -> SplatInstance {
    let (cx, cy) = apply_matrix(transform, (s.x as f32, s.y as f32));
    let radius_x = effective_radius_axis(s.sigma_x, 0.02);
    let radius_y = effective_radius_axis(s.sigma_y, 0.02);

    SplatInstance {
        center: [cx, cy],
        half_extents: [radius_x, radius_y],
        rotation: s.rotation as f32,
        sigma: [s.sigma_x as f32, s.sigma_y as f32],
        color: [
            s.color.r as f32 / 255.0,
            s.color.g as f32 / 255.0,
            s.color.b as f32 / 255.0,
            (s.color.a as f64 / 255.0 * s.opacity) as f32,
        ],
    }
}

/// Per-axis version of `msx_splat::gaussian::effective_radius`, which uses
/// `max(sigma_x, sigma_y)` for a single combined cull radius — here each
/// axis gets its own extent so a strongly anisotropic splat gets a snugly
/// rotated quad instead of an oversized square.
fn effective_radius_axis(sigma: f64, threshold: f64) -> f32 {
    (sigma * (-2.0 * threshold.ln()).sqrt()) as f32
}

fn apply_matrix(m: Matrix2D, p: (f32, f32)) -> (f32, f32) {
    (m.a as f32 * p.0 + m.c as f32 * p.1 + m.e as f32, m.b as f32 * p.0 + m.d as f32 * p.1 + m.f as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Color, Element as MsxElement, Group, Layer};

    #[test]
    fn to_instance_bakes_opacity_into_alpha() {
        let splat = GaussianSplat::circle(10.0, 20.0, 5.0, Color::rgba(255, 0, 0, 200), 0.5);
        let instance = to_instance(&splat, Matrix2D::identity());
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
    }
