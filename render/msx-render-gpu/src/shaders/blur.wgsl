// render/msx-render-gpu/src/shaders/blur.wgsl
// One axis of a separable Gaussian blur — `effects.rs::GpuEffects::apply`
// runs this twice per `Effect::Blur` (horizontal pass, then vertical pass
// on the horizontal pass's own output) rather than doing a single 2D
// convolution, which is the standard trick that turns an O(taps^2)
// per-pixel cost into O(taps) twice — see this project's `effects.rs`
// module doc for why this is a real Gaussian (not CPU's box-blur-x3
// approximation) and why the kernel is a storage buffer, not a uniform
// array.
//
// Vertex stage is the same full-screen two-triangle quad already used by
// `composite.wgsl`/`backdrop_blend.wgsl` in this crate, including the
// same V-flip (texture-space V grows downward, clip space Y grows
// upward) — copied verbatim rather than re-derived, so all three passes
// agree on orientation by construction.

struct BlurParams {
    direction:  vec2<f32>, // (1,0) for the horizontal pass, (0,1) for the vertical pass
    texel_size: vec2<f32>, // (1/width, 1/height) of the texture being sampled
    taps:       u32,       // kernel extends `taps` texels either side of center
};

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;
// Read-only storage, not a uniform array: WGSL pads every element of a
// `uniform` array to 16 bytes regardless of the element's own size (a
// plain f32 kernel would burn 4x the bytes for nothing — there's no
// dynamic-indexing performance need here that `uniform` exists to serve
// at this scale). Sized `2*taps + 1`, matching `params.taps` exactly —
// `effects.rs` allocates it fresh per pass, sized to that pass's own
// kernel, never reused across a different radius.
@group(0) @binding(3) var<storage, read> kernel: array<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 0.0),
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

// The source texture holds premultiplied alpha (same invariant
// `composite.wgsl`/`backdrop_blend.wgsl` already document for a layer
// buffer) — a weighted sum of premultiplied samples is itself a valid
// premultiplied result (linearity of the convolution preserves the
// `rgb = true_color * true_alpha` relationship at every tap, since it
// holds independently at each sample being averaged), so no
// premultiply/unpremultiply step is needed here the way a straight-alpha
// source would require to avoid dark fringing at partial-coverage edges.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var sum = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    let taps = i32(params.taps);
    for (var i: i32 = -taps; i <= taps; i = i + 1) {
        let weight = kernel[u32(i + taps)];
        let offset = params.direction * params.texel_size * f32(i);
        sum = sum + textureSample(src_texture, src_sampler, in.uv + offset) * weight;
    }
    return sum;
}
