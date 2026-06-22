// render/msx-render-gpu/src/shaders/sdf.wgsl
// Flattened-tree SDF evaluator. `ops` is the postorder-flattened SdfTree
// (see sdf.rs::flatten_tree); `evaluate_sdf` walks it once per pixel with
// an explicit array-as-stack — WGSL has no recursion, so this stack
// machine is the actual tree-evaluation mechanism, not an optimization.

struct SdfOp {
    kind: u32,
    count: u32,
    param0: f32,
    param1: f32,
    param2: f32,
    param3: f32,
    param4: f32,
    param5: f32,
};

struct SdfParams {
    inv_row0: vec4<f32>,
    inv_row1: vec4<f32>,
    fill_color: vec4<f32>,
    stroke_color: vec4<f32>,
    stroke_width: f32,
    has_stroke: f32,
    op_count: u32,
    _pad: f32,
};

@group(0) @binding(0) var<storage, read> ops: array<SdfOp>;
@group(0) @binding(1) var<uniform> params: SdfParams;

const OP_CIRCLE: u32 = 0u;
const OP_BOX: u32 = 1u;
const OP_LINE: u32 = 2u;
const OP_RING: u32 = 3u;
const OP_ARC: u32 = 4u;
const OP_UNION: u32 = 5u;
const OP_SMOOTH_UNION: u32 = 6u;
const OP_SUBTRACT: u32 = 7u;
const OP_SMOOTH_SUBTRACT: u32 = 8u;
const OP_INTERSECT: u32 = 9u;
const OP_SMOOTH_INTERSECT: u32 = 10u;
const OP_OFFSET: u32 = 11u;

fn sd_circle(p: vec2<f32>, cx: f32, cy: f32, r: f32) -> f32 {
    return distance(p, vec2<f32>(cx, cy)) - r;
}

fn sd_box(p: vec2<f32>, x: f32, y: f32, w: f32, h: f32, corner: f32) -> f32 {
    let half = vec2<f32>(w, h) * 0.5;
    let center = vec2<f32>(x, y) + half;
    let q = abs(p - center) - half + vec2<f32>(corner, corner);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - corner;
}

fn sd_segment(p: vec2<f32>, x1: f32, y1: f32, x2: f32, y2: f32, thickness: f32) -> f32 {
    let a = vec2<f32>(x1, y1);
    let b = vec2<f32>(x2, y2);
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - thickness * 0.5;
}

fn sd_ring(p: vec2<f32>, cx: f32, cy: f32, r: f32, thickness: f32) -> f32 {
    return abs(distance(p, vec2<f32>(cx, cy)) - r) - thickness * 0.5;
}

fn angle_in_range(theta: f32, start: f32, end: f32) -> bool {
    let tau = 6.283185307;
    let t = theta - tau * floor(theta / tau);
    let s = start - tau * floor(start / tau);
    let e = end - tau * floor(end / tau);
    if (s <= e) {
        return t >= s && t <= e;
    }
    return t >= s || t <= e;
}

