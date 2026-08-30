// render/msx-render-gpu/src/layer.rs
//! `Layer` rendering: children render into an isolated offscreen texture
//! (cleared to transparent, not the canvas background), then that buffer
//! composites onto the parent target at the layer's opacity.
//!
//! ## Known gaps, flagged rather than hidden
//!
//! **`effects` (blur, drop shadow, glow) aren't applied on this path.**
//! `msx-render-cpu` already has all of these; the GPU path doesn't have
//! texture-sampling/blur infrastructure built yet.
//!
//! ## Blend modes
//!
//! `Normal` takes a separate, unchanged fixed-function fast path
//! (`composite`, `PREMULTIPLIED_ALPHA_BLENDING` — no shader-side blend
//! math at all) precisely because it was already proven correct by this
//! file's existing tests before any of this existed; reusing it
//! byte-for-byte rather than routing Normal through the general formula
//! too means that proof still covers exactly the code Normal actually
//! runs.
//!
//! Every other mode (`Multiply`/`Screen`/`Overlay`/etc.) goes through
//! `composite_blended`: a render pass can't sample the texture it's
//! currently writing to (no framebuffer-fetch in WebGPU/wgpu), so
//! `dst`'s current contents are copied into a separate "backdrop"
//! texture first (`copy_texture_to_texture` — needs a real `Texture`
//! handle on both ends, not just a `TextureView`, which is why
//! `render_ordered`/`render_ops`/`render_layer` all take
//! `&OffscreenTarget` rather than `&wgpu::TextureView` now), then
//! `shaders/backdrop_blend.wgsl` samples that backdrop copy AND the
//! layer buffer together and writes the fully W3C-blended pixel
//! straight into `dst`, with `blend: None` on that pipeline so nothing
//! composites it a second time. See that shader's own module doc for
//! the exact formula and its derivation from
//! `msx-render-core::blend::composite` (already real-adapter-independent
//! and unit-tested there), and `layer.rs`'s own `wgsl_blend_port_matches_real_rust_reference`
//! /`wgsl_full_composite_formula_matches_reference` tests for a
//! GPU-adapter-independent numerical cross-check of that derivation
//! against the same reference — the two `..._if_a_gpu_adapter_is_available`
//! tests in `lib.rs` are the only pieces of this feature that actually
//! exercise the real wgpu pipeline/bind-group/texture-copy plumbing,
//! and (like everything else marked that way in this crate) can only
//! ever be confirmed by real CI, not local sandbox verification — `wgpu`
//! itself needs a newer rustc than this project's local/sandbox floor
//! provides.
//!
//! **Layer paint order is sibling-scoped, at every nesting level,
//! including nesting itself.** A `Layer` only ever trades paint-order
//! places with another `Layer` in the SAME sibling list (`paint_order`,
//! below, calls the exact same `msx_ast::layer::layer_reordered`
//! `msx-render-svg`/`msx-render-cpu` use — not a separate
//! reimplementation), and a non-`Layer` sibling's position relative to
//! any `Layer` around it is always respected. This replaced the old
//! behavior (every `Layer` anywhere composited after every non-`Layer`
//! element, unconditionally, sorted globally against every other `Layer`
//! in the whole scene regardless of nesting depth).
//!
//! The decided nesting model (matching what `msx-render-svg`/
//! `msx-render-cpu` already do by construction, via plain recursive
//! dispatch — see those crates' `render_layer`/`render_element`): a
//! nested `Layer`'s `z_index` is resolved purely against its own
//! immediate siblings, at its own nesting level, BEFORE the level
//! containing it is resolved — never against a `Layer` at a different
//! depth. Opacity composes the same way: a nested `Layer` composites
//! at its own opacity into its parent's buffer first, and the parent
//! then composites that already-attenuated result at ITS OWN opacity —
//! so visual opacity across N nesting levels is the product of all N
//! opacities, purely as an emergent property of resolving each level
//! before the one above it, never a separate multiply this crate
//! computes directly. Two cases previously fell outside this:
//!
//! - **A `Layer` nested inside a `Group`** is now given the exact same
//!   local, sibling-scoped treatment as one nested inside a `Layer`:
//!   `paint_order` (below) pulls any `Group` that transitively contains
//!   a `Layer` out of its surrounding `Run` and recurses into it with
//!   its own fresh `paint_order` call scoped to `group.children` (see
//!   `PaintOp::GroupSplit` and `render_ops`), drawn inline at exactly
//!   the Group's real position in its parent's document order — not
//!   collected into one global bucket and composited last, which is
//!   what this crate did before. A `Group` that contains no `Layer`
//!   anywhere in its subtree is untouched by any of this and still goes
//!   through `vector.rs`/`sdf.rs`/`splat.rs`'s own fast internal
//!   Group-recursion as one ordinary tessellated batch, exactly as
//!   before — only a Group that actually needs a composite pass
//!   somewhere inside it pays for this extra recursion.
//! - **A `Layer` nested inside another `Layer`** now renders for real:
//!   `render_layer`'s own call into `render_ops` (below) is no longer
//!   restricted, so a `Composite` op found while walking a `Layer`'s
//!   own children performs a genuine recursive `render_layer` call at
//!   that exact point, instead of being silently skipped.
//!
//! Ordering WITHIN one contiguous non-`Layer` run — vector shapes vs SDF
//! nodes vs splats vs each other — is unaffected by any of this and
//! remains type-batched (all vectors, then all shader fills, then all
//! SDF, then all splats), not true per-element document order. That's a
//! separate, pre-existing gap from this crate's very first version,
//! independent of `Layer`/`z_index` entirely, deliberately not folded
//! into this fix.
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
//!
//! ## The layer buffer is premultiplied alpha
//!
//! `render_layer`'s offscreen `buffer` starts fully transparent and every
//! child draw into it blends translucently — which unavoidably leaves it
//! holding `rgb = true_color * true_alpha`, the same result "premultiplied
//! alpha" storage would produce on purpose, regardless of which blend
//! state any individual child draw used. `LayerCompositor::composite`
//! (below) and `composite.wgsl` treat it accordingly
//! (`PREMULTIPLIED_ALPHA_BLENDING`, and scaling `rgb` by `opacity` too,
//! not just `a`) — see `composite`'s own doc comment and `composite.wgsl`
//! for the full reasoning and the real (now-fixed) darkening bug this
//! used to cause for any partial-alpha pixel.

