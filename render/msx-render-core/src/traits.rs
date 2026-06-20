// render/msx-render-core/src/traits.rs
use msx_ast::Scene;

use crate::tile::RenderTarget;

/// Implemented by every rendering backend (`msx-render-cpu`'s rasterizer,
/// eventually `msx-render-gpu`'s wgpu pipeline) — lets the CLI, the viewer,
/// and tests pick a backend without caring which one they got.
pub trait Renderer {
    fn render(&self, scene: &Scene, target: &mut RenderTarget);
}