fn sd_arc(p: vec2<f32>, cx: f32, cy: f32, r: f32, angle_start: f32, angle_end: f32, thickness: f32) -> f32 {
    let center = vec2<f32>(cx, cy);
    let rel = p - center;
    let theta = atan2(rel.y, rel.x);
    if (angle_in_range(theta, angle_start, angle_end)) {
        return abs(length(rel) - r) - thickness * 0.5;
    }
    let cap_a = center + vec2<f32>(cos(angle_start), sin(angle_start)) * r;
    let cap_b = center + vec2<f32>(cos(angle_end), sin(angle_end)) * r;
    return min(distance(p, cap_a), distance(p, cap_b)) - thickness * 0.5;
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn smax(a: f32, b: f32, k: f32) -> f32 {
    return -smin(-a, -b, k);
}

fn evaluate_sdf(p: vec2<f32>) -> f32 {
    var stack: array<f32, 32>;
    var sp: u32 = 0u;

    for (var i: u32 = 0u; i < params.op_count; i = i + 1u) {
        let o = ops[i];

        if (o.kind == OP_CIRCLE) {
            stack[sp] = sd_circle(p, o.param0, o.param1, o.param2);
            sp = sp + 1u;
        } else if (o.kind == OP_BOX) {
            stack[sp] = sd_box(p, o.param0, o.param1, o.param2, o.param3, o.param4);
            sp = sp + 1u;
        } else if (o.kind == OP_LINE) {
            stack[sp] = sd_segment(p, o.param0, o.param1, o.param2, o.param3, o.param4);
            sp = sp + 1u;
        } else if (o.kind == OP_RING) {
            stack[sp] = sd_ring(p, o.param0, o.param1, o.param2, o.param3);
            sp = sp + 1u;
        } else if (o.kind == OP_ARC) {
            stack[sp] = sd_arc(p, o.param0, o.param1, o.param2, o.param3, o.param4, o.param5);
            sp = sp + 1u;
        } else if (o.kind == OP_UNION) {
            var result: f32 = 3.4e38;
            for (var k: u32 = 0u; k < o.count; k = k + 1u) {
                sp = sp - 1u;
                result = min(result, stack[sp]);
            }
            stack[sp] = result;
            sp = sp + 1u;
        } else if (o.kind == OP_SMOOTH_UNION) {
            var result: f32 = 3.4e38;
            for (var k: u32 = 0u; k < o.count; k = k + 1u) {
                sp = sp - 1u;
                result = smin(result, stack[sp], o.param0);
            }
            stack[sp] = result;
            sp = sp + 1u;
        } else if (o.kind == OP_SUBTRACT) {
            let b = stack[sp - 1u];
            let a = stack[sp - 2u];
            sp = sp - 2u;
            stack[sp] = max(a, -b);
            sp = sp + 1u;
        } else if (o.kind == OP_SMOOTH_SUBTRACT) {
            let b = stack[sp - 1u];
            let a = stack[sp - 2u];
            sp = sp - 2u;
            stack[sp] = smax(a, -b, o.param0);
            sp = sp + 1u;
        } else if (o.kind == OP_INTERSECT) {
            let b = stack[sp - 1u];
            let a = stack[sp - 2u];
            sp = sp - 2u;
            stack[sp] = max(a, b);
            sp = sp + 1u;
        } else if (o.kind == OP_SMOOTH_INTERSECT) {
            let b = stack[sp - 1u];
            let a = stack[sp - 2u];
            sp = sp - 2u;
            stack[sp] = smax(a, b, o.param0);
            sp = sp + 1u;
        } else if (o.kind == OP_OFFSET) {
            sp = sp - 1u;
            stack[sp] = stack[sp] - o.param0;
            sp = sp + 1u;
        }
    }

    return stack[sp - 1u];
}

struct VertexInput {
    @location(0) clip_position: vec2<f32>,
    @location(1) screen_position: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) screen_position: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(input.clip_position, 0.0, 1.0);
    out.screen_position = input.screen_position;
    return out;
}

fn antialiased_alpha(d: f32) -> f32 {
    return clamp(1.0 - (d + 1.0) * 0.5, 0.0, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let local = vec2<f32>(
        params.inv_row0.x * input.screen_position.x + params.inv_row0.y * input.screen_position.y + params.inv_row0.z,
        params.inv_row1.x * input.screen_position.x + params.inv_row1.y * input.screen_position.y + params.inv_row1.z,
    );

    let d = evaluate_sdf(local);

    if (params.has_stroke > 0.5) {
        let band = abs(d) - params.stroke_width * 0.5;
        if (band <= 1.0) {
            return vec4<f32>(params.stroke_color.rgb, antialiased_alpha(band) * params.stroke_color.a);
        }
    }

    if (d <= 1.0) {
        return vec4<f32>(params.fill_color.rgb, antialiased_alpha(d) * params.fill_color.a);
    }

    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
