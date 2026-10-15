// render/msx-render-gpu/src/shaders/vector.wgsl
// Vertices arrive already in clip space (transformed on the CPU side in
// vector.rs::to_clip_space). A flat vertex carries its resolved color. A
// gradient vertex carries `uv`, its position in objectBoundingBox units,
// and `grad`, an index into the gradient storage buffer; the fragment
// stage turns those into a color.
//
// GradientDesc must stay byte-for-byte in step with `GradientDesc` in
// gradient.rs (688 bytes, checked by a unit test there).

struct GradientDesc {
    kind: u32,
    count: u32,
    pad0: u32,
    pad1: u32,
    a: vec4<f32>,
    b: vec4<f32>,
    colors: array<vec4<f32>, 32>,
    offsets: array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<storage, read> gradients: array<GradientDesc>;

const NO_GRADIENT: u32 = 0xffffffffu;
const KIND_LINEAR: u32 = 1u;
const KIND_RADIAL: u32 = 2u;
const KIND_CONIC: u32 = 3u;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) grad: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) grad: u32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(input.position, 0.0, 1.0);
    out.color = input.color;
    out.uv = input.uv;
    out.grad = input.grad;
    return out;
}

fn offset_at(i: u32, k: u32) -> f32 {
    return gradients[i].offsets[k / 4u][k % 4u];
}

// Straight-alpha color for parameter t in 0..1. Twin of `color_at` in
// gradient.rs.
fn ramp(i: u32, t: f32) -> vec4<f32> {
    let n = gradients[i].count;
    if (t <= offset_at(i, 0u)) {
        return gradients[i].colors[0];
    }
    for (var k = 1u; k < n; k = k + 1u) {
        let o1 = offset_at(i, k);
        if (t <= o1) {
            let o0 = offset_at(i, k - 1u);
            let span = o1 - o0;
            var f = 1.0;
            if (span > 1e-6) {
                f = (t - o0) / span;
            }
            return mix(gradients[i].colors[k - 1u], gradients[i].colors[k], f);
        }
    }
    return gradients[i].colors[n - 1u];
}

fn gradient_t(i: u32, uv: vec2<f32>) -> f32 {
    let kind = gradients[i].kind;
    let a = gradients[i].a;
    let b = gradients[i].b;
    if (kind == KIND_LINEAR) {
        let d = a.zw - a.xy;
        return dot(uv - a.xy, d) / dot(d, d);
    }
    if (kind == KIND_RADIAL) {
        // Two-point conical gradient: focal point (radius 0) to the center
        // circle (radius r). Pick the non-negative root of
        // |p - f - t (c - f)| = t r.
        let cd = a.xy - a.zw;
        let pd = uv - a.zw;
        let r = b.x;
        let aa = dot(cd, cd) - r * r;
        let bb = dot(pd, cd);
        let cc = dot(pd, pd);
        let disc = max(bb * bb - aa * cc, 0.0);
        return (bb - sqrt(disc)) / aa;
    }
    if (kind == KIND_CONIC) {
        // Angle is measured in local space, so scale the bounding-box
        // offset back by the box size. 0 degrees is the top, clockwise.
        let delta = (uv - a.xy) * b.xy;
        if (dot(delta, delta) < 1e-12) {
            return 0.0;
        }
        let deg = degrees(atan2(delta.y, delta.x)) + 90.0 - a.z;
        return fract(deg / 360.0);
    }
    return 0.0;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.grad == NO_GRADIENT) {
        // Straight (non-premultiplied) alpha; the pipeline blend state is
        // standard ALPHA_BLENDING to match.
        return input.color;
    }
    let t = clamp(gradient_t(input.grad, input.uv), 0.0, 1.0);
    return ramp(input.grad, t);
}
