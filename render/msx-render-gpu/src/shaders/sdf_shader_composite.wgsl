// render/msx-render-gpu/src/shaders/sdf_shader_composite.wgsl
// Composites a shader-def's rendered output onto an SDF node's true
// shape. `color_tex` is the shader's full output, drawn by
// ShaderFillPipeline over the node's rectangular bounding-box quad — NOT
// clipped to the node's actual silhouette, since a quad is rectangular
// and the shape usually isn't (a circle's bounding box has four corners
// the circle itself doesn't cover). `mask_tex` is that same node's own
// antialiased fill coverage, rendered by SdfPipeline::draw_mask with
// fill forced to opaque white — sdf.wgsl already computes
// `antialiased_alpha(d) * fill_color.a` as its output alpha, so with
// fill_color.a = 1.0 that alpha channel IS exactly "is this pixel really
// inside the shape", independent of whatever color ended up in mask_tex's
// RGB channels (never read here).
//
// Same fullscreen-quad vertex technique as composite.wgsl (layer
// compositing) — not a new pattern, this file just samples two textures
// and multiplies their alphas instead of one texture scaled by a uniform
// opacity.

@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var mask_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

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
    // V flipped vs. clip-space Y, same reason composite.wgsl's is: texture
    // V grows downward, clip-space Y grows upward.
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
    let color = textureSample(color_tex, samp, input.uv);
    let mask  = textureSample(mask_tex, samp, input.uv);
    return vec4<f32>(color.rgb, color.a * mask.a);
}
