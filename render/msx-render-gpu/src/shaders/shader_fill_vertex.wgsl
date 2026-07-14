// render/msx-render-gpu/src/shaders/shader_fill_vertex.wgsl
// Shared vertex stage for EVERY `Def::Shader` fill, regardless of which
// user WGSL fragment shader it's paired with in shader.rs's pipeline
// cache. A render pipeline's vertex and fragment stages can come from two
// completely independent `wgpu::ShaderModule`s (confirmed directly
// against wgpu 26.0.1's `RenderPipelineDescriptor`/`VertexState`/
// `FragmentState` — `vertex.module` and `fragment.module` are unrelated
// `&ShaderModule` references, no requirement they share a module) — so
// there's no WGSL source-text splicing here, just two independently
// compiled modules meeting at the pipeline boundary via ordinary
// vertex-to-fragment I/O matching (by `@builtin`/`@location`, not by
// struct name — same reason `vector.wgsl`'s `VertexOutput` and a user
// fragment shader's own bare `@builtin(position)` parameter are
// interface-compatible despite being declared in different modules).
//
// Position arrives already tessellated + transformed into clip space on
// the CPU side (see vector.rs's `fill_path_positions`, the position-only
// sibling of `fill_path` used for every other shape) — this vertex stage
// has nothing left to compute beyond the pass-through.

struct VertexInput {
    @location(0) position: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(input.position, 0.0, 1.0);
    return out;
}
