// render/msx-render-gpu/src/sdf.rs
//! `SdfTree` evaluation on the GPU. WGSL has no recursion, so the tree is
//! flattened (postorder, on the CPU) into a linear `SdfOp` array uploaded
//! as a storage buffer, and the fragment shader evaluates it with an
//! explicit fixed-size array acting as a stack — push primitive distances,
//! pop operands for n-ary/binary ops, push results — instead of recursive
//! calls. One draw call per `Sdf` element: a screen-space quad sized to a
//! conservative bbox (same estimation logic as
//! `msx-render-cpu::sdf_raster::sdf_bounds`, duplicated here rather than
//! shared — same pattern as every other small geometry helper across the
//! renderer crates), with the inverse transform passed in as a uniform so
//! the fragment shader can map each pixel back to the tree's local space.
//!
//! Ordering note: every `Sdf` element draws *after* all vector geometry,
//! regardless of where it sits in document order — proper interleaving
//! would need either a depth buffer with per-element depth or genuinely
//! per-element pipeline switching in tree order, neither of which exists
//! yet. Flagged, not hidden.

use wgpu::util::DeviceExt;

use msx_ast::{Color, Element, Matrix2D, Paint, SdfNode, SdfTree, Scene};

// ── Flattened op representation ─────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SdfOp {
    pub kind: u32,
    pub count: u32,
    pub param0: f32,
    pub param1: f32,
    pub param2: f32,
    pub param3: f32,
    pub param4: f32,
    pub param5: f32,
}

const OP_CIRCLE: u32 = 0;
const OP_BOX: u32 = 1;
const OP_LINE: u32 = 2;
const OP_RING: u32 = 3;
const OP_ARC: u32 = 4;
const OP_UNION: u32 = 5;
const OP_SMOOTH_UNION: u32 = 6;
const OP_SUBTRACT: u32 = 7;
const OP_SMOOTH_SUBTRACT: u32 = 8;
const OP_INTERSECT: u32 = 9;
const OP_SMOOTH_INTERSECT: u32 = 10;
const OP_OFFSET: u32 = 11;

fn op(kind: u32, count: u32, p: [f32; 6]) -> SdfOp {
    SdfOp { kind, count, param0: p[0], param1: p[1], param2: p[2], param3: p[3], param4: p[4], param5: p[5] }
}

/// Postorder flatten — see module docs for why children of `Union`/
/// `SmoothUnion` are pushed in *reverse*.
pub fn flatten_tree(tree: &SdfTree, ops: &mut Vec<SdfOp>) {
    match tree {
        SdfTree::Circle { cx, cy, r } => {
            ops.push(op(OP_CIRCLE, 0, [*cx as f32, *cy as f32, *r as f32, 0.0, 0.0, 0.0]));
        }
        SdfTree::Box { x, y, width, height, corner_radius } => {
            ops.push(op(OP_BOX, 0, [*x as f32, *y as f32, *width as f32, *height as f32, *corner_radius as f32, 0.0]));
        }
        SdfTree::Line { x1, y1, x2, y2, thickness } => {
            ops.push(op(OP_LINE, 0, [*x1 as f32, *y1 as f32, *x2 as f32, *y2 as f32, *thickness as f32, 0.0]));
        }
        SdfTree::Ring { cx, cy, r, thickness } => {
            ops.push(op(OP_RING, 0, [*cx as f32, *cy as f32, *r as f32, *thickness as f32, 0.0, 0.0]));
        }
        SdfTree::Arc { cx, cy, r, angle_start, angle_end, thickness } => {
            ops.push(op(OP_ARC, 0, [*cx as f32, *cy as f32, *r as f32, *angle_start as f32, *angle_end as f32, *thickness as f32]));
        }
        SdfTree::Union(children) => {
            for child in children.iter().rev() {
                flatten_tree(child, ops);
            }
            ops.push(op(OP_UNION, children.len() as u32, [0.0; 6]));
        }
        SdfTree::SmoothUnion { children, k } => {
            for child in children.iter().rev() {
                flatten_tree(child, ops);
            }
            ops.push(op(OP_SMOOTH_UNION, children.len() as u32, [*k as f32, 0.0, 0.0, 0.0, 0.0, 0.0]));
        }
        SdfTree::Subtract { a, b } => {
            flatten_tree(a, ops);
            flatten_tree(b, ops);
            ops.push(op(OP_SUBTRACT, 2, [0.0; 6]));
        }
        SdfTree::SmoothSubtract { a, b, k } => {
            flatten_tree(a, ops);
            flatten_tree(b, ops);
            ops.push(op(OP_SMOOTH_SUBTRACT, 2, [*k as f32, 0.0, 0.0, 0.0, 0.0, 0.0]));
        }
        SdfTree::Intersect { a, b } => {
            flatten_tree(a, ops);
            flatten_tree(b, ops);
            ops.push(op(OP_INTERSECT, 2, [0.0; 6]));
        }
        SdfTree::SmoothIntersect { a, b, k } => {
            flatten_tree(a, ops);
            flatten_tree(b, ops);
            ops.push(op(OP_SMOOTH_INTERSECT, 2, [*k as f32, 0.0, 0.0, 0.0, 0.0, 0.0]));
        }
        SdfTree::Offset { child, amount } => {
            flatten_tree(child, ops);
            ops.push(op(OP_OFFSET, 1, [*amount as f32, 0.0, 0.0, 0.0, 0.0, 0.0]));
        }
    }
}

