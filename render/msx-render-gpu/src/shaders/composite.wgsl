// render/msx-render-gpu/src/shaders/composite.wgsl
// Composites a fully-rendered layer buffer onto the current target.
//
// KNOWN GAP: this only implements opacity-scaled Normal (plain alpha)
// blending — see layer.rs's module doc for why proper Multiply/Screen/etc.
// needs a backdrop-snapshot + dual-texture pass that doesn't exist yet.

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
    return vec4<f32>(sample.rgb, sample.a * params.opacity);
}
