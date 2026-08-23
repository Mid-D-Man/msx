// render/msx-render-gpu/src/shaders/image.wgsl
// Draws one Element::Image as a textured quad.
//
// Unlike composite.wgsl (which samples a PREMULTIPLIED layer buffer via a
// hardcoded fullscreen-quad vertex shader), this one:
//   1. Takes REAL vertex-buffer input (position + uv) — the quad's four
//      corners are computed on the CPU (image.rs's `quad_vertices`), the
//      same "transform on the CPU, upload already-positioned clip-space
//      coordinates" approach vector.rs's own vertex_from_point already
//      uses, not a uniform-matrix multiply in the vertex shader.
//   2. Samples a STRAIGHT-alpha texture (the `image` crate's decoded
//      output, uploaded as-is, never premultiplied) — so this pairs with
//      ALPHA_BLENDING in image.rs's pipeline creation, not
//      PREMULTIPLIED_ALPHA_BLENDING the way composite.wgsl's does. See
//      that file's own comment for the premultiplied case this
//      deliberately is NOT; mixing the two blend modes up is exactly the
//      bug class that already bit this crate once for layer compositing.

@group(0) @binding(0) var image_texture: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

struct ImageParams {
    opacity: f32,
    _pad: vec3<f32>,
};
@group(0) @binding(2) var<uniform> params: ImageParams;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Already in clip space by the time it reaches here — no matrix
    // multiply in this shader at all, see this file's own header comment.
    out.clip_position = vec4<f32>(input.position, 0.0, 1.0);
    out.uv = input.uv;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var sample = textureSample(image_texture, image_sampler, input.uv);
    // STRAIGHT alpha: only the alpha channel scales with opacity, rgb is
    // left as the texture's own true color — ALPHA_BLENDING's fixed-
    // function blend equation is what actually applies that alpha to the
    // color during compositing. Scaling rgb here too (composite.wgsl's
    // own approach) is only correct for an ALREADY-premultiplied source,
    // which this texture is not.
    sample.a = sample.a * params.opacity;
    return sample;
}