// ── Per-draw uniform / vertex layout ────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SdfParams {
    pub inv_row0: [f32; 4], // a, c, e, unused
    pub inv_row1: [f32; 4], // b, d, f, unused
    pub fill_color: [f32; 4],
    pub stroke_color: [f32; 4],
    pub stroke_width: f32,
    pub has_stroke: f32, // 0.0 / 1.0 — avoids mixing bool semantics into a uniform struct
    pub op_count: u32,
    pub _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SdfVertex {
    pub clip_position: [f32; 2],
    pub screen_position: [f32; 2],
}

// ── Pipeline ─────────────────────────────────────────────────────────────────

pub struct SdfPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl SdfPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx sdf shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sdf.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx sdf bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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
            label: Some("msx sdf pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0, // see pipeline.rs's note on this field
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SdfVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx sdf pipeline"),
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

        SdfPipeline { pipeline, bind_group_layout }
    }

    /// Draws every `Sdf` element in the scene over whatever's already in
    /// `view` — one draw call per shape.
    pub fn draw_all(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, scene: &Scene) {
        let canvas = (scene.canvas.width as f32, scene.canvas.height as f32);
        let mut nodes = Vec::new();
        collect_sdf_nodes(&scene.elements, Matrix2D::identity(), &mut nodes);

        for (node, transform) in &nodes {
            self.draw_one(device, encoder, view, node, *transform, canvas);
        }
    }

    fn draw_one(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, node: &SdfNode, transform: Matrix2D, canvas: (f32, f32)) {
        let local = node.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
        let combined = transform.concat(local);
        let Some(inv) = invert_matrix(combined) else { return };

        let mut ops = Vec::new();
        flatten_tree(&node.tree, &mut ops);
        if ops.is_empty() {
            return;
        }

        let local_bounds = sdf_bounds(&node.tree);
        let screen_bounds = transform_bounds(local_bounds, combined);
        let (verts, indices) = quad_vertices(screen_bounds, canvas);

        let fill_color = color_to_rgba(paint_color(&node.fill));
        let (stroke_color, stroke_width, has_stroke) = match (&node.stroke, node.stroke_width) {
            (Some(paint), Some(w)) => (color_to_rgba(paint_color(paint)), w as f32, 1.0f32),
            _ => ([0.0; 4], 0.0, 0.0f32),
        };

        let params = SdfParams {
            inv_row0: [inv.a as f32, inv.c as f32, inv.e as f32, 0.0],
            inv_row1: [inv.b as f32, inv.d as f32, inv.f as f32, 0.0],
            fill_color,
            stroke_color,
            stroke_width,
            has_stroke,
            op_count: ops.len() as u32,
            _pad: 0.0,
        };

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf vertex buffer"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf index buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ops_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf ops buffer"),
            contents: bytemuck::cast_slice(&ops),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf params buffer"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx sdf bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ops_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
            ],
        });

        // `Load`, not `Clear` — this pass must preserve whatever the
        // vector pass (or an earlier SDF draw) already painted.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx sdf pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }
}

