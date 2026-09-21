// render/msx-render-gpu/src/effects.rs
//! GPU post-processing effects for `Layer`. Currently: `Effect::Blur`
//! only.
//!
//! ## Still open
//! `DropShadow`/`InnerShadow`/`OuterGlow`/`InnerGlow` all reuse this
//! exact blur pass in msx-render-cpu's own architecture (tint-or-invert
//! -> blur -> composite/clip) and would be the natural continuation —
//! not attempted here. A `Layer` carrying one of those four renders
//! today exactly as if that effect weren't present: `apply`'s
//! `filter_map` below simply has no arm for them (silently skipped, not
//! silently wrong — msx-render-cpu already implements all five, so this
//! is a GPU-parity gap, not a format gap).
//!
//! ## Architecture
//! A real separable two-pass Gaussian blur (horizontal pass, then a
//! second pass on the horizontal pass's own output) via
//! `shaders/blur.wgsl` — a genuine per-axis convolution, not CPU's
//! three-pass box-blur approximation (msx-render-cpu's own deliberate
//! choice, documented there). Nothing in this project promises the two
//! rasterizers produce bit-identical blur output, only that each is a
//! correct blur for what it claims to be, and a real Gaussian is cheap
//! enough per-axis on a GPU that there's no reason to approximate here
//! just because CPU does.
//!
//! Kernel weights are uploaded as a STORAGE buffer, not a uniform array
//! — see `shaders/blur.wgsl`'s own doc for why.
//!
//! `apply` returns `None` when nothing changed (an empty `effects` list,
//! or every entry present is one of the four still-unimplemented
//! variants) — a `Layer` with no blur, the overwhelmingly common case,
//! pays zero extra allocation or GPU work. `render_layer` (in
//! `layer.rs`) falls back to the original, un-blurred buffer in that
//! case.

use wgpu::util::DeviceExt;

use msx_ast::Effect;

use crate::target::OffscreenTarget;

/// Hard cap on taps either side of center (so `2*MAX_TAPS + 1` total) —
/// keeps a pathological `radius` from allocating an unbounded kernel or
/// looping unboundedly in the fragment shader. `radius / 3.0` (see
/// `gaussian_kernel`'s own doc) means a `radius` past roughly 3x this
/// cap is indistinguishable from the cap anyway (the Gaussian's tail
/// contributes negligible weight well before then), so clamping here
/// costs no real visual range.
const MAX_TAPS: usize = 64;

