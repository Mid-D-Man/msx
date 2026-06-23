// render/msx-render-gpu/src/shaders/splat.wgsl
struct Instance {
    @location(0) center: vec2<f32>,
    @location(1) half_extents: vec2<f32>,
    @location(2) rotation: f32,
    @location(3) sigma: vec2<f32>,
    @location(4) color: vec4<f32>,
};

struct CanvasParams {
    width: f32,
    height: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> canvas: CanvasParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local_offset: vec2<f32>,
    @location(1) sigma: vec2<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, instance: Instance) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[vertex_index];
    // `local` is the pre-rotation offset from center — this is exactly
    // the (lx, ly) the gaussian formula needs (see msx-splat::gaussian's
    // derivation: it rotates a world-space offset BACK into local-aligned
    // axes; here we go the other direction, local -> world, so `local`
    // itself is already in the right frame without any further rotation
    // in the fragment shader).
    let local = corner * instance.half_extents;

    let cos_r = cos(instance.rotation);
    let sin_r = sin(instance.rotation);
    let rotated = vec2<f32>(
        local.x * cos_r - local.y * sin_r,
        local.x * sin_r + local.y * cos_r,
    );
    let world = instance.center + rotated;

    var out: VertexOutput;
    out.position = vec4<f32>(
        (world.x / canvas.width) * 2.0 - 1.0,
        1.0 - (world.y / canvas.height) * 2.0,
        0.0,
        1.0,
    );
    out.local_offset = local;
    out.sigma = instance.sigma;
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let lx = input.local_offset.x;
    let ly = input.local_offset.y;
    let sx = input.sigma.x;
    let sy = input.sigma.y;
    let gaussian = exp(-(lx * lx) / (2.0 * sx * sx) - (ly * ly) / (2.0 * sy * sy));
    return vec4<f32>(input.color.rgb, gaussian * input.color.a);
}
