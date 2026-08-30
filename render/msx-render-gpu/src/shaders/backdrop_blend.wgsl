// render/msx-render-gpu/src/shaders/backdrop_blend.wgsl
//
// The non-Normal half of Layer blend-mode compositing — see layer.rs's
// module doc for why Normal keeps its own separate, unchanged fast path
// (fixed-function PREMULTIPLIED_ALPHA_BLENDING, composite.wgsl) rather
// than being routed through here too.
//
// ## Where this formula comes from
//
// `msx-render-core::blend::composite` (already real-adapter-independent
// and unit-tested there) is the ground truth, expressed in STRAIGHT
// (non-premultiplied) color:
//
//   ao      = as + ab*(1-as)
//   blended = B(cb, cs)                          // the blend function, e.g. multiply
//   premixed = (1-ab)*cs + ab*blended
//   mix      = (1 - as/ao)*cb + (as/ao)*premixed
//   result   = (mix, ao)                          // straight rgb, alpha
//
// Every texture this shader reads is premultiplied, not straight (same
// invariant composite.wgsl documents and relies on) — re-deriving the
// above in premultiplied terms (Cb = cb*ab, Cs = cs*as, substituting and
// multiplying `mix` through by `ao`, then simplifying `ao - as =
// ab*(1-as)`) gives:
//
//   Result_premul = (1-as)*Cb + (1-ab)*Cs + as*ab*B(cb,cs)
//   Result_a      = as + ab*(1-as)
//
// where `cb = Cb/ab` and `cs = Cs/as` are un-premultiplied ONLY for
// feeding `B`, everywhere else stays premultiplied. Sanity check: for
// `B(cb,cs) = cs` (Normal), this collapses to `Result_premul = (1-as)*Cb
// + Cs` — exactly the standard premultiplied "over" operator, and
// exactly what composite.wgsl's PREMULTIPLIED_ALPHA_BLENDING pipeline
// state already computes in hardware. That agreement on the one case
// both paths can be checked against is the load-bearing confidence check
// here, since neither path can be run against a real GPU adapter from
// this sandbox — see layer.rs's own module doc for the toolchain reason.
//
// `Cs`/`as` below are already opacity-scaled by the time they reach the
// formula (see `fs_main`) — same convention composite.wgsl uses: scaling
// premultiplied rgb AND a together by opacity is equivalent to
// attenuating straight alpha while leaving straight color unchanged.

@group(0) @binding(0) var backdrop_texture: texture_2d<f32>;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

// See layer.rs's `BlendParams` for why this specific field order/padding
// was chosen (`vec2<f32>` padding, not `vec3`) — deliberately sidesteps
// the exact WGSL-alignment pitfall `CompositeParams` (composite.wgsl)
// already ran into once with a `vec3` pad field.
struct BlendParams {
    opacity: f32,
    blend_mode: u32,
    _pad: vec2<f32>,
};
@group(0) @binding(3) var<uniform> params: BlendParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Identical full-screen-triangle-pair setup to composite.wgsl's own
// vs_main, including the same V-flip (texture-space V grows downward,
// clip space Y grows upward) — not reused via an #include (WGSL has
// none), just kept byte-for-byte identical on purpose.
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 0.0),
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

// Mirrors `hard_light` in core/msx-render-core/src/blend.rs exactly:
// `if cs <= 0.5 { multiply(cb, 2*cs) } else { screen(cb, 2*cs-1) }`,
// inlined rather than calling separate multiply/screen functions since
// WGSL has no operator overloading to keep them one-liners the way the
// Rust versions are.
fn hard_light(cb: f32, cs: f32) -> f32 {
    if (cs <= 0.5) {
        return cb * (2.0 * cs);
    }
    let s = 2.0 * cs - 1.0;
    return cb + s - cb * s;
}

// Mirrors `soft_light` in blend.rs exactly, including its own
// `cb <= 0.25` split for the `d` term.
fn soft_light(cb: f32, cs: f32) -> f32 {
    if (cs <= 0.5) {
        return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
    }
    var d: f32;
    if (cb <= 0.25) {
        d = ((16.0 * cb - 12.0) * cb + 4.0) * cb;
    } else {
        d = sqrt(cb);
    }
    return cb + (2.0 * cs - 1.0) * (d - cb);
}

// `mode` values match `msx_ast::BlendMode`'s own `#[repr(u8)]`
// discriminants exactly (`layer.rs` passes `layer.blend_mode as u32`
// straight through, no remapping table) — Normal (0) is never actually
// routed to this shader (see layer.rs's `composite`), so `default`
// falling back to plain `cs` only matters if that invariant is ever
// broken, not a case this shader is expected to hit in practice.
fn blend_channel(cb: f32, cs: f32, mode: u32) -> f32 {
    switch mode {
        case 1u: { return cb * cs; }                         // Multiply
        case 2u: { return cb + cs - cb * cs; }                // Screen
        case 3u: { return hard_light(cs, cb); }               // Overlay = HardLight(Cs,Cb)
        case 4u: { return min(cb + cs, 1.0); }                // Add
        case 5u: { return soft_light(cb, cs); }               // SoftLight
        case 6u: { return hard_light(cb, cs); }               // HardLight
        case 7u: { return abs(cb - cs); }                     // Difference
        case 8u: { return cb + cs - 2.0 * cb * cs; }          // Exclusion
        case 9u: { return min(cb, cs); }                      // Darken
        case 10u: { return max(cb, cs); }                     // Lighten
        case 11u: { return max(cb - cs, 0.0); }               // Subtract
        case 12u: {                                           // Divide
            if (cs <= 0.0) { return 1.0; }
            return min(cb / cs, 1.0);
        }
        default: { return cs; }
    }
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let backdrop = textureSample(backdrop_texture, tex_sampler, input.uv);
    let raw_source = textureSample(source_texture, tex_sampler, input.uv);

    // Same opacity convention as composite.wgsl: scale premultiplied rgb
    // AND a together, equivalent to attenuating straight alpha while
    // leaving straight color unchanged.
    let source = raw_source * params.opacity;

    let ab = backdrop.a;
    let asrc = source.a;

    if (asrc <= 0.0) {
        // A fully-transparent source contributes nothing — the general
        // formula below degrades to exactly `backdrop` in this limit
        // too (asrc*ab*blended vanishes, (1-asrc)*Cb -> Cb, (1-ab)*Cs ->
        // 0), this is just the cheap, exact-in-the-limit early exit that
        // also sidesteps dividing by a near-zero `asrc` for `cs` below.
        return backdrop;
    }

    // `max(_, 0.0001)` rather than an exact zero-check: guards the
    // divide without a branch. Wherever the true denominator is that
    // small, the un-premultiplied value it produces is immediately
    // scaled back down by that same near-zero factor in the final blend
    // term (`asrc*ab*blended`) or isn't used at all (`cb` only appears
    // there), so the clamp can't meaningfully perturb the result.
    let cs = source.rgb / max(asrc, 0.0001);
    let cb = backdrop.rgb / max(ab, 0.0001);

    let blended = vec3<f32>(
        blend_channel(cb.r, cs.r, params.blend_mode),
        blend_channel(cb.g, cs.g, params.blend_mode),
        blend_channel(cb.b, cs.b, params.blend_mode),
    );

    let result_rgb = (1.0 - asrc) * backdrop.rgb + (1.0 - ab) * source.rgb + asrc * ab * blended;
    let result_a = asrc + ab * (1.0 - asrc);

    return vec4<f32>(result_rgb, result_a);
}
