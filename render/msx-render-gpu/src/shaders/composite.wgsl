// render/msx-render-gpu/src/shaders/composite.wgsl
// Composites a fully-rendered layer buffer onto the current target.
//
// KNOWN GAP: this only implements opacity-scaled Normal (plain alpha)
// blending — see layer.rs's module doc for why proper Multiply/Screen/etc.
// needs a backdrop-snapshot + dual-texture pass that doesn't exist yet.
//
// `layer_texture` is PREMULTIPLIED alpha, not straight — every child draw
// that wrote into it (vector/SDF/splat, shader-filled or not) got there by
// alpha-blending onto a buffer that started fully transparent, which is
// exactly the operation that produces `rgb = true_color * true_alpha` in
// the destination regardless of which blend equation any individual draw
// used. Scaling BOTH `sample.rgb` and `sample.a` by `params.opacity` here
// (not just alpha) is what keeps that premultiplied invariant intact after
// the layer's own opacity attenuates it — see `layer.rs`'s
// `LayerCompositor::composite` for the matching
// `PREMULTIPLIED_ALPHA_BLENDING` pipeline state this output is meant to be
// blended with. Scaling only alpha (the old, buggy version of this file)
// left rgb un-attenuated going into a blend equation that then multiplies
// by alpha a SECOND time — harmless when the source pixel's alpha was
// already exactly 1.0 (every previous test's sampled pixel), but a real,
// silent darkening for any partial-alpha pixel, most visibly a Gaussian
// splat's naturally-partial coverage almost everywhere.

@group(0) @binding(0) var layer_texture: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;

struct CompositeParams {
    opacity: f32,
    _pad: vec3<f32>,
};
@group(0) @binding(2) var<uniform> params: CompositeParams;

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
    // V flipped vs. clip-space Y: texture-space V grows downward, clip
    // space Y grows upward.
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 0.0),
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(layer_texture, layer_sampler, input.uv);
    return vec4<f32>(sample.rgb * params.opacity, sample.a * params.opacity);
}
