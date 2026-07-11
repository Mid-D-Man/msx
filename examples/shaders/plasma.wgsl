// examples/shaders/plasma.wgsl
// Referenced by examples/shader_placeholder.msx's "plasma_1" shader def
// (source_ref = "shaders/plasma.wgsl", relative to that .msx file).
//
// Not executed by anything yet — msx-render-gpu doesn't have a
// WGSL-executing path for user shader defs today, only the built-in
// vector/SDF pipelines under render/msx-render-gpu/src/shaders/. Every
// renderer currently paints this shader's shapes flat with its declared
// fallback_color instead (see ShaderDef's doc comment in
// core/msx-ast/src/gradient.rs for the full story and the migration path
// once DixScript grows real inline shader source support).
//
// This file exists so `source_ref` resolves to something real — msx-cli
// compile validates that at compile time (see validate_shader_refs in
// apps/msx-cli/src/main.rs) — and so the eventual WGSL-executing renderer
// has a real, working example to test against rather than starting from
// nothing. The uniform names/types here match the "uniforms" declared in
// shader_placeholder.msx's def exactly: speed (f32), resolution (vec2f).

struct Uniforms {
    speed:      f32,
    resolution: vec2<f32>,
    time:       f32, // fed by the eventual renderer, not part of the def's
                      // own declared uniforms — every shader gets a running
                      // clock for free, same convention as shadertoy/isf.
}

@group(0) @binding(0) var<uniform> u: Uniforms;

// Classic three-wave plasma: three offset sine fields summed and mapped
// through a small color ramp. Cheap, has no dependencies, and reads as
// obviously animated once something actually drives `u.time` — good
// properties for a first real shader to exercise the eventual pipeline
// with, since a mistake in uniform wiring is easy to spot on screen.
fn plasma(uv: vec2<f32>, t: f32) -> f32 {
    let p1 = sin(uv.x * 10.0 + t);
    let p2 = sin(10.0 * (uv.x * sin(t * 0.5) + uv.y * cos(t * 0.3)) + t);
    let cx = uv.x + 0.5 * sin(t * 0.2);
    let cy = uv.y + 0.5 * cos(t * 0.15);
    let p3 = sin(sqrt(100.0 * (cx * cx + cy * cy) + 1.0) + t);
    return (p1 + p2 + p3) / 3.0;
}

fn palette(v: f32) -> vec3<f32> {
    // Maps [-1, 1] to a small purple -> cyan -> white ramp — matches
    // shader_placeholder.msx's fallback_color (#6b46ff) as the low end,
    // so the flat-fallback render and the eventual real one are at least
    // in the same family rather than jarringly different.
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
    let v = plasma(uv, u.time * u.speed);
    return vec4<f32>(palette(v), 1.0);
}