use wgpu::util::DeviceExt;

use msx_ast::{Def, Element, Group, Layer, Matrix2D};

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

// Deliberately `_pad: [f32; 2]` (WGSL `vec2<f32>`, alignment 8), not a
// 3-wide pad — a `vec3<f32>` field in WGSL needs its OWN field to start
// 16-byte-aligned, which is exactly the trap `CompositeParams` above
// already fell into once. Laid out by hand against WGSL's struct-layout
// rules (offset = round_up(current_offset, member_align)):
//   opacity (f32,  align 4, size 4)  -> offset 0
//   blend_mode (u32, align 4, size 4) -> offset 4  (already 4-aligned)
//   _pad (vec2<f32>, align 8, size 8) -> offset 8  (already 8-aligned —
//                                         no gap needed before it)
//   struct align = max(4,4,8) = 8; size = round_up(16, 8) = 16
// A plain Rust `#[repr(C)]` struct with the same three fields in the
// same order produces that identical 16-byte/offset-8 layout on its
// own (unlike `[f32; 3]`, a `[f32; 2]` has alignment 4, and 4 already
// divides the offset-8 starting point evenly) — so, unlike
// `CompositeParams`, no explicit extra padding arithmetic is needed to
// make the two sides agree.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlendParams {
    opacity: f32,
    blend_mode: u32,
    _pad: [f32; 2],
}

pub struct LayerCompositor {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    // Non-Normal blend modes (`composite_blended`) — separate pipeline
    // and layout from the Normal-only fast path above, since the bind
    // group shapes genuinely differ (two sampled textures plus a
    // `blend_mode` field, vs. one texture) — see this struct's `new`
    // and `composite_blended` for why the two paths stay independent
    // rather than sharing one bind group layout sized for the larger
    // case.
    blend_pipeline: wgpu::RenderPipeline,
    blend_bind_group_layout: wgpu::BindGroupLayout,
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
                    // PREMULTIPLIED, not plain `ALPHA_BLENDING` — the
                    // layer buffer this pipeline reads from is itself the
                    // product of blending translucent draws onto a
                    // transparent-cleared texture, which always leaves it
                    // holding `rgb = true_color * true_alpha` regardless
                    // of what any individual child draw's own blend state
                    // was. `ALPHA_BLENDING` (non-premultiplied) would
                    // multiply that already-premultiplied rgb by alpha a
                    // SECOND time here — a real bug that shipped
                    // undetected because every test before
                    // `layer_shader_fill_executes_for_a_splat_inside_a_layer`
                    // happened to sample a pixel with source alpha exactly
                    // 1.0, where premultiplied and non-premultiplied
                    // reads are numerically identical and the bug is
                    // invisible. A Gaussian splat's naturally-partial
                    // coverage (alpha < 1.0 almost everywhere, even at
                    // its rendered "center" — see that test's own doc
                    // comment) was the first thing to actually exercise
                    // this path with alpha < 1.0 in the buffer, and
                    // caught it as a visibly wrong (too dark) pixel.
                    // `composite.wgsl`'s `fs_main` is the other half of
                    // this fix — it scales `sample.rgb` by `opacity` too,
                    // not just `sample.a`, so the premultiplied invariant
                    // survives the layer's own opacity attenuation before
                    // reaching this blend stage.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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

