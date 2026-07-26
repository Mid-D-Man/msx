// render/msx-render-gpu/src/shader.rs
//! Real WGSL execution for `Def::Shader` fills — the path every other
//! renderer (and this one, until now) deliberately deferred, painting
//! shader-filled shapes flat with `fallback_color` instead. See
//! `ShaderDef`'s doc comment in `core/msx-ast/src/gradient.rs` for the
//! full history and the FUTURE(raw-section) migration note.
//!
//! ## The contract a shader-def's `.wgsl` file must follow
//!
//! `source_ref` points at a **fragment-only** WGSL file: no vertex stage,
//! no `struct VertexOutput` of its own — just a `@group(0) @binding(0)
//! var<uniform> u: Uniforms;` declaration and an `entry_point` fragment
//! function receiving at minimum `@builtin(position)`. The vertex stage
//! is supplied by this crate (`shaders/shader_fill_vertex.wgsl`, shared
//! across every shader-def) and compiled as a **separate** `ShaderModule`
//! — wgpu doesn't require a render pipeline's vertex and fragment stages
//! to come from the same module, so there's no source-text splicing.
//! `examples/shaders/plasma.wgsl` is the reference example this was
//! built and hand-checked against.
//!
//! ## Uniform buffer layout — a convention, not something parsed
//!
//! This crate does not parse the user's WGSL (no `naga`, no WGSL parser
//! anywhere in this dependency tree), so it cannot discover the user's
//! `Uniforms` struct layout automatically. Instead, exactly like
//! `SdfParams`/`SplatInstance`/`CanvasParams` elsewhere in this crate,
//! the layout is a **documented convention** the shader author must
//! follow by hand:
//!
//! 1. One field per entry in the def's own `uniforms` list, **in
//!    declared order**, using the matching WGSL type (`float` -> `f32`,
//!    `vec2` -> `vec2<f32>`, etc.).
//! 2. Exactly one trailing `time: f32` field, appended automatically by
//!    this renderer — every shader gets a free running clock, same
//!    convention as Shadertoy/ISF, and it is **not** part of the def's
//!    own declared `uniforms`.
//!
//! `pack_uniforms` below lays these out using WGSL's own uniform
//! address-space alignment rules (scalar align 4, `vec2` align 8, `vec3`
//! /`vec4` align 16, struct size rounded up to the largest member's
//! alignment) — the exact same algorithm naga itself would apply to the
//! user's hand-written struct, so a correctly-ordered/-typed `Uniforms`
//! struct lines up byte-for-byte with zero manual padding math required
//! from the shader author. Verified by hand against `plasma.wgsl`'s own
//! `speed: f32, resolution: vec2<f32>, time: f32` (offsets 0, 8, 16;
//! total size 24) before writing this doc comment.
//!
//! ## Known gaps, flagged rather than hidden
//!
//! - **Fill only.** A shader referenced from a `stroke` still falls back
//!   to `fallback_color` — stroking a shader-filled outline needs the
//!   same treatment applied to `stroke_path`'s output, not attempted
//!   here yet.
//! - **Shapes inside a `Layer` now route through here too.** `layer.rs`
//!   calls `vector::tessellate_elements_with_shaders` (with the real
//!   scene `defs`, not an empty one) instead of the plain
//!   `tessellate_elements` it used to — a shader fill on a shape inside a
//!   `Layer` executes for real now, same as a top-level shape. See
//!   `layer.rs`'s module doc for the details.
//! - **Opacity isn't applied.** The shape's `style.opacity` is not
//!   multiplied into the shader's output alpha — the shader's returned
//!   `vec4` is used as-is. Doing this properly needs either a uniform
//!   the renderer injects unconditionally (changing the layout contract
//!   above) or a wrapping composite pass; deferred rather than bolted on
//!   half-correctly.
//! - **Malformed WGSL isn't caught gracefully.** A `source_ref` that
//!   doesn't resolve to a readable file falls back to `fallback_color`
//!   cleanly (checked via `std::fs::read_to_string`, an ordinary
//!   `Result`). A file that exists but contains *invalid* WGSL does not
//!   — real wgpu reports shader-compilation errors asynchronously via
//!   the device's uncaptured-error callback / error-scope mechanism
//!   (`create_shader_module` itself always returns a `ShaderModule`
//!   synchronously, valid or not), and wiring that up needs
//!   `device.push_error_scope`/`pop_error_scope` around an async point
//!   this crate's otherwise-synchronous render path doesn't have yet.
//!   `msx-cli compile`'s `validate_shader_refs` already catches the far
//!   more common failure (typo'd/moved path); catching bad WGSL syntax
//!   gracefully at render time is a deliberate follow-up, not an
//!   oversight.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use msx_ast::{ShaderDef, ShaderUniformValue};

