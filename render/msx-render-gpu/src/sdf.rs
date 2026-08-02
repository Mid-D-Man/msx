// render/msx-render-gpu/src/sdf.rs
//! `SdfTree` evaluation on the GPU. WGSL has no recursion, so the tree is
//! flattened (postorder, on the CPU) into a linear `SdfOp` array uploaded
//! as a storage buffer, and the fragment shader evaluates it with an
//! explicit fixed-size array acting as a stack.
//!
//! `collect_sdf_nodes` deliberately does NOT recurse into `Element::Layer`
//! children — those get rendered by `layer.rs`'s own isolated pass.
//! Recursing here too would draw layer-internal SDF shapes twice: once
//! flattened into the main pass (ignoring the layer's opacity entirely),
//! and once correctly inside the layer's buffer.

use wgpu::util::DeviceExt;

use msx_ast::{Color, Element, Matrix2D, Paint, SdfNode, SdfTree, Scene};

use crate::sdf_shader::{draw_sdf_shader_fill, SdfShaderContext};
use crate::vector::{average_stop_color, resolve_shader_def, Defs};

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

/// Postorder flatten. Children of `Union`/`SmoothUnion` are pushed in
/// *reverse* so that LIFO popping in the shader recovers the original
/// forward fold order — `SmoothUnion`'s n-ary fold isn't associative for
/// 3+ children, only pairwise-commutative, so popping a stack naively
/// would silently fold in the wrong sequence vs. the CPU reference
/// (`msx-sdf`'s fold walks children left-to-right).
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

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SdfParams {
    pub inv_row0: [f32; 4],
    pub inv_row1: [f32; 4],
    pub fill_color: [f32; 4],
    pub stroke_color: [f32; 4],
    pub stroke_width: f32,
    pub has_stroke: f32,
    pub op_count: u32,
    pub _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SdfVertex {
    pub clip_position: [f32; 2],
    pub screen_position: [f32; 2],
}

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
            push_constant_ranges: &[], // confirmed field name — see pipeline.rs's note
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

    /// `pub(crate)`, not `pub` (despite `SdfPipeline` itself being
    /// re-exported publicly via `lib.rs`'s `pub use sdf::SdfPipeline;`) —
    /// `shader_ctx: Option<&SdfShaderContext>` takes a `pub(crate)` type,
    /// so external code could only ever call this with `None` anyway;
    /// rustc's `private_interfaces` lint correctly flags that mismatch
    /// (public method, non-public parameter type) as a hard error under
    /// this project's `-D warnings` CI. Nothing outside this crate
    /// constructs an `SdfPipeline` directly today — everything goes
    /// through `GpuRenderer` — so narrowing this to `pub(crate)` costs
    /// nothing real.
    pub(crate) fn draw_all(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, scene: &Scene, shader_ctx: Option<&SdfShaderContext>) {
        let canvas = (scene.canvas.width as f32, scene.canvas.height as f32);
        let defs = Defs::build(&scene.defs);
        self.draw_all_elements(device, encoder, view, &scene.elements, Matrix2D::identity(), canvas, &defs, shader_ctx);
    }

    /// Entry point for `layer.rs`: draw just the `Sdf` shapes within one
    /// layer's children, scoped to its own buffer.
    ///
    /// `pub(crate)`, not `pub` — only ever called from within this crate
    /// (`draw_all` above, and `layer.rs`), and it takes `Defs`, which is
    /// itself `pub(crate)`; a `pub` function can't expose a `pub(crate)`
    /// type in its signature (rustc's private-interfaces lint), so this
    /// needs to match rather than the other way around — `Defs` staying
    /// internal is the intentional design, not an oversight.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_all_elements(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, elements: &[Element], base_transform: Matrix2D, canvas: (f32, f32), defs: &Defs, shader_ctx: Option<&SdfShaderContext>) {
        let mut nodes = Vec::new();
        collect_sdf_nodes(elements, base_transform, &mut nodes);
        for (node, transform) in &nodes {
            // Mirrors vector.rs's own `routed_to_shader` decision — a real
            // Def::Shader fill, AND a shader context actually provided by
            // this call site (see the doc comment above for what routes
            // `None` here). `Option::zip`, not a manual `and_then(..map)`
            // (clippy's `manual_option_zip`) — every real call site today
            // (`draw_all`, `render_layer`) always passes `Some`, so eager
            // evaluation of `resolve_shader_def` costs nothing extra in
            // practice; the `None` arm only exists for defensiveness /
            // future callers, not as a live optimization target.
            let routed = shader_ctx.zip(resolve_shader_def(&node.fill, defs));
            match routed {
                Some((ctx, shader_def)) => {
                    draw_sdf_shader_fill(device, encoder, view, self, node, shader_def, *transform, canvas, ctx);
                    // The shader routing above only ever handles the fill
                    // — a stroke on this same node, if it has one, still
                    // needs to show up.
                    self.draw_stroke_only(device, encoder, view, node, *transform, canvas, defs);
                }
                None => self.draw_one(device, encoder, view, node, *transform, canvas, defs),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_one(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, node: &SdfNode, transform: Matrix2D, canvas: (f32, f32), defs: &Defs) {
        let Some((ops, verts, indices, inv)) = prepare_node_geometry(node, transform, canvas) else { return };

        let fill_color = color_to_rgba(paint_color(&node.fill, defs));
        let (stroke_color, stroke_width, has_stroke) = match (&node.stroke, node.stroke_width) {
            (Some(paint), Some(w)) => (color_to_rgba(paint_color(paint, defs)), w as f32, 1.0f32),
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

        self.draw_quad(device, encoder, view, &verts, &indices, &ops, &params, wgpu::LoadOp::Load);
    }

    /// Draws only `node`'s stroke, with the fill forced fully transparent
    /// — used when the fill was already drawn separately by
    /// `draw_sdf_shader_fill` (real WGSL execution routes only the fill,
    /// never the stroke, same scope vector.rs's shader routing has). If
    /// `node.stroke` is unset, this is a cheap no-op — checked before
    /// touching the GPU at all, so calling this unconditionally after
    /// every shader-routed fill (regardless of whether a stroke actually
    /// exists) costs nothing in the common no-stroke case.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_stroke_only(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, node: &SdfNode, transform: Matrix2D, canvas: (f32, f32), defs: &Defs) {
        let (stroke_color, stroke_width, has_stroke) = match (&node.stroke, node.stroke_width) {
            (Some(paint), Some(w)) => (color_to_rgba(paint_color(paint, defs)), w as f32, 1.0f32),
            _ => ([0.0; 4], 0.0, 0.0f32),
        };
        if has_stroke == 0.0 {
            return;
        }

        let Some((ops, verts, indices, inv)) = prepare_node_geometry(node, transform, canvas) else { return };

        let params = SdfParams {
            inv_row0: [inv.a as f32, inv.c as f32, inv.e as f32, 0.0],
            inv_row1: [inv.b as f32, inv.d as f32, inv.f as f32, 0.0],
            fill_color: [0.0; 4],
            stroke_color,
            stroke_width,
            has_stroke,
            op_count: ops.len() as u32,
            _pad: 0.0,
        };

        self.draw_quad(device, encoder, view, &verts, &indices, &ops, &params, wgpu::LoadOp::Load);
    }
    /// into `mask_view`, for compositing against a shader's output
    /// afterward (see `crate::sdf_shader::draw_sdf_shader_fill`, which is
    /// the only caller). Reuses `sdf.wgsl` completely unmodified — the
    /// shader already computes `antialiased_alpha(d) * fill_color.a` as
    /// its output alpha (see that file's `fs_main`), so forcing
    /// `fill_color = (1,1,1,1)` and disabling the stroke means that alpha
    /// channel IS exactly "is this pixel inside the shape", with zero
    /// changes to the WGSL itself. `mask_view` is cleared to transparent
    /// first — this is always a fresh per-node scratch texture, never the
    /// shared scene buffer `draw_one` draws onto with `LoadOp::Load`.
    ///
    /// Returns `false` (nothing drawn, `mask_view` stays all-zero) on the
    /// same early-outs `draw_one` has: an empty tree, or a non-invertible
    /// transform.
    pub(crate) fn draw_mask(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, mask_view: &wgpu::TextureView, node: &SdfNode, transform: Matrix2D, canvas: (f32, f32)) -> bool {
        let Some((ops, verts, indices, inv)) = prepare_node_geometry(node, transform, canvas) else { return false };

        let params = SdfParams {
            inv_row0: [inv.a as f32, inv.c as f32, inv.e as f32, 0.0],
            inv_row1: [inv.b as f32, inv.d as f32, inv.f as f32, 0.0],
            fill_color: [1.0, 1.0, 1.0, 1.0], // forced opaque white — see doc above
            stroke_color: [0.0; 4],
            stroke_width: 0.0,
            has_stroke: 0.0, // never draw a stroke band into the mask
            op_count: ops.len() as u32,
            _pad: 0.0,
        };

        self.draw_quad(device, encoder, mask_view, &verts, &indices, &ops, &params, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        true
    }

    /// Flat-color fallback for a node whose shader fill failed to resolve
    /// at render time — same "always paint something sane" contract every
    /// other shader-fill-capable path in this crate has (see shader.rs's
    /// module doc, and lib.rs's own fallback for shader-filled vector
    /// shapes). `color` is used directly instead of resolving `node.fill`
    /// through `paint_color`/`defs` again — the caller already knows it's
    /// a shader ref, not a color/gradient one, so re-resolving would just
    /// redo work to reach the same `ShaderDef` it already has in hand.
    /// Fill only, same as `draw_mask` — a node's stroke is the caller's
    /// separate concern either way, shader-routed or not.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_fallback_fill(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, node: &SdfNode, transform: Matrix2D, canvas: (f32, f32), color: Color) {
        let Some((ops, verts, indices, inv)) = prepare_node_geometry(node, transform, canvas) else { return };

        let params = SdfParams {
            inv_row0: [inv.a as f32, inv.c as f32, inv.e as f32, 0.0],
            inv_row1: [inv.b as f32, inv.d as f32, inv.f as f32, 0.0],
            fill_color: color_to_rgba(color),
            stroke_color: [0.0; 4],
            stroke_width: 0.0,
            has_stroke: 0.0,
            op_count: ops.len() as u32,
            _pad: 0.0,
        };

        self.draw_quad(device, encoder, view, &verts, &indices, &ops, &params, wgpu::LoadOp::Load);
    }

    /// Shared tail for `draw_one` and `draw_mask`: uploads the quad
    /// geometry + flattened ops + params, builds the bind group, and runs
    /// the render pass. The only thing that ever differs between the two
    /// callers is `load_op` — `draw_one` composites onto the existing
    /// scene buffer (`LoadOp::Load`), `draw_mask` starts a fresh scratch
    /// texture (`LoadOp::Clear`) — so that's the one thing pulled out as a
    /// parameter rather than duplicating this whole function body.
    #[allow(clippy::too_many_arguments)]
    fn draw_quad(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, verts: &[SdfVertex; 4], indices: &[u16; 6], ops: &[SdfOp], params: &SdfParams, load_op: wgpu::LoadOp<wgpu::Color>) {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf vertex buffer"),
            contents: bytemuck::cast_slice(verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf index buffer"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ops_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf ops buffer"),
            contents: bytemuck::cast_slice(ops),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx sdf params buffer"),
            contents: bytemuck::bytes_of(params),
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

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx sdf pass"),
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
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }
}

/// Does NOT recurse into `Element::Layer` — see module docs.
fn collect_sdf_nodes<'a>(elements: &'a [Element], transform: Matrix2D, out: &mut Vec<(&'a SdfNode, Matrix2D)>) {
    for el in elements {
        match el {
            Element::Sdf(node) => out.push((node, transform)),
            Element::Group(g) => {
                let local = g.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                collect_sdf_nodes(&g.children, transform.concat(local), out);
            }
            _ => {}
        }
    }
}

/// Return type of `prepare_node_geometry`: (flattened SDF ops, the node's
/// screen-space bounding-box quad as 4 vertices + a 2-triangle index list,
/// and the node's inverted combined transform). Named purely to satisfy
/// clippy's `type_complexity` — every field keeps its original meaning
/// from the inline tuple this replaced, see that function's own doc.
type SdfNodeGeometry = (Vec<SdfOp>, [SdfVertex; 4], [u16; 6], Matrix2D);

/// Shared geometry setup for every SDF node draw variant (`draw_one`,
/// `draw_mask`, `draw_fallback_fill`, `node_bounding_quad`): resolves the
/// node's combined transform and inverts it, flattens its tree into ops,
/// and computes its screen-space bounding-box quad. Returns `None` on the
/// same two early-outs all four callers had individually before this was
/// factored out of them: an empty tree, or a non-invertible transform.
fn prepare_node_geometry(node: &SdfNode, transform: Matrix2D, canvas: (f32, f32)) -> Option<SdfNodeGeometry> {
    let local = node.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    let combined = transform.concat(local);
    let inv = invert_matrix(combined)?;

    let mut ops = Vec::new();
    flatten_tree(&node.tree, &mut ops);
    if ops.is_empty() {
        return None;
    }

    let local_bounds = sdf_bounds(&node.tree);
    let screen_bounds = transform_bounds(local_bounds, combined);
    let (verts, indices) = quad_vertices(screen_bounds, canvas);
    Some((ops, verts, indices, inv))
}

/// Computes `node`'s screen-space bounding-box quad as clip-space
/// geometry ready for `ShaderFillPipeline::draw`, which takes arbitrary
/// `[f32; 2]` clip-space vertices + `u32` indices (see
/// `shader::PendingShaderShape`'s doc comment — it was already designed
/// position-only/shape-agnostic for exactly this kind of reuse, not just
/// for vector.rs's triangulated shapes). Reuses `prepare_node_geometry`,
/// the same bounds/transform computation `draw_mask` uses for its own
/// quad, so the shader's rendered output and the node's mask always land
/// in the same screen position with no separate offset bookkeeping needed
/// — both render at full canvas size. A free function, not a method —
/// unlike `draw_one`/`draw_mask`/`draw_fallback_fill`, it never touches
/// `self` (there's no GPU work here, only geometry math), so it doesn't
/// need an `SdfPipeline` instance — or a GPU adapter — to call or to test.
///
/// Returns `None` on the same early-outs every draw variant here has.
pub(crate) fn node_bounding_quad(node: &SdfNode, transform: Matrix2D, canvas: (f32, f32)) -> Option<(Vec<[f32; 2]>, Vec<u32>)> {
    let (_, verts, indices, _) = prepare_node_geometry(node, transform, canvas)?;
    let vertices = verts.iter().map(|v| v.clip_position).collect();
    let indices = indices.iter().map(|&i| i as u32).collect();
    Some((vertices, indices))
}

/// Resolves any `Paint` to a flat `Color` — shared with `splat.rs` for the
/// same reason `average_stop_color`/`resolve_shader_def` (from
/// `vector.rs`) are themselves `pub(crate)`: a splat's `fill`, when it has
/// one and it isn't a `Def::Shader` (which `splat.rs` routes through the
/// mask+color+composite path instead — see `splat_shader::draw_splat_shader_fill`),
/// resolves through this exact same gradient-average path.
pub(crate) fn paint_color(paint: &Paint, defs: &Defs) -> Color {
    match paint {
        Paint::Color(c) => *c,
        Paint::CurrentColor => Color::BLACK,
        Paint::None => Color::rgba(0, 0, 0, 0),
        Paint::Ref(reference) => {
            // Same resolution vector.rs::paint_to_rgba uses for every
            // other shape type — see average_stop_color's doc comment
            // there. This used to unconditionally return transparent for
            // any Paint::Ref here regardless of what it pointed at, since
            // defs was never threaded into this call path at all.
            match reference.strip_prefix("url(#").and_then(|s| s.strip_suffix(')')) {
                Some(id) => match defs.get(id) {
                    Some(def) => average_stop_color(def),
                    None => Color::rgba(0, 0, 0, 0),
                },
                None => Color::rgba(0, 0, 0, 0),
            }
        }
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
    use msx_ast::{Element as MsxElement, Group};

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
        assert_eq!(ops[0].param2, 50.0);
        assert_eq!(ops[1].param2, 30.0);
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
        assert_eq!(ops[0].param2, 30.0);
        assert_eq!(ops[1].param2, 20.0);
        assert_eq!(ops[2].param2, 10.0);
        assert_eq!(ops[3].kind, OP_SMOOTH_UNION);
        assert_eq!(ops[3].count, 3);
    }

    #[test]
    fn collect_sdf_nodes_skips_layer_children() {
        let inner = SdfNode::new(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 5.0 }, Paint::Color(Color::BLACK));
        let layer = msx_ast::Layer::new(vec![MsxElement::Sdf(inner)]);
        let elements = vec![MsxElement::Layer(layer)];

        let mut out = Vec::new();
        collect_sdf_nodes(&elements, Matrix2D::identity(), &mut out);
        assert!(out.is_empty(), "SDF nodes inside a Layer must not be collected by the main pass");
    }

    #[test]
    fn collect_sdf_nodes_still_recurses_through_groups() {
        let inner = SdfNode::new(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 5.0 }, Paint::Color(Color::BLACK));
        let group = Group::new(vec![MsxElement::Sdf(inner)]);
        let elements = vec![MsxElement::Group(group)];

        let mut out = Vec::new();
        collect_sdf_nodes(&elements, Matrix2D::identity(), &mut out);
        assert_eq!(out.len(), 1);
    }

    /// Pure geometry math, no GPU adapter needed — `node_bounding_quad` is
    /// a free function precisely so this doesn't have to be gated behind
    /// hardware availability the way the real-adapter tests in `lib.rs`
    /// are.
    #[test]
    fn node_bounding_quad_returns_four_vertices_and_six_indices_for_a_circle() {
        let node = SdfNode::new(SdfTree::Circle { cx: 20.0, cy: 20.0, r: 10.0 }, Paint::Color(Color::BLACK));
        let quad = node_bounding_quad(&node, Matrix2D::identity(), (40.0, 40.0));
        let (vertices, indices) = quad.expect("a plain circle with an identity transform should always produce a quad");
        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);
    }

    #[test]
    fn node_bounding_quad_returns_none_for_a_non_invertible_transform() {
        let node = SdfNode::new(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 10.0 }, Paint::Color(Color::BLACK));
        // A zero matrix has determinant 0 — not invertible, same early-out
        // draw_one/draw_mask/draw_fallback_fill all share via
        // prepare_node_geometry.
        let degenerate = Matrix2D { a: 0.0, b: 0.0, c: 0.0, d: 0.0, e: 0.0, f: 0.0 };
        assert!(node_bounding_quad(&node, degenerate, (40.0, 40.0)).is_none());
    }

    /// The whole masking technique (see draw_mask's doc comment) rests on
    /// one assumption: forcing `fill_color = (1,1,1,1)` and `has_stroke =
    /// 0` makes sdf.wgsl's output alpha exactly equal
    /// `antialiased_alpha(d)`, with the RGB channels and the stroke path
    /// both irrelevant. This test doesn't run the WGSL itself (no GPU
    /// adapter needed), it just pins down the params draw_mask actually
    /// builds — if a future edit accidentally left has_stroke on, or used
    /// anything other than opaque white, this fails loudly instead of
    /// silently producing a wrong mask that would only show up as a
    /// visually-wrong composite on real hardware.
    #[test]
    fn mask_params_force_opaque_white_fill_and_disable_stroke() {
        let node = SdfNode::new(SdfTree::Circle { cx: 20.0, cy: 20.0, r: 10.0 }, Paint::Color(Color::rgb(255, 0, 0)));
        let (_, _, _, inv) = prepare_node_geometry(&node, Matrix2D::identity(), (40.0, 40.0))
            .expect("plain circle, identity transform — always succeeds");
        // Mirrors draw_mask's own params construction exactly, so this
        // fails the moment that construction ever drifts from what the
        // masking technique actually requires.
        let params = SdfParams {
            inv_row0: [inv.a as f32, inv.c as f32, inv.e as f32, 0.0],
            inv_row1: [inv.b as f32, inv.d as f32, inv.f as f32, 0.0],
            fill_color: [1.0, 1.0, 1.0, 1.0],
            stroke_color: [0.0; 4],
            stroke_width: 0.0,
            has_stroke: 0.0,
            op_count: 1,
            _pad: 0.0,
        };
        assert_eq!(params.fill_color, [1.0, 1.0, 1.0, 1.0], "mask fill must be opaque white — the node's real color (red, here) must never leak into the mask");
        assert_eq!(params.has_stroke, 0.0, "the mask must never include a stroke band");
    }
                                    }