        // ── Non-Normal blend pipeline ──────────────────────────────────
        // See `composite_blended`'s own doc comment and
        // `shaders/backdrop_blend.wgsl` for the technique and the
        // formula it implements; this is just the pipeline plumbing.
        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx layer backdrop blend shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/backdrop_blend.wgsl").into()),
        });

        let blend_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx layer backdrop blend bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, // backdrop_texture
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, // source_texture
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, // tex_sampler
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3, // params
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

        let blend_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx layer backdrop blend pipeline layout"),
            bind_group_layouts: &[&blend_bind_group_layout],
            push_constant_ranges: &[],
        });

        let blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx layer backdrop blend pipeline"),
            layout: Some(&blend_layout),
            vertex: wgpu::VertexState {
                module: &blend_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blend_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    // `None`, not a `BlendState` — the fragment shader's
                    // own output IS the fully-composited pixel (source
                    // blended over the sampled backdrop, per the W3C
                    // formula in backdrop_blend.wgsl), so it must
                    // overwrite the destination outright. Blending it
                    // again here with a `BlendState` would mix an
                    // already-final color with the backdrop a SECOND
                    // time — the exact double-application bug class
                    // `composite.wgsl`'s own module doc already
                    // describes for a different field (premultiplied
                    // rgb/alpha), same underlying lesson: know which
                    // stage is responsible for compositing, and only let
                    // that one stage do it.
                    blend: None,
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

        LayerCompositor { pipeline, bind_group_layout, blend_pipeline, blend_bind_group_layout, sampler }
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
    ///
    /// `pub(crate)`, not `pub`: this takes `&ShaderFillPipeline` and
    /// `&MaskedShaderComposite`, both deliberately `pub(crate)` (internal
    /// pipeline plumbing, not part of this crate's public API — see their
    /// own doc comments) — a `pub fn` taking `pub(crate)` types is a
    /// private-interfaces error under `-D warnings`. `render_layer` has
    /// exactly one caller, `lib.rs`, in this same crate, so narrowing to
    /// match the types it already depends on (rather than widening those
    /// types' own deliberately-narrow visibility) is the correct fix, not
    /// a workaround.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_layer(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &OffscreenTarget,
        layer: &Layer,
        parent_transform: Matrix2D,
        canvas: (u32, u32),
        vector_pipeline: &VectorPipeline,
        sdf_pipeline: &SdfPipeline,
        splat_pipeline: &SplatPipeline,
        shader_pipeline: &ShaderFillPipeline,
        masked_shader_composite: &MaskedShaderComposite,
        image_pipeline: &crate::image::ImagePipeline,
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

        // `render_ordered` instead of a direct tessellate+draw of
        // `layer.children`: a Layer's own children get the exact same
        // sibling-scoped Layer-ordering treatment the top level does —
        // including a `Layer` nested directly inside THIS Layer, which
        // now renders for real (a `Composite` op found by `render_ops`
        // below is never suppressed) — so a
        // Layer-containing-Layer-and-Rect-siblings case is correct at
        // ANY nesting depth of `render_layer` calls, not just at the
        // very top. Its own opacity composites into `buffer` here first,
        // then `buffer` as a whole composites into `view` below at
        // `layer.opacity` — the two multiply together purely because
        // each level resolves before the one above it, not because
        // anything here computes a product directly. See
        // `render_ordered`'s own doc comment for the full reasoning.
        render_ordered(
            device,
            queue,
            &buffer,
            &layer.children,
            combined,
            canvas_f,
            wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            self,
            vector_pipeline,
            sdf_pipeline,
            splat_pipeline,
            shader_pipeline,
            masked_shader_composite,
            image_pipeline,
            &defs,
            scene_defs,
            shader_base_dir,
            time,
        );

        self.composite(device, queue, target, &buffer.view, layer.opacity as f32, layer.blend_mode);
    }

    /// Blends `src_view` (a fully-rendered layer buffer — see this
    /// module's "The layer buffer is premultiplied alpha" doc section)
    /// onto `dst`'s current contents at `opacity`, honoring `blend_mode`.
    /// Requires `src_view`'s contents to already be premultiplied alpha;
    /// this function does not itself premultiply anything.
    ///
    /// `BlendMode::Normal` takes the original, unchanged fast path
    /// (fixed-function `PREMULTIPLIED_ALPHA_BLENDING` — no shader-side
    /// blend math at all) precisely because it's already proven correct
    /// by the existing tests below, most pointedly the splat/premultiplied
    /// -alpha regression test — reusing it byte-for-byte rather than
    /// routing Normal through the new general formula too means that
    /// proof still covers exactly the code path Normal actually takes.
    /// Every other mode goes through `composite_blended`.
    fn composite(&self, device: &wgpu::Device, queue: &wgpu::Queue, dst: &OffscreenTarget, src_view: &wgpu::TextureView, opacity: f32, blend_mode: msx_ast::BlendMode) {
        if blend_mode != msx_ast::BlendMode::Normal {
            self.composite_blended(device, queue, dst, src_view, opacity, blend_mode);
            return;
        }

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
                    view: &dst.view,
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

    /// The non-Normal blend-mode path — see `shaders/backdrop_blend.wgsl`
    /// for the formula and its derivation from
    /// `msx-render-core::blend::composite`.
    ///
    /// A render pass can't sample the texture it's currently writing to
    /// (no framebuffer-fetch in WebGPU/wgpu), so `dst`'s CURRENT contents
    /// are copied into a fresh, separate "backdrop" texture first — that
    /// copy is exactly why this function needs `dst: &OffscreenTarget`
    /// rather than just a `TextureView` the way the Normal path above
    /// still does (`copy_texture_to_texture` needs real `Texture`
    /// handles on both ends, a view alone can't be a copy source). The
    /// blend shader then reads that backdrop copy AND `src_view`
    /// together and writes the final composited pixel straight into
    /// `dst.view`, with pipeline `blend: None` (see `new`'s own comment
    /// on that) so nothing composites it a second time.
    fn composite_blended(&self, device: &wgpu::Device, queue: &wgpu::Queue, dst: &OffscreenTarget, src_view: &wgpu::TextureView, opacity: f32, blend_mode: msx_ast::BlendMode) {
        let backdrop = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msx layer blend backdrop snapshot"),
            size: wgpu::Extent3d { width: dst.width(), height: dst.height(), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let backdrop_view = backdrop.create_view(&wgpu::TextureViewDescriptor::default());

        let params = BlendParams { opacity, blend_mode: blend_mode as u32, _pad: [0.0; 2] };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx layer backdrop blend params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx layer backdrop blend bind group"),
            layout: &self.blend_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&backdrop_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: params_buffer.as_entire_binding() },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx layer backdrop blend encoder"),
        });

        // The snapshot: dst's own texture (its state as of right now,
        // before this call changes it) copied into `backdrop`. Must
        // happen as a distinct encoder step before the render pass
        // below opens — `copy_texture_to_texture` and
        // `begin_render_pass` can't overlap on the same encoder.
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: dst.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &backdrop,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d { width: dst.width(), height: dst.height(), depth_or_array_layers: 1 },
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("msx layer backdrop blend pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst.view,
                    depth_slice: None,
                    resolve_target: None,
                    // `Load`, not `Clear` — this pass's own shader output
                    // already accounts for whatever was in `dst` (that's
                    // the entire point of sampling the backdrop copy),
                    // so the attachment just needs to still hold that
                    // same content up until the draw call overwrites it;
                    // `Clear` here would erase it before the shader ever
                    // gets to read the (separate, already-safe) copy.
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.blend_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

/// Whether `elements`, reachable purely through `Group` nesting (never
/// descending into a `Layer`'s own children — each `Layer` gets its own
/// separate recursive `paint_order` scope via `PaintOp::Composite`, not
/// this check), contains at least one `Element::Layer` anywhere in the
/// subtree.
///
/// This is the one question that decides whether a `Group` needs to be
/// pulled out of its surrounding `Run` and given its own recursive
/// `paint_order` pass (`PaintOp::GroupSplit`) or can stay exactly where
/// it is, tessellated inline as ordinary content by `vector.rs`/`sdf.rs`/
/// `splat.rs`'s own existing internal Group-recursion — which is correct
/// and unchanged for the common case (a Group with no Layer anywhere
/// inside it), but cannot represent a Layer's isolated-buffer-plus-
/// composite semantics at all, since tessellation only ever produces
/// vertices for the shared vertex/index buffers those crates draw in one
/// batch.
fn group_contains_layer(elements: &[Element]) -> bool {
    elements.iter().any(|el| match el {
        Element::Layer(_) => true,
        Element::Group(g) => group_contains_layer(&g.children),
        _ => false,
    })
}

/// One step in a single sibling list's real final paint order — see
/// `msx_ast::layer::layer_reordered`'s own doc for the reordering rule
/// this is built directly on top of (a `Layer` only ever trades places
/// with another `Layer` in the SAME list; a non-`Layer` sibling's slot
/// is always pinned, exactly matching what `msx-render-svg`/
/// `msx-render-cpu` already do via that same function).
enum PaintOp<'a> {
    /// A genuine slice of the ORIGINAL `elements` array passed to
    /// `paint_order` — never a copy. Safe because of the invariant
    /// above: a run of non-`Layer` siblings between two `Layer`-occupied
    /// slots never moves, regardless of which `Layer` ends up assigned
    /// to those slots after z_index reordering (confirmed directly
    /// against `layer_reordered`'s real implementation: it starts as an
    /// exact identity copy of references and only ever overwrites the
    /// indices where a `Layer` originally sat).
    Run(&'a [Element]),
    Composite(&'a Layer),
    /// A `Group`, at this exact position in the sibling list, whose
    /// subtree contains a `Layer` somewhere (`group_contains_layer`) and
    /// therefore can't be folded into a surrounding `Run`'s ordinary
    /// tessellated batch. Recursed into via `render_ops`'s own
    /// `paint_order(&group.children)` call, not tessellated here.
    GroupSplit(&'a Group),
}

/// Splits `elements` into the ordered sequence of `PaintOp`s that
/// reflects this ONE sibling list's real paint order. This asks the same
/// question SVG/CPU already ask via `layer_reordered`, but scoped to
/// exactly one sibling list at a time, and preserves exactly where each
/// resulting `Composite`/`GroupSplit` falls relative to its real
/// non-`Layer` neighbors in THIS list.
///
/// `layer_reordered` only ever moves `Layer`-occupied slots (see its own
/// doc); every `Group` slot — whether or not it needs `GroupSplit` — is
/// therefore guaranteed to sit at the exact same index in `reordered` as
/// in `elements`, the same invariant the existing `Run` slicing below
/// already relies on for non-`Layer` elements generally.
fn paint_order(elements: &[Element]) -> Vec<PaintOp<'_>> {
    let reordered = msx_ast::layer::layer_reordered(elements);
    debug_assert_eq!(reordered.len(), elements.len(), "layer_reordered must be a same-length permutation");

    let mut ops = Vec::new();
    let mut run_start = 0usize;
    for (i, el) in reordered.iter().enumerate() {
        match el {
            Element::Layer(layer) => {
                if run_start < i {
                    ops.push(PaintOp::Run(&elements[run_start..i]));
                }
                ops.push(PaintOp::Composite(layer));
                run_start = i + 1;
            }
            Element::Group(g) if group_contains_layer(&g.children) => {
                if run_start < i {
                    ops.push(PaintOp::Run(&elements[run_start..i]));
                }
                ops.push(PaintOp::GroupSplit(g));
                run_start = i + 1;
            }
            _ => {}
        }
    }
    if run_start < elements.len() {
        ops.push(PaintOp::Run(&elements[run_start..]));
    }
    ops
}

/// The vector → shader-fill → SDF → splat batch every non-Layer render
/// path already did, unchanged internally, now scoped to one `Run`
/// slice instead of a whole scene/layer's worth of elements at once, and
/// parametrized on whether this batch clears `view` (only the very
/// first thing painted into a given target should) or loads onto
/// whatever a previous run or Layer composite already painted there.
///
/// NOTE on ordering WITHIN a run: this still draws all of this run's
/// vector shapes, then all its shader fills, then all its SDF nodes,
/// then all its splats — type-batched, same as before this fix, NOT
/// true per-element document order between those types. That's a
/// separate, pre-existing gap from day one of this crate (independent
/// of Layers/z_index entirely — e.g. `sdf_splat_mixed.msx` orders its
/// elements deliberately for z-order reasons per its own header comment,
/// and this crate doesn't actually respect that ordering today). Fixing
/// it would mean turning SDF/splat from "collect everything in this
/// scope, one batched/instanced draw" into "draw one at a time,
/// interleaved with vector" — a bigger, separate change with its own
/// performance question, deliberately not folded into this fix.
#[allow(clippy::too_many_arguments)]
fn draw_run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    run: &[Element],
    transform: Matrix2D,
    canvas: (f32, f32),
    clear: Option<wgpu::Color>,
    vector_pipeline: &VectorPipeline,
    sdf_pipeline: &SdfPipeline,
    splat_pipeline: &SplatPipeline,
    shader_pipeline: &ShaderFillPipeline,
    masked_shader_composite: &MaskedShaderComposite,
    image_pipeline: &crate::image::ImagePipeline,
    defs: &vector::Defs,
    shader_base_dir: &std::path::Path,
    time: f32,
) {
    let (geometry, shader_shapes) = vector::tessellate_elements_with_shaders(run, transform, canvas, defs);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("msx render-ordered run encoder"),
    });
    match clear {
        Some(color) => vector_pipeline.draw(device, &mut encoder, view, &geometry, color),
        None => vector_pipeline.draw_loaded(device, &mut encoder, view, &geometry),
    }
    for shape in &shader_shapes {
        if let Err(e) = shader_pipeline.draw(device, &mut encoder, view, shader_base_dir, shape, time) {
            eprintln!("msx-render-gpu: {e} — falling back to fallback_color");
            vector_pipeline.draw_fallback_fill(device, &mut encoder, view, &shape.vertices, &shape.indices, shape.shader.fallback_color);
        }
    }
    sdf_pipeline.draw_all_elements(
        device, &mut encoder, view, run, transform, canvas, defs,
        Some(&SdfShaderContext { shader_pipeline, composite: masked_shader_composite, shader_base_dir, time }),
    );
    splat_pipeline.draw_all_elements(
        device, &mut encoder, view, run, transform, canvas, defs,
        Some(&SplatShaderContext { shader_pipeline, composite: masked_shader_composite, shader_base_dir, time }),
    );
    queue.submit(std::iter::once(encoder.finish()));

    // Not recorded into the shared `encoder` above like SDF/splat are —
    // `ImagePipeline::draw_all_elements` manages its own encoder(s)
    // internally (one per image, since each needs a real texture upload
    // via `queue.write_texture` ahead of its own draw call), so this
    // runs as its own, separate step after the rest of this run's
    // submission, not folded into it.
    image_pipeline.draw_all_elements(device, queue, view, run, transform, canvas, shader_base_dir);
}