/// One tessellated shape whose *fill* resolved to a `Def::Shader`,
/// collected by `vector::tessellate_scene_with_shaders` instead of being
/// baked flat into the shared batch. Position-only (clip-space xy,
/// already transformed CPU-side) — color comes entirely from the user's
/// fragment shader, there's nothing to bake per-vertex here.
pub(crate) struct PendingShaderShape {
    pub shader: ShaderDef,
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Cache key: a shader-def's pipeline depends only on its resolved
/// source file and entry point — two defs pointing at the same file/
/// entry point (or the same def referenced by multiple elements) share
/// one compiled pipeline rather than rebuilding per shape per frame.
type PipelineKey = (PathBuf, String);

pub(crate) struct ShaderFillPipeline {
    vertex_module: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    // `Renderer::render` takes `&self` (msx-render-core's shared trait,
    // used identically by the CPU/SVG backends too — not something this
    // one crate can unilaterally change), but a pipeline cache is
    // inherently a mutating structure. `RefCell` gets `&self`-compatible
    // interior mutability without touching that shared trait; nothing in
    // this project runs renderers across threads today; a real
    // `Mutex`-based cache is the natural upgrade if that ever changes.
    cache: RefCell<HashMap<PipelineKey, wgpu::RenderPipeline>>,
}

impl ShaderFillPipeline {
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let vertex_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx shader-fill vertex module"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shader_fill_vertex.wgsl").into()),
        });

        // Every user shader gets the identical binding shape: one uniform
        // buffer at binding 0, visible to the fragment stage only.
        // `min_binding_size: None` accepts whatever byte length that
        // particular def's packed uniforms come out to, so this one
        // layout (and the pipeline layout built from it) is shared across
        // every shader-def rather than rebuilt per def.
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx shader-fill bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx shader-fill pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        ShaderFillPipeline { vertex_module, bind_group_layout, pipeline_layout, target_format, cache: RefCell::new(HashMap::new()) }
    }

    /// Draws one shader-filled shape. Returns `Err` (never panics) if
    /// `source_ref` doesn't resolve to a readable file — the caller is
    /// expected to fall back to a flat `fallback_color` draw in that
    /// case, exactly like every other renderer already does
    /// unconditionally today (see the module doc's "known gaps" for what
    /// this does *not* catch: invalid-but-readable WGSL).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        base_dir: &Path,
        shape: &PendingShaderShape,
        time: f32,
    ) -> Result<(), String> {
        if shape.indices.is_empty() {
            return Ok(());
        }

        let resolved_path = base_dir.join(&shape.shader.source_ref);
        let key: PipelineKey = (resolved_path.clone(), shape.shader.entry_point.clone());

        if !self.cache.borrow().contains_key(&key) {
            let source = std::fs::read_to_string(&resolved_path).map_err(|e| {
                format!("shader def '{}': failed to read source_ref '{}' (resolved to {}): {}", shape.shader.id, shape.shader.source_ref, resolved_path.display(), e)
            })?;
            let pipeline = self.build_pipeline(device, &source, &shape.shader.entry_point);
            self.cache.borrow_mut().insert(key.clone(), pipeline);
        }
        let cache = self.cache.borrow();
        let pipeline = cache.get(&key).expect("just inserted above");

        let uniform_bytes = pack_uniforms(&shape.shader, time);
        let uniform_buffer = {
            use wgpu::util::DeviceExt;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx shader-fill uniform buffer"),
                contents: &uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };
        let vertex_buffer = {
            use wgpu::util::DeviceExt;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx shader-fill vertex buffer"),
                contents: bytemuck::cast_slice(&shape.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            })
        };
        let index_buffer = {
            use wgpu::util::DeviceExt;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("msx shader-fill index buffer"),
                contents: bytemuck::cast_slice(&shape.indices),
                usage: wgpu::BufferUsages::INDEX,
            })
        };

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx shader-fill bind group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() }],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msx shader-fill pass"),
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
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..shape.indices.len() as u32, 0, 0..1);
        drop(pass);

        Ok(())
    }

    fn build_pipeline(&self, device: &wgpu::Device, fragment_source: &str, entry_point: &str) -> wgpu::RenderPipeline {
        let fragment_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx shader-fill user fragment module"),
            source: wgpu::ShaderSource::Wgsl(fragment_source.to_owned().into()),
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 }],
        };

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx shader-fill pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex_module,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment_module,
                entry_point: Some(entry_point),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.target_format,
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
        })
    }
}

