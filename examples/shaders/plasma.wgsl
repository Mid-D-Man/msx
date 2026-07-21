// examples/shaders/plasma.wgsl
// Referenced by examples/shader_placeholder.msx's "plasma_1" shader def
// (source_ref = "shaders/plasma.wgsl", relative to that .msx file).
//
// Real WGSL execution as of this session's GPU work (msx-render-gpu's
// shader.rs) — the uniform names/types here match the "uniforms" declared
// in shader_placeholder.msx's def exactly: speed (f32), resolution
// (vec2f), then the renderer-appended trailing time (f32).
//
// LOOP-CLOSURE FIX (this pass): the original version of this file used
// five different fractional frequency coefficients on `time * speed`
// (1, 0.5, 0.3, 0.2, 0.15). None of those share a short common period —
// worked out by hand, the true loop period was ~84 seconds, so any
// `animate-gpu --duration 4` export (what pages.yml actually runs) showed
// a visible pop at the wrap, every time, no matter which 4 seconds got
// sampled. Fixed by rounding `speed` to the nearest whole number of
// LOOP_SECONDS-length cycles, then expressing every OTHER frequency as a
// small-integer multiple of that base rate (2, 3, 4, 5 instead of 0.5,
// 0.3, 0.2, 0.15) — integer × integer stays an integer multiple of the
// base angular frequency, so the whole function is now exactly periodic
// over LOOP_SECONDS by construction, not by coincidence. This holds for
// ANY `speed` value a .msx file declares (1.5 here rounds to 2) — nobody
// authoring a shader def has to know this convention exists to not break
// it.
struct Uniforms {
    speed:      f32,
    resolution: vec2<f32>,
    time:       f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

// Loop length, in seconds, this shader is designed to close cleanly
// over — must match whatever --duration animate-gpu actually samples
// this shader with. pages.yml currently hardcodes 4 for every
// shader-using example (see that file's own comment on why); change this
// only in lockstep with that.
const LOOP_SECONDS: f32 = 4.0;
const TAU: f32 = 6.283185307;

// Classic three-wave plasma: three offset sine fields summed and mapped
// through a small color ramp. Cheap, has no dependencies, and reads as
// obviously animated once something actually drives `u.time` — good
// properties for a first real shader to exercise the pipeline with, since
// a mistake in uniform wiring is easy to spot on screen.
//
// `t` here is already loop-locked (an integer multiple of
// `TAU / LOOP_SECONDS`, computed once in fs_main) — every coefficient
// below is a small integer specifically so multiplying by an integer
// keeps every term locked to the same base frequency, guaranteeing the
// whole sum returns to its start value at t's own loop point.
fn plasma(uv: vec2<f32>, t: f32) -> f32 {
    let p1 = sin(uv.x * 10.0 + t);
    let p2 = sin(10.0 * (uv.x * sin(t * 2.0) + uv.y * cos(t * 3.0)) + t);
    let cx = uv.x + 0.5 * sin(t * 4.0);
    let cy = uv.y + 0.5 * cos(t * 5.0);
    let p3 = sin(sqrt(100.0 * (cx * cx + cy * cy) + 1.0) + t);
    return (p1 + p2 + p3) / 3.0;
}

fn palette(v: f32) -> vec3<f32> {
    // Maps [-1, 1] to a small purple -> cyan -> white ramp — matches
    // shader_placeholder.msx's fallback_color (#6b46ff) as the low end,
    // so the flat-fallback render (CPU/SVG) and the real GPU one are at
    // least in the same family rather than jarringly different.
    let t = v * 0.5 + 0.5;
    let low  = vec3<f32>(0.420, 0.271, 1.0);   // #6b46ff
    let mid  = vec3<f32>(0.176, 0.831, 1.0);   // #2dd4ff
    let high = vec3<f32>(1.0, 1.0, 1.0);
    if (t < 0.5) {
        return mix(low, mid, t * 2.0);
    }
    return mix(mid, high, (t - 0.5) * 2.0);
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag_coord.xy / u.resolution;
    // Nearest whole number of loop cycles — see LOOP_SECONDS' doc comment
    // above. round() uses ties-to-even, so speed = 1.5 (this shader's
    // current declared value) rounds to 2, not 1.
    let cycles = round(u.speed);
    let t = u.time * cycles * (TAU / LOOP_SECONDS);
    let v = plasma(uv, t);
    return vec4<f32>(palette(v), 1.0);
}