/// Entry point: clears `view` to `initial_clear`, then hands off to
/// `render_ops` for the actual walk. `view` is unconditionally cleared
/// as its own explicit first step — before anything in `elements` is
/// looked at — because this has to happen regardless of whether
/// `elements` happens to start with a `Layer` (no leading `Run` to
/// piggyback a clear onto) or is empty; recursive calls for nested
/// `Layer`s and `Group`s go straight to `render_ops` instead, since only
/// the outermost call for a given target (the real scene, or a fresh
/// `Layer` buffer in `render_layer`) should ever clear it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_ordered(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &OffscreenTarget,
    elements: &[Element],
    transform: Matrix2D,
    canvas: (f32, f32),
    initial_clear: wgpu::Color,
    layer_compositor: &LayerCompositor,
    vector_pipeline: &VectorPipeline,
    sdf_pipeline: &SdfPipeline,
    splat_pipeline: &SplatPipeline,
    shader_pipeline: &ShaderFillPipeline,
    masked_shader_composite: &MaskedShaderComposite,
    image_pipeline: &crate::image::ImagePipeline,
    defs: &vector::Defs,
    scene_defs: &[Def],
    shader_base_dir: &std::path::Path,
    time: f32,
) {
    let view = &target.view;

    // Unconditional up-front clear — see doc comment above for why this
    // can't just be "the first Run clears."
    {
        let empty = vector::VectorGeometry { vertices: Vec::new(), indices: Vec::new() };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx render-ordered initial clear"),
        });
        vector_pipeline.draw(device, &mut encoder, view, &empty, initial_clear);
        queue.submit(std::iter::once(encoder.finish()));
    }

    render_ops(
        device, queue, target, elements, transform, canvas,
        layer_compositor, vector_pipeline, sdf_pipeline, splat_pipeline,
        shader_pipeline, masked_shader_composite, image_pipeline,
        defs, scene_defs, shader_base_dir, time,
    );
}

