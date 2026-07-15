// examples/shaders/orb_raymarch.wgsl
// Referenced by examples/shader_orb.msx's "orb_1" shader def
// (source_ref = "shaders/orb_raymarch.wgsl", relative to that .msx file).
//
// Everything shader.rs has been tested against so far (plasma.wgsl,
// checkerboard.wgsl) is flat per-pixel math: no loops, no branching on
// distance, one texture-space evaluation per fragment. This one isn't —
// a real sphere-tracing raymarcher, up to MAX_STEPS distance-field
// evaluations per pixel plus 6 more for the central-difference normal at
// the hit point. If msx-render-gpu's pipeline ever mishandles loop-heavy/
// branchy WGSL, this is the shader that will show it, where
// plasma/checkerboard wouldn't.
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
// pack_uniforms' own unit tests cover vec3-then-scalar but not
// vec3-immediately-after-vec3, so this is genuinely new ground for that
// function, not just a new example.
//
// Uniform names/types match shader_orb.msx's declared "uniforms" in
// order: resolution (vec2f), base_color (vec3f), rim_color (vec3f),
// speed (f32) — then the renderer-appended trailing time (f32).

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

// Signed distance to the scene: one sphere at the origin, radius
// breathing slowly so the orb reads as animated even before any surface
// shading or camera motion is applied.
fn map(p: vec3<f32>) -> f32 {
    let radius = 1.0 + 0.08 * sin(u.time * u.speed);
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

    // Camera orbits the origin at a fixed radius/height, driven entirely
    // by time — rotating the ray basis rather than moving a look-at
    // target, so the orb stays centered in frame at every angle.
    let angle = u.time * u.speed * 0.3;
    let cam_pos = vec3<f32>(sin(angle) * 3.0, 0.6, cos(angle) * 3.0);
    let forward = normalize(-cam_pos);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), forward));
    let up = cross(forward, right);
    let ray_dir = normalize(forward * 1.8 + right * uv.x + up * uv.y);

    var t = 0.0;
    var hit = false;
    for (var i = 0; i < MAX_STEPS; i = i + 1) {
        let p = cam_pos + ray_dir * t;
        let d = map(p);
        if (d < SURF_EPS) {
            hit = true;
            break;
        }
        t = t + d;
        if (t > MAX_DIST) {
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

    let p = cam_pos + ray_dir * t;
    let n = calc_normal(p);

    let light_dir = normalize(vec3<f32>(0.5, 0.8, -0.4));
    let diffuse = max(dot(n, light_dir), 0.0);
    let fresnel = pow(1.0 - max(dot(n, -ray_dir), 0.0), 3.0);

    // Slow moving latitude bands, purely procedural (derived from the
    // surface normal + time, no extra uniform needed) so the sphere reads
    // as more than a flat-shaded ball.
    let bands = 0.5 + 0.5 * sin(n.y * 6.0 + u.time * u.speed * 2.0);
    let surface = mix(u.base_color * 0.7, u.base_color * 1.3, bands);

    let lit = surface * (0.15 + 0.85 * diffuse);
    let rim = u.rim_color * fresnel * 1.5;

    return vec4<f32>(lit + rim, 1.0);
}
