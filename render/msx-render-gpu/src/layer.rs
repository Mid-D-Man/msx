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
//! **Layer paint order is now sibling-scoped**, matching
//! `msx-render-svg`/`msx-render-cpu`'s own `layer_reordered`-based
//! behavior: a `Layer` only ever trades paint-order places with another
//! `Layer` in the SAME sibling list (`paint_order`, below, calls the
//! exact same `msx_ast::layer::layer_reordered` those two renderers use
//! — not a separate reimplementation), and a non-`Layer` sibling's
//! position relative to any `Layer` around it is always respected. This
//! replaced the old behavior (every `Layer` anywhere composited after
//! every non-`Layer` element, unconditionally, sorted globally against
//! every other `Layer` in the whole scene regardless of nesting depth).
//! Two deliberate scope boundaries remain, both flagged rather than
//! silently half-fixed:
//!
//! - **A `Layer` nested inside a `Group` still keeps the OLD behavior**
//!   (found via `collect_layers`'s existing Group-recursion, composited
//!   after everything else in whichever sibling list contains that
//!   Group, sorted globally against every other Group-nested Layer found
//!   the same way in that list) — see `render_ordered`'s own doc comment
//!   for why: real sibling-scoping there would mean teaching
//!   `vector.rs`/`sdf.rs`/`splat.rs`'s own internal Group-recursion about
//!   `paint_order` too, a materially bigger, separate change.
//! - **Nested layers** (a `Layer` inside another `Layer`) still aren't
//!   rendered — `render_ordered` takes an `allow_nested_layers` flag,
//!   `false` whenever it's walking a `Layer`'s own children, that turns
//!   any `Layer` found there into a silent no-op. This is a deliberate
//!   restriction, not a leftover limitation: nested-layer semantics
//!   (opacity composition across levels, whether z_index should even
//!   compare across a nesting boundary) aren't decided or tested
//!   anywhere in this project yet, not just here — enabling it as an
//!   incidental side effect of the ordering fix would trade one
//!   cross-renderer divergence for a new, unspecified one.
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

use msx_ast::{Def, Element, Layer, Matrix2D};

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
        // sibling-scoped Layer-ordering treatment the top level does
        // (this is what makes a Layer-containing-Layer-and-Rect-siblings
        // case correct at ANY nesting depth of `render_layer` calls, not
        // just at the very top) — with `allow_nested_layers = false`, so
        // a Layer nested inside THIS Layer is still silently skipped,
        // same restriction `collect_layers` always had. See
        // `render_ordered`'s own doc comment for the full reasoning.
        render_ordered(
            device,
            queue,
            &buffer.view,
            &layer.children,
            combined,
            canvas_f,
            wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            false,
            self,
            vector_pipeline,
            sdf_pipeline,
            splat_pipeline,
            shader_pipeline,
            masked_shader_composite,
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

/// Recurses through `Group`, collects `Layer` elements with their
/// accumulated transform — does NOT descend into a found `Layer`'s own
/// children (no nested-layer support; see module docs).
///
/// Kept exactly as it always was. `render_ordered` (below) no longer
/// calls this at the TOP of a sibling list — top-level Layers now go
/// through `paint_order`'s real sibling-scoped treatment instead — but
/// still calls it, deliberately, for Layers found nested inside a
/// `Group`: see `render_ordered`'s own doc for why that case keeps this
/// function's original global-collect-then-composite-last behavior
/// rather than also being taught `paint_order`'s finer-grained ordering.
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
}

