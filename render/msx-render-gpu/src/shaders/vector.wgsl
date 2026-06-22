// render/msx-render-gpu/src/shaders/vector.wgsl
// Position + color passthrough — vertices arrive already in clip space
// (transformed on the CPU side in vector.rs::to_clip_space) and already
// carrying their resolved fill/stroke color, so there's nothing left for
// either stage to compute.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(input.position, 0.0, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Straight (non-premultiplied) alpha — the pipeline's blend state is
    // standard ALPHA_BLENDING to match, same compositing rule every other
    // MSX renderer uses.
    return input.color;
}
