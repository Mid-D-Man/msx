// render/msx-render-gpu/src/shaders/color_matrix.wgsl
// One 4x5 color matrix pass over a layer buffer, for `Effect::ColorMatrix`.
// The vertex stage is the full-screen quad of `blur.wgsl`, V flip included.
//
// The layer buffer holds premultiplied alpha, and the matrix is defined on
// straight channels, so the fragment stage divides the alpha out, optionally
// moves the color to linear light, applies the matrix, holds the result to 0
// to 1, moves back, and premultiplies again. This is the same sequence as
// `msx-render-cpu`'s `color_matrix`.

struct Params {
    // Column major: m[column][row], filled from the row major 4x5 matrix.
    m:      mat4x4<f32>,
    // The fifth column of the matrix.
    offset: vec4<f32>,
    // 1 when the matrix works on linear light.
    linear: u32,
    pad0:   u32,
    pad1:   u32,
    pad2:   u32,
};

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

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

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), c * 12.92, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let px = textureSample(src_texture, src_sampler, in.uv);
    var rgb = vec3<f32>(0.0);
    if (px.a > 0.0) {
        rgb = clamp(px.rgb / px.a, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (params.linear == 1u) {
        rgb = srgb_to_linear(rgb);
    }
    let out = clamp(params.m * vec4<f32>(rgb, px.a) + params.offset, vec4<f32>(0.0), vec4<f32>(1.0));
    var color = out.rgb;
    if (params.linear == 1u) {
        color = linear_to_srgb(color);
    }
    return vec4<f32>(color * out.a, out.a);
}
