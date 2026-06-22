// render/msx-render-gpu/src/lib.rs
//! GPU renderer — wgpu-backed, offscreen-only (no window `Surface`; that's
//! `apps/msx-viewer`'s job once it exists).
//!
//! ## Honesty about scope and risk
//!
//! This is the highest API-risk crate in the project so far, by a wide
//! margin. wgpu ships a breaking release roughly every three months, and
//! the project's original plan pinned `wgpu = "0.19"` — current is
//! **29.0.3**, more than two years and a complete versioning-scheme change
//! later (`0.x` → plain integers). Every signature in `context.rs`/
//! `target.rs` was checked against current (~April 2026) tutorials and
//! docs rather than pulled from training-data memory, but "checked against
//! docs" still isn't "compiled" — the `DeviceDescriptor`/`InstanceDescriptor`
//! field lists are the most likely place for drift, since those are
//! exactly the structs wgpu tends to grow fields on release-over-release.
//!
//! This pass proves the full round trip end to end — instance → adapter →
//! device → offscreen texture → clear to the canvas background color →
//! read back to a `RenderTarget` — without drawing any geometry yet.
//! Device setup and GPU→CPU readback are their own substantial chunk,
//! worth nailing down before lyon tessellation and WGSL shaders for
//! vector/SDF/splat land on top.

mod context;
mod target;

pub use context::GpuContext;
pub use target::OffscreenTarget;

use msx_ast::Scene;
use msx_render_core::{RenderTarget, Renderer};

pub struct GpuRenderer {
    context: GpuContext,
}

impl GpuRenderer {
    pub fn new() -> Result<Self, String> {
        Ok(GpuRenderer { context: GpuContext::new()? })
    }
}

impl Renderer for GpuRenderer {
    fn render(&self, scene: &Scene, target: &mut RenderTarget) {
        let width = scene.canvas.width.round().max(1.0) as u32;
        let height = scene.canvas.height.round().max(1.0) as u32;

        let offscreen = OffscreenTarget::new(&self.context.device, width, height);

        let bg = scene.canvas.background;
        let clear_color = wgpu::Color {
            r: bg.r as f64 / 255.0,
            g: bg.g as f64 / 255.0,
            b: bg.b as f64 / 255.0,
            a: bg.a as f64 / 255.0,
        };

        let mut encoder = self.context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx clear encoder"),
        });
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("msx clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &offscreen.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
            });
            // No draw calls yet — vector (lyon tessellation), SDF, and
            // splat pipelines are next.
        }
        self.context.queue.submit(std::iter::once(encoder.finish()));

        *target = offscreen.read_back(&self.context.device, &self.context.queue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_cleared_background_color_if_a_gpu_adapter_is_available() {
        // CI/sandboxed environments often have no GPU passthrough at all —
        // skip rather than fail when that's the case, same as you'd want
        // for any hardware-dependent test.
        let Ok(renderer) = GpuRenderer::new() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let scene = Scene::new(msx_ast::Canvas::new(8.0, 8.0, msx_ast::Color::rgb(200, 100, 50)));
        let mut target = RenderTarget::new(8, 8);
        renderer.render(&scene, &mut target);

        assert_eq!(target.get_pixel(4, 4), [200, 100, 50, 255]);
    }
}