/// Splits `elements` into the ordered sequence of `PaintOp`s that
/// reflects this ONE sibling list's real paint order — the piece that
/// was missing before this fix. Previously (`collect_layers` called at
/// the top of the whole tree) every `Layer` anywhere painted after every
/// non-`Layer` element, unconditionally, sorted against every OTHER
/// `Layer` in the whole scene regardless of nesting depth. This asks the
/// same question SVG/CPU already ask via `layer_reordered`, but scoped
/// to exactly one sibling list at a time, and preserves exactly where
/// each resulting composite falls relative to its real non-`Layer`
/// neighbors in THIS list.
fn paint_order(elements: &[Element]) -> Vec<PaintOp<'_>> {
    let reordered = msx_ast::layer::layer_reordered(elements);
    debug_assert_eq!(reordered.len(), elements.len(), "layer_reordered must be a same-length permutation");

    let mut ops = Vec::new();
    let mut run_start = 0usize;
    for (i, el) in reordered.iter().enumerate() {
        if let Element::Layer(layer) = el {
            if run_start < i {
                ops.push(PaintOp::Run(&elements[run_start..i]));
            }
            ops.push(PaintOp::Composite(layer));
            run_start = i + 1;
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
}

/// Walks `elements` in real paint order (`paint_order`, above) and
/// issues the corresponding operations into `view`: each `Run` gets
/// `draw_run`'s existing batch treatment, each `Composite` gets a real,
/// recursive `render_layer` call at exactly that point in the sequence
/// — replacing the old "every non-Layer element first, then every Layer
/// anywhere, sorted globally, always last" split entirely.
///
/// `view` is unconditionally cleared to `initial_clear` as the very
/// first operation, before anything in `elements` is looked at — this
/// has to happen regardless of whether `elements` happens to start with
/// a `Layer` (no leading `Run` to piggyback a clear onto) or is empty,
/// so it's pulled out as its own explicit step rather than special-cased
/// onto "whichever op happens to run first."
///
/// `allow_nested_layers = false` makes any `Composite` encountered here
/// a silent no-op instead of a real render — this is what keeps a
/// `Layer` nested inside another `Layer` unsupported, on purpose, same
/// as `collect_layers` always refusing to descend into one. Nested-layer
/// semantics (opacity composition across levels, whether z_index should
/// even compare across a nesting boundary) aren't decided or tested
/// ANYWHERE in this project yet — not just here, `msx-render-svg`/
/// `msx-render-cpu` have zero nested-layer tests either. Making this
/// function naturally recursive would have made nested layers "work" as
/// an incidental side effect of this fix; that would trade one
/// cross-renderer divergence (Layer ordering, what this fix is actually
/// for) for a new, unspecified one (GPU alone having some behavior for
/// nested layers that nothing else in the project agrees on). `lib.rs`
/// calls this with `true`; `render_layer` below calls it on its own
/// `layer.children` with `false`.
///
/// Layers found nested inside a `Group` (at any depth) within a `Run`
/// are deliberately NOT given this same sibling-scoped treatment — see
/// this function's body: each `Run`'s `Group` children are still walked
/// with the old `collect_layers`, and every layer found that way across
/// this whole `elements` list composites after everything else here,
/// sorted globally against each other (exactly `collect_layers`'s
/// original behavior, just no longer swallowing top-level Layers too).
/// Teaching `vector.rs`/`sdf.rs`/`splat.rs`'s OWN internal
/// Group-recursion about `paint_order` as well, so a Layer nested inside
/// a Group gets real sibling-scoping against ITS OWN Group-level
/// siblings, is a materially bigger, separate change — this preserves
/// today's exact (already-existing, non-regressing) behavior for that
/// case instead of silently dropping those layers or half-fixing them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_ordered(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    elements: &[Element],
    transform: Matrix2D,
    canvas: (f32, f32),
    initial_clear: wgpu::Color,
    allow_nested_layers: bool,
    layer_compositor: &LayerCompositor,
    vector_pipeline: &VectorPipeline,
    sdf_pipeline: &SdfPipeline,
    splat_pipeline: &SplatPipeline,
    shader_pipeline: &ShaderFillPipeline,
    masked_shader_composite: &MaskedShaderComposite,
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

    let canvas_u32 = (canvas.0.round().max(1.0) as u32, canvas.1.round().max(1.0) as u32);
    let ops = paint_order(elements);
    let mut leftover_group_layers: Vec<(&Layer, Matrix2D)> = Vec::new();

    for op in &ops {
        match op {
            PaintOp::Run(run) => {
                draw_run(
                    device, queue, view, run, transform, canvas, None,
                    vector_pipeline, sdf_pipeline, splat_pipeline, shader_pipeline,
                    masked_shader_composite, defs, shader_base_dir, time,
                );
                for el in run.iter() {
                    if let Element::Group(g) = el {
                        let local = g.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
                        collect_layers(&g.children, transform.concat(local), &mut leftover_group_layers);
                    }
                }
            }
            PaintOp::Composite(layer) => {
                if !allow_nested_layers {
                    continue;
                }
                layer_compositor.render_layer(
                    device, queue, view, layer, transform, canvas_u32,
                    vector_pipeline, sdf_pipeline, splat_pipeline, shader_pipeline,
                    masked_shader_composite, scene_defs, shader_base_dir, time,
                );
            }
        }
    }

    // Group-nested layers: old behavior, preserved — see this function's
    // own doc comment's last paragraph.
    leftover_group_layers.sort_by(|a, b| a.0.z_index.partial_cmp(&b.0.z_index).unwrap_or(std::cmp::Ordering::Equal));
    for (layer, layer_transform) in &leftover_group_layers {
        layer_compositor.render_layer(
            device, queue, view, layer, *layer_transform, canvas_u32,
            vector_pipeline, sdf_pipeline, splat_pipeline, shader_pipeline,
            masked_shader_composite, scene_defs, shader_base_dir, time,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Element as MsxElement, Group};

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
