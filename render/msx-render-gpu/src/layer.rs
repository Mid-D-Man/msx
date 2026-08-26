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
        view: &wgpu::TextureView,
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
            &buffer.view,
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

        self.composite(device, queue, view, &buffer.view, layer.opacity as f32);
    }

    /// Blends `src_view` (a fully-rendered layer buffer — see this
    /// module's "The layer buffer is premultiplied alpha" doc section)
    /// onto `dst_view` at `opacity`. Requires `src_view`'s contents to
    /// already be premultiplied alpha; this function does not itself
    /// premultiply anything, it only preserves the invariant through the
    /// opacity scale (`composite.wgsl`) and blends accordingly
    /// (`PREMULTIPLIED_ALPHA_BLENDING`, set at pipeline-creation time in
    /// `new` above).
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
    view: &wgpu::TextureView,
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
        device, queue, view, elements, transform, canvas,
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
    view: &wgpu::TextureView,
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
                    device, queue, view, layer, transform, canvas_u32,
                    vector_pipeline, sdf_pipeline, splat_pipeline, shader_pipeline,
                    masked_shader_composite, image_pipeline, scene_defs, shader_base_dir, time,
                );
            }
            PaintOp::GroupSplit(group) => {
                let local = group.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                let combined = transform.concat(local);
                render_ops(
                    device, queue, view, &group.children, combined, canvas,
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