/// Normalized 1D Gaussian kernel weights spanning `taps` texels either
/// side of center (`2*taps + 1` weights total, index 0 = leftmost/
/// topmost tap). `radius` maps to a Gaussian sigma via `sigma = radius /
/// 3.0` — the standard "~99.7% of a Gaussian's mass sits within 3
/// standard deviations" convention — chosen so `radius` means
/// approximately the same visual thing here as it does for
/// msx-render-cpu's box-blur-x3 approximation of the same
/// `Effect::Blur { radius }` field (both should look like "about this
/// many pixels of blur" for the same number), even though the two
/// rasterizers compute genuinely different curves — not a promise of
/// pixel-identical output between them, just a comparable meaning for
/// the same input.
///
/// Pure function, zero wgpu dependency — verified standalone (see this
/// module's tests): always normalizes to unit mass, never produces a
/// NaN or negative weight across a 0.1-50.0 radius sweep, and is
/// symmetric/monotonically non-increasing away from center.
fn gaussian_kernel(radius: f32) -> Vec<f32> {
    let radius = radius.max(0.1); // guards the sigma divide below for a literal-zero or negative input
    let sigma = (radius / 3.0).max(0.05);
    let taps = (radius.ceil() as usize).clamp(1, MAX_TAPS);

    let mut weights: Vec<f32> = (0..=taps * 2)
        .map(|i| {
            let x = i as f32 - taps as f32;
            (-(x * x) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f32 = weights.iter().sum();
    for w in &mut weights {
        *w /= sum;
    }
    weights
}

// `#[repr(C)]` plus hand-checked WGSL layout, same discipline as
// `layer.rs`'s `CompositeParams`/`BlendParams` (see those for the full
// worked example of why this can't just be assumed from the Rust side
// alone). WGSL side (`shaders/blur.wgsl`):
//     struct BlurParams { direction: vec2<f32>, texel_size: vec2<f32>, taps: u32 }
// Offsets: direction @ 0 (align 8, size 8), texel_size @ 8 (already
// 8-aligned, no gap needed — unlike a `vec3`, two back-to-back `vec2`s
// never need padding between them), taps @ 16 (align 4, size 4). Struct
// align = max(8, 8, 4) = 8; size = round_up(16 + 4, 8) = 24. A plain
// Rust `#[repr(C)]` struct with two `[f32; 2]` fields (Rust align 4,
// same reasoning as `BlendParams`' own `_pad: [f32; 2]`) followed by
// `u32` then one explicit trailing `u32` pad reproduces that exact
// 24-byte/offset-16 layout without needing any inserted padding fields
// before either `[f32; 2]` — verified via `size_of::<BlurParams>() == 24`
// in this module's tests.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurParams {
    direction: [f32; 2],
    texel_size: [f32; 2],
    taps: u32,
    _pad: u32,
}

pub struct GpuEffects {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl GpuEffects {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx layer effects blur shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blur.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx layer effects blur bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, // src_texture
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, // src_sampler
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, // params
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3, // kernel
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx layer effects blur pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx layer effects blur pipeline"),
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
                    // `None`, not a blend state — each pass's fragment
                    // output IS that pass's final pixel (a weighted sum
                    // of premultiplied samples, itself a valid
                    // premultiplied result — see blur.wgsl's own doc),
                    // written into a fresh target texture no earlier
                    // pass has touched. There is nothing already in that
                    // destination to blend with.
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("msx layer effects blur sampler"),
            ..Default::default()
        });

        GpuEffects { pipeline, bind_group_layout, sampler }
    }

    /// Runs every `Effect::Blur` found in `effects`, in order, as a
    /// chained pair of passes each (horizontal then vertical) — later
    /// blurs in the list compose onto the result of earlier ones, the
    /// same left-to-right effect stacking `render_layer` already applies
    /// this result to (compositing after). Any other `Effect` variant is
    /// silently skipped (see this module's doc for why that's the
    /// correct current behavior, not a bug).
    ///
    /// Returns `None`, touching neither `device` nor `queue` again after
    /// the initial no-op check, when `effects` contains no `Blur` at
    /// all — the common case (most layers have no effects), kept cheap
    /// on purpose.
    pub fn apply(&self, device: &wgpu::Device, queue: &wgpu::Queue, src: &OffscreenTarget, effects: &[Effect]) -> Option<OffscreenTarget> {
        let radii: Vec<f32> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Blur { radius } => Some(*radius as f32),
                _ => None,
            })
            .collect();
        if radii.is_empty() {
            return None;
        }

        let mut current: Option<OffscreenTarget> = None;
        for radius in radii {
            let input_view: &wgpu::TextureView = current.as_ref().map(|t| &t.view).unwrap_or(&src.view);
            let horizontal = self.pass(device, queue, input_view, src.width(), src.height(), [1.0, 0.0], radius);
            let vertical   = self.pass(device, queue, &horizontal.view, src.width(), src.height(), [0.0, 1.0], radius);
            current = Some(vertical);
        }
        current
    }

    /// One axis of one blur: renders a full-screen pass sampling
    /// `input_view` with `direction`/`radius` into a freshly-allocated
    /// `width`x`height` target, and returns that target. `TEXTURE_BINDING`
    /// on the output (in addition to `RENDER_ATTACHMENT`) is required
    /// either way it gets used next: the vertical pass samples the
    /// horizontal pass's own output, and `render_layer`'s later
    /// `composite`/`composite_blended` call samples whatever this
    /// function's caller (`apply`) ultimately returns.
    fn pass(&self, device: &wgpu::Device, queue: &wgpu::Queue, input_view: &wgpu::TextureView, width: u32, height: u32, direction: [f32; 2], radius: f32) -> OffscreenTarget {
        let output = OffscreenTarget::new(
            device, width, height,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );

        let kernel = gaussian_kernel(radius);
        let taps = (kernel.len() as u32 - 1) / 2;

        let params = BlurParams {
            direction,
            texel_size: [1.0 / width as f32, 1.0 / height as f32],
            taps,
            _pad: 0,
        };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx layer effects blur params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let kernel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx layer effects blur kernel"),
            contents: bytemuck::cast_slice(&kernel),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx layer effects blur bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(input_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: params_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: kernel_buffer.as_entire_binding() },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx layer effects blur encoder"),
        });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("msx layer effects blur pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &bind_group, &[]);
            rp.draw(0..6, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_normalizes_to_unit_mass() {
        for radius in [0.1_f32, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 50.0] {
            let k = gaussian_kernel(radius);
            let sum: f32 = k.iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "radius {radius}: sum {sum}, expected ~1.0");
        }
    }

    #[test]
    fn kernel_has_no_nan_or_negative_weights() {
        let mut r = 0.1_f32;
        while r <= 50.0 {
            for &w in &gaussian_kernel(r) {
                assert!(w.is_finite() && w >= 0.0, "radius {r}: bad weight {w}");
            }
            r += 0.37; // irrational-ish step so the sweep isn't suspiciously round
        }
    }

    #[test]
    fn kernel_is_symmetric_and_peaks_at_center() {
        let k = gaussian_kernel(10.0);
        let mid = k.len() / 2;
        assert_eq!(k.len() % 2, 1, "kernel should have an odd tap count (a true center tap)");
        for i in 0..=mid {
            assert!((k[i] - k[k.len() - 1 - i]).abs() < 1e-6, "not symmetric at {i}");
        }
        for i in 0..mid {
            assert!(k[i] <= k[i + 1] + 1e-6, "not monotonically non-decreasing toward center at {i}");
        }
    }

    #[test]
    fn kernel_respects_max_taps_cap() {
        let k = gaussian_kernel(1000.0);
        assert_eq!(k.len(), MAX_TAPS * 2 + 1);
    }

    #[test]
    fn kernel_never_empty_even_at_zero_radius() {
        // radius.max(0.1) inside gaussian_kernel guards this — a literal
        // 0 or negative radius (malformed input, not a real DixScript
        // value since parse_effect defaults to 4.0 and the format has no
        // negative-radius concept) must still produce a valid 3-tap
        // kernel, not panic or divide by zero.
        for bad in [0.0_f32, -5.0, -0.0001] {
            let k = gaussian_kernel(bad);
            assert!(!k.is_empty());
            assert!(k.iter().all(|w| w.is_finite()));
        }
    }

    #[test]
    fn blur_params_matches_wgsl_std_layout_size() {
        // Only total byte size is asserted here, not `align_of` — WGSL's
        // struct alignment (8, from its `vec2<f32>` fields) governs
        // field OFFSETS inside the byte content `bytemuck::bytes_of`
        // uploads, not any property of the Rust struct's own in-memory
        // placement. `#[repr(C)]` here naturally lands `direction` at
        // offset 0, `texel_size` at 8, and `taps` at 16 (each field's
        // own 4-byte Rust alignment already divides its offset evenly,
        // so no inserted padding is needed anywhere in this particular
        // struct) — `align_of::<BlurParams>()` is 4, not 8, and that's
        // fine: nothing reads it, wgpu only ever sees the raw bytes.
        assert_eq!(std::mem::size_of::<BlurParams>(), 24);
    }
}
