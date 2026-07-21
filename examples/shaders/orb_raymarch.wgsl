// examples/shaders/orb_raymarch.wgsl
// Referenced by examples/shader_orb.msx's "orb_1" shader def
// (source_ref = "shaders/orb_raymarch.wgsl", relative to that .msx file).
//
// Everything shader.rs was originally tested against (plasma.wgsl,
// checkerboard.wgsl) is flat per-pixel math: no loops, no branching on
// distance, one texture-space evaluation per fragment. This one isn't —
// a real sphere-tracing raymarcher, up to MAX_STEPS distance-field
// evaluations per pixel plus 6 more for the central-difference normal at
// the hit point.
//
// It also exercises a uniform-layout case the other two examples don't:
// TWO consecutive vec3<f32> fields (base_color, rim_color) after a vec2
// (resolution). vec3's WGSL alignment is 16, not 12 — so base_color
// starts at offset 16 (padded up from resolution's own end at offset 8),
// and rim_color starts at offset 32 (padded up from base_color's end at
// offset 28 — NOT simply "28 rounded up as if vec3 were a 12-byte step").
// Worked by hand before writing shader_orb.msx's uniforms list:
//   resolution  0-8
//   base_color  16-28  (8-15 is padding)
//   rim_color   32-44  (28-31 is padding)
//   speed       44-48
//   time        48-52
//   struct size rounds up to the max member alignment (16) -> 64 bytes.
//
// Uniform names/types match shader_orb.msx's declared "uniforms" in
// order: resolution (vec2f), base_color (vec3f), rim_color (vec3f),
// speed (f32) — then the renderer-appended trailing time (f32).
//
// LOOP-CLOSURE FIX (this pass): originally the radius pulse (coefficient
// 1), camera orbit (coefficient 0.3), and surface bands (coefficient 2)
// all moved at unrelated rates relative to `time * speed` — the camera
// alone needed ~21 seconds for one full lap. No short `animate-gpu
// --duration` could close all three at once. Fixed the same way
// plasma.wgsl was: round `speed` to the nearest whole number of
// LOOP_SECONDS-length cycles via `loop_time()`, then express every
// frequency as a small integer multiple of that (camera orbit now
// completes exactly 1 full lap per loop, bands complete 2) instead of an
// arbitrary fraction. A camera doing one full clean rotation per loop
// also just reads as more intentional than a partial creep did — closer
// to a product-shot turntable than an accident of the old timing.

struct Uniforms {
    resolution: vec2<f32>,
    base_color: vec3<f32>,
    rim_color:  vec3<f32>,
    speed:      f32,
    time:       f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

const MAX_STEPS: i32 = 80;
const MAX_DIST: f32 = 20.0;
const SURF_EPS: f32 = 0.001;

// Loop length, in seconds, this shader is designed to close cleanly
// over — must match whatever --duration animate-gpu actually samples
// this shader with. pages.yml currently hardcodes 4 for every
// shader-using example; change this only in lockstep with that.
const LOOP_SECONDS: f32 = 4.0;
const TAU: f32 = 6.283185307;

// Nearest whole number of loop cycles, times the base angular frequency
// for one full loop — see LOOP_SECONDS' doc comment above. Called from
// both `map` and `fs_main` rather than threaded through as a parameter,
// matching this file's existing convention of reading `u` directly
// wherever needed instead of passing uniforms down through every
// function signature.
fn loop_time() -> f32 {
    let cycles = round(u.speed);
    return u.time * cycles * (TAU / LOOP_SECONDS);
}

// Signed distance to the scene: one sphere at the origin, radius
// breathing slowly so the orb reads as animated even before any surface
// shading or camera motion is applied.
fn map(p: vec3<f32>) -> f32 {
    let radius = 1.0 + 0.08 * sin(loop_time());
    return length(p) - radius;
}

// Central-difference surface normal — six extra `map` evaluations per
// hit, on top of the raymarch loop itself, exactly the kind of
// evaluation-count this shader exists to stress (see file header).
fn calc_normal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.0015, 0.0);
    return normalize(vec3<f32>(
        map(p + e.xyy) - map(p - e.xyy),
        map(p + e.yxy) - map(p - e.yxy),
        map(p + e.yyx) - map(p - e.yyx),
    ));
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    // Centered, aspect-corrected UV — same normalize-by-shorter-edge
    // approach as checkerboard.wgsl, so the sphere stays round instead of
    // squashing on a non-square canvas.
    let short_edge = min(u.resolution.x, u.resolution.y);
    let uv = (frag_coord.xy - 0.5 * u.resolution) / short_edge;

    let t = loop_time();

    // Camera orbits the origin at a fixed radius/height, one full lap per
    // loop (see file header) — rotating the ray basis rather than moving
    // a look-at target, so the orb stays centered in frame at every
    // angle.
    let angle = t;
    let cam_pos = vec3<f32>(sin(angle) * 3.0, 0.6, cos(angle) * 3.0);
    let forward = normalize(-cam_pos);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), forward));
    let up = cross(forward, right);
    let ray_dir = normalize(forward * 1.8 + right * uv.x + up * uv.y);

    var dist = 0.0;
    var hit = false;
    for (var i = 0; i < MAX_STEPS; i = i + 1) {
        let p = cam_pos + ray_dir * dist;
        let d = map(p);
        if (d < SURF_EPS) {
            hit = true;
            break;
        }
        dist = dist + d;
        if (dist > MAX_DIST) {
            break;
        }
    }

    if (!hit) {
        // Background: a soft vertical gradient so the orb reads as
        // floating in space rather than pasted on a flat fill.
        let bg_t = clamp(uv.y * 0.5 + 0.5, 0.0, 1.0);
        let bg = mix(vec3<f32>(0.02, 0.02, 0.05), vec3<f32>(0.07, 0.05, 0.12), bg_t);
        return vec4<f32>(bg, 1.0);
    }

    let p = cam_pos + ray_dir * dist;
    let n = calc_normal(p);

    let light_dir = normalize(vec3<f32>(0.5, 0.8, -0.4));
    let diffuse = max(dot(n, light_dir), 0.0);
    let fresnel = pow(1.0 - max(dot(n, -ray_dir), 0.0), 3.0);

    // Slow moving latitude bands, two cycles per loop — purely
    // procedural (derived from the surface normal + t, no extra uniform
    // needed) so the sphere reads as more than a flat-shaded ball.
    let bands = 0.5 + 0.5 * sin(n.y * 6.0 + t * 2.0);
    let surface = mix(u.base_color * 0.7, u.base_color * 1.3, bands);

    let lit = surface * (0.15 + 0.85 * diffuse);
    let rim = u.rim_color * fresnel * 1.5;

    return vec4<f32>(lit + rim, 1.0);
}
