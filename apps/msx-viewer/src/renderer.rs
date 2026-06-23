// apps/msx-viewer/src/renderer.rs
//! Loads an `.msx` file (source or binary, auto-detected) and renders it
//! via `msx-render-cpu` into a plain RGBA byte buffer ready to hand to
//! `pixels::Pixels::frame_mut()`.
//!
//! ## Honest scope for this pass
//!
//! CPU-only. The plan's "GPU path switchable at runtime" is deferred —
//! `pixels` bundles its own internal `wgpu` (confirmed: it re-exports
//! `wgpu` directly as `pixels::wgpu`), separate from `msx-render-gpu`'s own
//! pinned `wgpu = "26"`. Running both live against the same window means
//! deliberately managing two independent wgpu instances on one window
//! handle — workable, but a separate, focused piece of work, not something
//! to fold in as an afterthought here. CPU is the one real, working path
//! for now, which also happens to be exactly what the original plan called
//! "always the fallback."

use std::path::Path;

use msx_ast::Scene;
use msx_render_core::{RenderTarget, Renderer};

pub struct RenderedScene {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn load_scene(path: &Path) -> Result<Scene, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    load_scene_bytes(&bytes)
}

fn load_scene_bytes(bytes: &[u8]) -> Result<Scene, String> {
    if bytes.len() >= 4 && &bytes[0..4] == b"MSX\0" {
        msx_binary::decode(bytes).map_err(|e| format!("binary decode failed: {}", e))
    } else {
        let source = std::str::from_utf8(bytes).map_err(|_| "input is not valid UTF-8 DixScript source".to_string())?;
        msx_parser::parse_scene(source)
    }
}

pub fn render_scene(scene: &Scene) -> RenderedScene {
    let renderer = msx_render_cpu::CpuRenderer::new();
    let width = scene.canvas.width.round().max(1.0) as u32;
    let height = scene.canvas.height.round().max(1.0) as u32;

    let mut target = RenderTarget::new(width, height);
    renderer.render(scene, &mut target);

    RenderedScene { width, height, rgba: target.into_bytes() }
  }