/// Packs a shader-def's declared uniforms plus a trailing `time: f32`
/// into a byte buffer laid out per WGSL's own uniform address-space
/// alignment rules — see the module doc's "Uniform buffer layout"
/// section for the full contract and the worked example this was
/// checked against.
fn pack_uniforms(def: &ShaderDef, time: f32) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut max_align: usize = 4;

    fn push(bytes: &mut Vec<u8>, data: &[u8], align: usize, max_align: &mut usize) {
        *max_align = (*max_align).max(align);
        while !bytes.len().is_multiple_of(align) {
            bytes.push(0);
        }
        bytes.extend_from_slice(data);
    }

    for u in &def.uniforms {
        match u.value {
            ShaderUniformValue::Float(v) => push(&mut bytes, &v.to_le_bytes(), 4, &mut max_align),
            ShaderUniformValue::Vec2(x, y) => {
                let mut data = [0u8; 8];
                data[0..4].copy_from_slice(&x.to_le_bytes());
                data[4..8].copy_from_slice(&y.to_le_bytes());
                push(&mut bytes, &data, 8, &mut max_align);
            }
            ShaderUniformValue::Vec3(x, y, z) => {
                let mut data = [0u8; 12];
                data[0..4].copy_from_slice(&x.to_le_bytes());
                data[4..8].copy_from_slice(&y.to_le_bytes());
                data[8..12].copy_from_slice(&z.to_le_bytes());
                push(&mut bytes, &data, 16, &mut max_align); // align 16, size 12 — WGSL vec3 rule
            }
            ShaderUniformValue::Vec4(x, y, z, w) => {
                let mut data = [0u8; 16];
                data[0..4].copy_from_slice(&x.to_le_bytes());
                data[4..8].copy_from_slice(&y.to_le_bytes());
                data[8..12].copy_from_slice(&z.to_le_bytes());
                data[12..16].copy_from_slice(&w.to_le_bytes());
                push(&mut bytes, &data, 16, &mut max_align);
            }
        }
    }

    // The renderer's own free-running clock, always last, always f32.
    push(&mut bytes, &time.to_le_bytes(), 4, &mut max_align);

    // WGSL struct size is rounded up to the struct's own alignment,
    // which equals the largest member alignment for a flat struct like
    // this (no nested struct/array members here to complicate it).
    while !bytes.len().is_multiple_of(max_align) {
        bytes.push(0);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Color, ShaderUniform};

    fn plasma_def() -> ShaderDef {
        ShaderDef::new("plasma_1", "shaders/plasma.wgsl", Color::rgb(0x6b, 0x46, 0xff))
            .with_entry_point("fs_main")
            .with_uniforms(vec![
                ShaderUniform::new("speed", ShaderUniformValue::Float(1.5)),
                ShaderUniform::new("resolution", ShaderUniformValue::Vec2(300.0, 200.0)),
            ])
    }

    /// Byte-for-byte against `plasma.wgsl`'s real
    /// `struct Uniforms { speed: f32, resolution: vec2<f32>, time: f32 }`
    /// laid out by hand per the WGSL alignment rules: speed at 0 (size 4),
    /// resolution at 8 (align 8, so 4 bytes of padding after speed),
    /// time at 16 (already 4-aligned, no padding needed), struct rounded
    /// up to the max member alignment (8) — total 24 bytes.
    #[test]
    fn pack_uniforms_matches_plasma_wgsl_layout_by_hand() {
        let bytes = pack_uniforms(&plasma_def(), 7.25);
        assert_eq!(bytes.len(), 24);

        let speed = f32::from_le_bytes(bytes[0..4].try_into().unwrap());
        assert_eq!(speed, 1.5);

        let res_x = f32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let res_y = f32::from_le_bytes(bytes[12..16].try_into().unwrap());
        assert_eq!((res_x, res_y), (300.0, 200.0));

        let time = f32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(time, 7.25);

        // Bytes 4..8 (padding before the vec2) and 20..24 (trailing
        // struct-size padding) must be zero, not garbage.
        assert_eq!(&bytes[4..8], &[0, 0, 0, 0]);
        assert_eq!(&bytes[20..24], &[0, 0, 0, 0]);
    }

    #[test]
    fn pack_uniforms_with_no_declared_uniforms_is_just_time() {
        let def = ShaderDef::new("id", "x.wgsl", Color::BLACK);
        let bytes = pack_uniforms(&def, 3.0);
        assert_eq!(bytes.len(), 4);
        assert_eq!(f32::from_le_bytes(bytes[0..4].try_into().unwrap()), 3.0);
    }

    #[test]
    fn pack_uniforms_vec3_then_scalar_does_not_over_pad() {
        // vec3 (align 16, size 12) followed by a scalar (align 4): the
        // scalar can start right after the vec3's 12 bytes (offset 12 is
        // already 4-aligned) — this must NOT round the vec3 up to a full
        // 16 bytes the way some std140 implementations naively do.
        let def = ShaderDef::new("id", "x.wgsl", Color::BLACK)
            .with_uniforms(vec![ShaderUniform::new("a", ShaderUniformValue::Vec3(1.0, 2.0, 3.0))]);
        let bytes = pack_uniforms(&def, 9.0);
        // vec3 at 0..12, time at 12..16 (no padding), struct rounds up to
        // max_align (16) -> 16 bytes total.
        assert_eq!(bytes.len(), 16);
        let time = f32::from_le_bytes(bytes[12..16].try_into().unwrap());
        assert_eq!(time, 9.0);
    }
                                                            }