fn collect_sdf_nodes<'a>(elements: &'a [Element], transform: Matrix2D, out: &mut Vec<(&'a SdfNode, Matrix2D)>) {
    for el in elements {
        match el {
            Element::Sdf(node) => out.push((node, transform)),
            Element::Group(g) => {
                let local = g.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                collect_sdf_nodes(&g.children, transform.concat(local), out);
            }
            Element::Layer(l) => {
                let local = l.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                collect_sdf_nodes(&l.children, transform.concat(local), out);
            }
            _ => {}
        }
    }
}

fn paint_color(paint: &Paint) -> Color {
    match paint {
        Paint::Color(c) => *c,
        Paint::CurrentColor => Color::BLACK,
        // None/gradient SDF fills: same gap msx-render-cpu has, same reason.
        Paint::None | Paint::Ref(_) => Color::rgba(0, 0, 0, 0),
    }
}

fn color_to_rgba(c: Color) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, c.a as f32 / 255.0]
}

fn quad_vertices(bounds: (f32, f32, f32, f32), canvas: (f32, f32)) -> ([SdfVertex; 4], [u16; 6]) {
    let (min_x, min_y, max_x, max_y) = bounds;
    let to_clip = |x: f32, y: f32| [(x / canvas.0) * 2.0 - 1.0, 1.0 - (y / canvas.1) * 2.0];
    let verts = [
        SdfVertex { clip_position: to_clip(min_x, min_y), screen_position: [min_x, min_y] },
        SdfVertex { clip_position: to_clip(max_x, min_y), screen_position: [max_x, min_y] },
        SdfVertex { clip_position: to_clip(max_x, max_y), screen_position: [max_x, max_y] },
        SdfVertex { clip_position: to_clip(min_x, max_y), screen_position: [min_x, max_y] },
    ];
    (verts, [0, 1, 2, 0, 2, 3])
}

// ── Matrix helpers + bbox estimation ────────────────────────────────────────
// Same math as msx-render-cpu's geom.rs/sdf_raster.rs — duplicated rather
// than shared, consistent with every renderer crate keeping its own small
// geometry helpers instead of depending on a sibling renderer crate.

fn apply_matrix(m: Matrix2D, p: (f32, f32)) -> (f32, f32) {
    (m.a as f32 * p.0 + m.c as f32 * p.1 + m.e as f32, m.b as f32 * p.0 + m.d as f32 * p.1 + m.f as f32)
}

fn invert_matrix(m: Matrix2D) -> Option<Matrix2D> {
    let det = m.a * m.d - m.b * m.c;
    if det.abs() < 1e-9 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m.d * inv_det;
    let b = -m.b * inv_det;
    let c = -m.c * inv_det;
    let d = m.a * inv_det;
    let e = -(a * m.e + c * m.f);
    let f = -(b * m.e + d * m.f);
    Some(Matrix2D { a, b, c, d, e, f })
}

