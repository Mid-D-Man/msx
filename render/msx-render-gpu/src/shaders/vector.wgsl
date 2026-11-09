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
    spread: u32,
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

// The parameter of gradient `i` at `uv` in `x`, and in `y` 1.0 when the
// gradient paints there or 0.0 when it does not (outside a cone).
fn gradient_t(i: u32, uv: vec2<f32>) -> vec2<f32> {
    let kind = gradients[i].kind;
    let a = gradients[i].a;
    let b = gradients[i].b;
    if (kind == KIND_LINEAR) {
        let d = a.zw - a.xy;
        return vec2<f32>(dot(uv - a.xy, d) / dot(d, d), 1.0);
    }
    if (kind == KIND_RADIAL) {
        // Two-point conical gradient between the focal circle (center f,
        // radius r0 = b.y) at t = 0 and the circle (center c, radius r) at
        // t = 1. Take the largest t whose circle has a positive radius and
        // passes through the point, the rule the canvas and SVG 2 define.
        let r1 = b.x;
        let r0 = b.y;
        let cd = a.xy - a.zw;
        let pd = uv - a.zw;
        let dr = r1 - r0;
        let aa = dot(cd, cd) - dr * dr;
        let bb = dot(pd, cd) + r0 * dr;
        let cc = dot(pd, pd) - r0 * r0;
        if (abs(aa) < 1e-9) {
            // The circles touch: one root, if the point is on the right side.
            if (abs(bb) < 1e-12) {
                return vec2<f32>(0.0, 0.0);
            }
            let t = cc / (2.0 * bb);
            return vec2<f32>(t, select(0.0, 1.0, r0 + t * dr >= 0.0));
        }
        let disc = bb * bb - aa * cc;
        if (disc < 0.0) {
            return vec2<f32>(0.0, 0.0);
        }
        let root = sqrt(disc);
        let t1 = (bb + root) / aa;
        let t2 = (bb - root) / aa;
        let hi = max(t1, t2);
        let lo = min(t1, t2);
        if (r0 + hi * dr >= 0.0) {
            return vec2<f32>(hi, 1.0);
        }
        if (r0 + lo * dr >= 0.0) {
            return vec2<f32>(lo, 1.0);
        }
        return vec2<f32>(0.0, 0.0);
    }
    if (kind == KIND_CONIC) {
        // Angle is measured in local space, so scale the bounding-box
        // offset back by the box size. 0 degrees is the top, clockwise.
        let delta = (uv - a.xy) * b.xy;
        if (dot(delta, delta) < 1e-12) {
            return vec2<f32>(0.0, 1.0);
        }
        let deg = degrees(atan2(delta.y, delta.x)) + 90.0 - a.z;
        return vec2<f32>(fract(deg / 360.0), 1.0);
    }
    return vec2<f32>(0.0, 1.0);
}

// What lies beyond the first and last stop: 0 pad, 1 reflect, 2 repeat.
fn spread_t(mode: u32, t: f32) -> f32 {
    if (mode == 1u) {
        let m = t - 2.0 * floor(t * 0.5);
        return select(m, 2.0 - m, m > 1.0);
    }
    if (mode == 2u) {
        return t - floor(t);
    }
    return clamp(t, 0.0, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.grad == NO_GRADIENT) {
        // Straight (non-premultiplied) alpha; the pipeline blend state is
        // standard ALPHA_BLENDING to match.
        return input.color;
    }
    let g = gradient_t(input.grad, input.uv);
    if (g.y < 0.5) {
        discard;
    }
    let t = spread_t(gradients[input.grad].spread, g.x);
    return ramp(input.grad, t);
}