/// Walks `elements` in real paint order (`paint_order`, above) and
/// issues the corresponding operations into `view`, WITHOUT clearing it
/// first — `view` is assumed to already hold whatever came before this
/// call in real document order (a previous sibling `Run`, an enclosing
/// `Layer`'s own already-cleared fresh buffer, or an enclosing `Group`'s
/// parent content), and every `Run` here draws with `draw_run`'s
/// load-not-clear path accordingly.
///
/// Each `PaintOp` gets exactly the treatment its own doc comment
/// describes: `Run` draws inline via the existing tessellated batch,
/// `Composite` performs a real, unconditionally-recursive `render_layer`
/// call (this is what makes a `Layer` nested inside another `Layer`
/// render for real — there is no longer any flag suppressing it, and
/// `msx-render-svg`/`msx-render-cpu` already do the equivalent via their
/// own plain recursive dispatch, so this isn't GPU inventing new
/// cross-renderer behavior, just matching what those two already do),
/// and `GroupSplit` recurses into THIS SAME function on `group.children`
/// with the Group's accumulated transform — giving that Group's own
/// contents the exact same local, sibling-scoped `paint_order` treatment
/// as any other sibling list in the tree, drawn inline at exactly the
/// Group's real position rather than deferred and globally sorted.
#[allow(clippy::too_many_arguments)]
fn render_ops(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &OffscreenTarget,
    elements: &[Element],
    transform: Matrix2D,
    canvas: (f32, f32),
    layer_compositor: &LayerCompositor,
    vector_pipeline: &VectorPipeline,
    sdf_pipeline: &SdfPipeline,
    splat_pipeline: &SplatPipeline,
    shader_pipeline: &ShaderFillPipeline,
    masked_shader_composite: &MaskedShaderComposite,
    image_pipeline: &crate::image::ImagePipeline,
    defs: &vector::Defs,
    scene_defs: &[Def],
    shader_base_dir: &std::path::Path,
    time: f32,
) {
    let view = &target.view;
    let canvas_u32 = (canvas.0.round().max(1.0) as u32, canvas.1.round().max(1.0) as u32);
    let ops = paint_order(elements);

    for op in &ops {
        match op {
            PaintOp::Run(run) => {
                draw_run(
                    device, queue, view, run, transform, canvas, None,
                    vector_pipeline, sdf_pipeline, splat_pipeline, shader_pipeline,
                    masked_shader_composite, image_pipeline, defs, shader_base_dir, time,
                );
            }
            PaintOp::Composite(layer) => {
                layer_compositor.render_layer(
                    device, queue, target, layer, transform, canvas_u32,
                    vector_pipeline, sdf_pipeline, splat_pipeline, shader_pipeline,
                    masked_shader_composite, image_pipeline, scene_defs, shader_base_dir, time,
                );
            }
            PaintOp::GroupSplit(group) => {
                let local = group.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                let combined = transform.concat(local);
                render_ops(
                    device, queue, target, &group.children, combined, canvas,
                    layer_compositor, vector_pipeline, sdf_pipeline, splat_pipeline,
                    shader_pipeline, masked_shader_composite, image_pipeline,
                    defs, scene_defs, shader_base_dir, time,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Element as MsxElement, Group, Rect, Style};

    fn rect() -> Element {
        MsxElement::Rect(Rect::new(0.0, 0.0, 1.0, 1.0, Style::default()))
    }

    fn id_of_run<'a>(op: &PaintOp<'a>) -> &'a [Element] {
        match op {
            PaintOp::Run(r) => *r,
            _ => panic!("expected Run"),
        }
    }

    // ── backdrop_blend.wgsl formula cross-check ─────────────────────────
    //
    // Everything below this point is a verbatim Rust transcription of
    // `shaders/backdrop_blend.wgsl`'s `blend_channel`/`hard_light`/
    // `soft_light`, checked against `msx_render_core::blend`'s real,
    // independently-unit-tested reference. This is the one piece of the
    // whole non-Normal-blend-mode feature that can be given real,
    // GPU-adapter-independent test coverage — everything else here only
    // runs `if_a_gpu_adapter_is_available` (see lib.rs's own tests), so
    // this is what actually runs on every CI invocation regardless of
    // runner GPU support. If a future edit to either the WGSL or this
    // mirror introduces a transcription slip (wrong operator, swapped
    // argument, a typo'd constant), this is what catches it — not a real
    // adapter, since none of this touches wgpu at all.

    fn wgsl_hard_light(cb: f32, cs: f32) -> f32 {
        if cs <= 0.5 {
            return cb * (2.0 * cs);
        }
        let s = 2.0 * cs - 1.0;
        cb + s - cb * s
    }

    fn wgsl_soft_light(cb: f32, cs: f32) -> f32 {
        if cs <= 0.5 {
            return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
        }
        let d = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }

    fn wgsl_blend_channel(cb: f32, cs: f32, mode: msx_ast::BlendMode) -> f32 {
        use msx_ast::BlendMode::*;
        match mode {
            Multiply => cb * cs,
            Screen => cb + cs - cb * cs,
            Overlay => wgsl_hard_light(cs, cb),
            Add => (cb + cs).min(1.0),
            SoftLight => wgsl_soft_light(cb, cs),
            HardLight => wgsl_hard_light(cb, cs),
            Difference => (cb - cs).abs(),
            Exclusion => cb + cs - 2.0 * cb * cs,
            Darken => cb.min(cs),
            Lighten => cb.max(cs),
            Subtract => (cb - cs).max(0.0),
            Divide => {
                if cs <= 0.0 { 1.0 } else { (cb / cs).min(1.0) }
            }
            Normal => cs,
        }
    }

    #[test]
    fn wgsl_blend_port_matches_real_rust_reference() {
        use msx_ast::BlendMode::*;
        let modes = [Multiply, Screen, Overlay, Add, SoftLight, HardLight, Difference, Exclusion, Darken, Lighten, Subtract, Divide];
        let vals = [0.0f32, 0.1, 0.24, 0.25, 0.26, 0.5, 0.75, 0.9, 1.0];

        for mode in modes {
            for &cb in &vals {
                for &cs in &vals {
                    let real = match mode {
                        Multiply => msx_render_core::blend::multiply(cb, cs),
                        Screen => msx_render_core::blend::screen(cb, cs),
                        Overlay => msx_render_core::blend::overlay(cb, cs),
                        Add => msx_render_core::blend::add(cb, cs),
                        SoftLight => msx_render_core::blend::soft_light(cb, cs),
                        HardLight => msx_render_core::blend::hard_light(cb, cs),
                        Difference => msx_render_core::blend::difference(cb, cs),
                        Exclusion => msx_render_core::blend::exclusion(cb, cs),
                        Darken => msx_render_core::blend::darken(cb, cs),
                        Lighten => msx_render_core::blend::lighten(cb, cs),
                        Subtract => msx_render_core::blend::subtract(cb, cs),
                        Divide => msx_render_core::blend::divide(cb, cs),
                        Normal => cs,
                    };
                    let ported = wgsl_blend_channel(cb, cs, mode);
                    assert!(
                        (real - ported).abs() < 1e-5,
                        "{:?} cb={} cs={}: rust reference={} wgsl-mirror={}",
                        mode, cb, cs, real, ported
                    );
                }
            }
        }
    }

    /// The full premultiplied-domain formula (not just `blend_channel`
    /// in isolation) — mirrors `backdrop_blend.wgsl`'s `fs_main` exactly,
    /// including opacity scaling and the premultiplied<->straight
    /// conversions, cross-checked against `msx_render_core::composite`'s
    /// straight-alpha reference across partial-alpha AND partial-opacity
    /// cases. This is the derivation in this file's own `composite_blended`
    /// doc comment, confirmed numerically rather than just algebraically.
    #[test]
    fn wgsl_full_composite_formula_matches_reference() {
        use msx_ast::BlendMode::*;
        // (backdrop straight color, backdrop alpha, source straight color,
        //  source alpha before opacity, opacity, mode)
        let cases: &[(f32, f32, f32, f32, f32, msx_ast::BlendMode)] = &[
            (1.0, 1.0, 0.0, 1.0, 1.0, Multiply),
            (0.3, 1.0, 0.6, 1.0, 1.0, Screen),
            (0.8, 0.6, 0.2, 0.7, 1.0, Darken),
            (0.2, 0.4, 0.9, 0.3, 0.5, Lighten),
            (0.5, 1.0, 0.5, 1.0, 0.5, Difference),
            (0.9, 0.2, 0.1, 0.8, 0.75, HardLight),
            (1.0, 1.0, 0.0, 1.0, 0.5, Darken), // the exact case the lib.rs GPU test also checks
        ];

        for &(cb, ab, cs, a_src, opacity, mode) in cases {
            let a_src_scaled = a_src * opacity;
            let (ref_r, _, _, ref_a) = msx_render_core::composite(mode, (cb, cb, cb, ab), (cs, cs, cs, a_src_scaled));

            let bp_rgb = cb * ab;
            let sp_rgb_scaled = cs * a_src * opacity;
            let sp_a_scaled = a_src_scaled;

            let (result_premul_rgb, result_a) = if sp_a_scaled <= 0.0 {
                (bp_rgb, ab)
            } else {
                let cs_straight = sp_rgb_scaled / sp_a_scaled.max(0.0001);
                let cb_straight = bp_rgb / ab.max(0.0001);
                let blended = wgsl_blend_channel(cb_straight, cs_straight, mode);
                let rgb = (1.0 - sp_a_scaled) * bp_rgb + (1.0 - ab) * sp_rgb_scaled + sp_a_scaled * ab * blended;
                let a = sp_a_scaled + ab * (1.0 - sp_a_scaled);
                (rgb, a)
            };

            assert!((result_a - ref_a).abs() < 1e-4, "{:?}: alpha mismatch: ported={} ref={}", mode, result_a, ref_a);
            let result_straight_rgb = if result_a > 0.0 { result_premul_rgb / result_a } else { 0.0 };
            assert!(
                (result_straight_rgb - ref_r).abs() < 1e-4,
                "{:?}: rgb mismatch: ported={} ref={} (cb={cb} ab={ab} cs={cs} a_src={a_src} opacity={opacity})",
                mode, result_straight_rgb, ref_r
            );
        }
    }

    /// `Normal` (mode 0) is never actually routed through this shader in
    /// practice (`composite` takes the separate fixed-function fast path
    /// instead — see its own doc comment for why), but if that ever
    /// changed, the formula's own `Normal => cs` fallback must still
    /// agree with `msx_render_core::composite`'s Normal case, not just
    /// silently produce something plausible-looking.
    #[test]
    fn wgsl_formula_would_still_be_correct_for_normal_if_ever_routed_here() {
        let (ref_r, _, _, ref_a) = msx_render_core::composite(msx_ast::BlendMode::Normal, (0.2, 0.2, 0.2, 0.6), (0.9, 0.9, 0.9, 0.8));

        let cb = 0.2_f32;
        let ab = 0.6_f32;
        let cs = 0.9_f32;
        let a_src = 0.8_f32;
        let bp_rgb = cb * ab;
        let sp_rgb = cs * a_src;
        let blended = wgsl_blend_channel(cb, cs, msx_ast::BlendMode::Normal);
        let result_rgb = (1.0 - a_src) * bp_rgb + (1.0 - ab) * sp_rgb + a_src * ab * blended;
        let result_a = a_src + ab * (1.0 - a_src);
        let result_straight = result_rgb / result_a;

        assert!((result_a - ref_a).abs() < 1e-4);
        assert!((result_straight - ref_r).abs() < 1e-4);
    }

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

    /// Same category of check as `composite_params_matches_the_wgsl_struct_size`
    /// above, for `BlendParams`/`backdrop_blend.wgsl`'s own uniform struct
    /// — see `BlendParams`'s own doc comment for the byte-by-byte layout
    /// arithmetic this pins down. 16, not 32: this struct's widest member
    /// is a `vec2<f32>` (align 8), not the `vec3<f32>` (align 16)
    /// `CompositeParams` has to accommodate, which is exactly why the two
    /// numbers differ and why this one didn't need the same explicit
    /// padding-arithmetic fix.
    #[test]
    fn blend_params_matches_the_wgsl_struct_size() {
        assert_eq!(std::mem::size_of::<BlendParams>(), 16);
    }

    #[test]
    fn group_contains_layer_true_for_direct_layer_child() {
        let elements = vec![MsxElement::Layer(Layer::new(vec![]))];
        assert!(group_contains_layer(&elements));
    }

    #[test]
    fn group_contains_layer_true_through_nested_group() {
        let inner = Group::new(vec![MsxElement::Layer(Layer::new(vec![]))]);
        let elements = vec![rect(), MsxElement::Group(inner)];
        assert!(group_contains_layer(&elements));
    }

    #[test]
    fn group_contains_layer_false_with_no_layer_anywhere() {
        let inner = Group::new(vec![rect(), rect()]);
        let elements = vec![rect(), MsxElement::Group(inner)];
        assert!(!group_contains_layer(&elements));
    }

    #[test]
    fn group_contains_layer_does_not_need_to_look_inside_a_found_layers_children() {
        // Finding the Layer itself is sufficient to require a GroupSplit;
        // what's inside IT is a separate, later recursive concern
        // (`PaintOp::Composite` -> `render_layer` -> its own
        // `render_ops` call), not something `group_contains_layer` needs
        // to answer on its own behalf.
        let deeply_nested_layer = Layer::new(vec![MsxElement::Layer(Layer::new(vec![rect()]))]);
        let elements = vec![MsxElement::Layer(deeply_nested_layer)];
        assert!(group_contains_layer(&elements));
    }

    #[test]
    fn paint_order_leaves_a_layer_free_group_inside_one_run() {
        // No Layer anywhere in the Group's subtree -> the whole slice
        // stays a single Run, exactly as before this fix (still
        // tessellated inline by vector.rs's own Group-recursion).
        let group = Group::new(vec![rect(), rect()]);
        let elements = vec![rect(), MsxElement::Group(group), rect()];
        let ops = paint_order(&elements);
        assert_eq!(ops.len(), 1, "a Layer-free Group must not be split out of its Run");
        assert_eq!(id_of_run(&ops[0]).len(), 3);
    }

    #[test]
    fn paint_order_splits_a_layer_containing_group_out_of_its_run() {
        let group = Group::new(vec![MsxElement::Layer(Layer::new(vec![]))]);
        let elements = vec![rect(), MsxElement::Group(group), rect()];
        let ops = paint_order(&elements);
        assert_eq!(ops.len(), 3, "expected Run, GroupSplit, Run");
        assert_eq!(id_of_run(&ops[0]).len(), 1, "leading rect stays its own Run");
        assert!(matches!(ops[1], PaintOp::GroupSplit(_)));
        assert_eq!(id_of_run(&ops[2]).len(), 1, "trailing rect stays its own Run");
    }

    #[test]
    fn paint_order_recurses_into_a_group_split_for_its_own_local_ordering() {
        // A GroupSplit isn't a leaf: recursing paint_order on its own
        // children must, on its own terms, still resolve correctly —
        // here that Group's children are themselves just one Layer, so
        // recursing into it should yield exactly one Composite op with
        // no surrounding Run (nothing else in there to run).
        let group = Group::new(vec![MsxElement::Layer(Layer::new(vec![]))]);
        let elements = vec![MsxElement::Group(group)];
        let ops = paint_order(&elements);
        assert_eq!(ops.len(), 1);
        let PaintOp::GroupSplit(g) = &ops[0] else { panic!("expected GroupSplit") };
        let inner_ops = paint_order(&g.children);
        assert_eq!(inner_ops.len(), 1);
        assert!(matches!(inner_ops[0], PaintOp::Composite(_)));
    }

    #[test]
    fn paint_order_still_reorders_top_level_layers_by_z_index_alongside_a_group_split() {
        // Confirms GroupSplit detection doesn't interfere with the
        // pre-existing sibling-scoped Layer z_index reordering at the
        // SAME list level.
        let mut front = Layer::new(vec![]);
        front.id = Some("front".into());
        front.z_index = 5.0;
        let mut back = Layer::new(vec![]);
        back.id = Some("back".into());
        back.z_index = 1.0;
        let group = Group::new(vec![MsxElement::Layer(Layer::new(vec![]))]);

        // Document order: front(z=5), group(has a layer), back(z=1).
        // back's lower z_index must still win the earlier composite slot
        // among the top-level Layers, exactly as layer_reordered dictates,
        // regardless of the GroupSplit sitting between them.
        let elements = vec![MsxElement::Layer(front), MsxElement::Group(group), MsxElement::Layer(back)];
        let ops = paint_order(&elements);
        assert_eq!(ops.len(), 3);
        let PaintOp::Composite(first) = &ops[0] else { panic!("expected Composite first") };
        assert_eq!(first.id.as_deref(), Some("back"), "lower z_index must composite first");
        assert!(matches!(ops[1], PaintOp::GroupSplit(_)));
        let PaintOp::Composite(third) = &ops[2] else { panic!("expected Composite third") };
        assert_eq!(third.id.as_deref(), Some("front"));
    }
}