fn transform_bounds(b: (f32, f32, f32, f32), m: Matrix2D) -> (f32, f32, f32, f32) {
    let corners = [
        apply_matrix(m, (b.0, b.1)),
        apply_matrix(m, (b.2, b.1)),
        apply_matrix(m, (b.0, b.3)),
        apply_matrix(m, (b.2, b.3)),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (x, y) in corners {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}

fn sdf_bounds(tree: &SdfTree) -> (f32, f32, f32, f32) {
    match tree {
        SdfTree::Circle { cx, cy, r } => circle_bounds(*cx, *cy, *r),
        SdfTree::Box { x, y, width, height, .. } => (*x as f32, *y as f32, (*x + *width) as f32, (*y + *height) as f32),
        SdfTree::Line { x1, y1, x2, y2, thickness } => {
            let pad = *thickness as f32 * 0.5;
            (x1.min(*x2) as f32 - pad, y1.min(*y2) as f32 - pad, x1.max(*x2) as f32 + pad, y1.max(*y2) as f32 + pad)
        }
        SdfTree::Ring { cx, cy, r, thickness } | SdfTree::Arc { cx, cy, r, thickness, .. } => {
            circle_bounds(*cx, *cy, r + thickness * 0.5)
        }
        SdfTree::Union(children) => union_bounds(children.iter().map(sdf_bounds)),
        SdfTree::SmoothUnion { children, k } => pad_bounds(union_bounds(children.iter().map(sdf_bounds)), *k as f32),
        SdfTree::Subtract { a, .. } => sdf_bounds(a),
        SdfTree::SmoothSubtract { a, k, .. } => pad_bounds(sdf_bounds(a), *k as f32),
        SdfTree::Intersect { a, b } => intersect_bounds(sdf_bounds(a), sdf_bounds(b)),
        SdfTree::SmoothIntersect { a, b, k } => pad_bounds(intersect_bounds(sdf_bounds(a), sdf_bounds(b)), *k as f32),
        SdfTree::Offset { child, amount } => pad_bounds(sdf_bounds(child), (*amount as f32).max(0.0)),
    }
}

fn circle_bounds(cx: f64, cy: f64, r: f64) -> (f32, f32, f32, f32) {
    ((cx - r) as f32, (cy - r) as f32, (cx + r) as f32, (cy + r) as f32)
}

fn union_bounds(mut iter: impl Iterator<Item = (f32, f32, f32, f32)>) -> (f32, f32, f32, f32) {
    let first = iter.next().unwrap_or((0.0, 0.0, 0.0, 0.0));
    iter.fold(first, |acc, b| (acc.0.min(b.0), acc.1.min(b.1), acc.2.max(b.2), acc.3.max(b.3)))
}

fn intersect_bounds(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3))
}

fn pad_bounds(b: (f32, f32, f32, f32), amount: f32) -> (f32, f32, f32, f32) {
    (b.0 - amount, b.1 - amount, b.2 + amount, b.3 + amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_circle_produces_one_op() {
        let mut ops = Vec::new();
        flatten_tree(&SdfTree::Circle { cx: 1.0, cy: 2.0, r: 3.0 }, &mut ops);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OP_CIRCLE);
    }

    #[test]
    fn flatten_subtract_pushes_a_then_b_then_op() {
        let mut ops = Vec::new();
        let tree = SdfTree::Circle { cx: 0.0, cy: 0.0, r: 50.0 }.subtract(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 30.0 });
        flatten_tree(&tree, &mut ops);
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0].param2, 50.0); // a (r=50) pushed first
        assert_eq!(ops[1].param2, 30.0); // b (r=30) pushed second
        assert_eq!(ops[2].kind, OP_SUBTRACT);
    }

    #[test]
    fn flatten_smooth_union_reverses_child_push_order() {
        let mut ops = Vec::new();
        let tree = SdfTree::SmoothUnion {
            children: vec![
                SdfTree::Circle { cx: 0.0, cy: 0.0, r: 10.0 },
                SdfTree::Circle { cx: 0.0, cy: 0.0, r: 20.0 },
                SdfTree::Circle { cx: 0.0, cy: 0.0, r: 30.0 },
            ],
            k: 0.5,
        };
        flatten_tree(&tree, &mut ops);
        // Children pushed in reverse (30, 20, 10) so LIFO popping in the
        // shader recovers forward fold order (10, 20, 30) — see module docs.
        assert_eq!(ops[0].param2, 30.0);
        assert_eq!(ops[1].param2, 20.0);
        assert_eq!(ops[2].param2, 10.0);
        assert_eq!(ops[3].kind, OP_SMOOTH_UNION);
        assert_eq!(ops[3].count, 3);
    }
              }
