// examples/shaders/checkerboard.wgsl
// Referenced by examples/shader_checkerboard.msx's "checker_1" shader def
// (source_ref = "shaders/checkerboard.wgsl", relative to that .msx file).
//
// Deliberately the simplest possible real shader test: two flat colors,
// hard cell boundaries, no interpolation anywhere in the color math. If a
// future change to msx-render-gpu's UV mapping, vertex-to-fragment
// interpolation, or uniform packing ever goes subtly wrong, a checkerboard
// shows it immediately and unambiguously — a wrong aspect ratio stretches
// the squares, a wrong UV origin shifts the whole grid off-center, and a
// uniform-offset bug (the exact class shader.rs's pack_uniforms tests
// already guard against, see that file's module doc) shows up as visibly
// wrong cell sizes rather than a plausible-looking wrong color. This file
// is meant as a diagnostic, not just a second decorative example.
//
// Uniform names/types here match the "uniforms" declared in
// shader_checkerboard.msx's def exactly, in order: cells (f32),
// resolution (vec2f), color_a (vec3f), color_b (vec3f) — then the
// renderer-appended trailing time (f32), used here for a slow diagonal
// scroll so this file doubles as a live test of the time uniform, not
// just a static pattern.

struct Uniforms {
    cells:      f32,
    resolution: vec2<f32>,
    color_a:    vec3<f32>,
    color_b:    vec3<f32>,
    time:       f32, // fed by the renderer, not part of the def's own
                      // declared uniforms — see shader.rs's module doc.
}

@group(0) @binding(0) var<uniform> u: Uniforms;

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    // Normalize by the SHORTER edge, not width/height independently, so
    // "cells" means the same physical square size on both axes — using
    // resolution.xy directly here would stretch squares on any non-square
    // canvas, exactly the aspect-ratio bug class this shader exists to
    // catch (see the file header).
    let short_edge = min(u.resolution.x, u.resolution.y);
    var uv = frag_coord.xy / short_edge;

    // Slow diagonal scroll driven entirely by the renderer's free-running
    // clock — small (0.15 / 0.1 units per second) so the grid reads as
    // alive without becoming hard to inspect frame to frame.
    uv += vec2<f32>(u.time * 0.15, u.time * 0.1);

    // Integer cell coordinates + bitwise-style parity check via i32 `%`
    // rather than f32 `%` on (cell.x + cell.y) directly — avoids any
    // doubt about floating-point modulo semantics for a value that only
    // ever needs to be "even or odd".
    let cx = i32(floor(uv.x * u.cells));
    let cy = i32(floor(uv.y * u.cells));
    let parity = (cx + cy) % 2;

    let color = select(u.color_a, u.color_b, parity == 1);
    return vec4<f32>(color, 1.0);
}
