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

// ── Tests ────────────────────────────────────────────────────────────────────
// Same source/binary/garbage detection as `msx-cli`'s identical, separately
// duplicated `load_scene_bytes` — the two crates carry their own copy of this
// sniff logic, so they each carry their own copy of the tests for it too,
// rather than leaving the viewer's copy unverified.

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Canvas, Color};

    #[test]
    fn load_scene_bytes_detects_dixscript_source() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #ffffff }
  elements::
)
"#;
        let scene = load_scene_bytes(src.as_bytes()).expect("source should parse");
        assert!((scene.canvas.width - 10.0).abs() < 1e-9);
    }

    #[test]
    fn load_scene_bytes_detects_binary() {
        let scene = Scene::new(Canvas::new(50.0, 50.0, Color::WHITE));
        let binary = msx_binary::compile(&scene, true).expect("compile should succeed");
        let decoded = load_scene_bytes(&binary).expect("binary should decode");
        assert!((decoded.canvas.width - 50.0).abs() < 1e-9);
    }

    #[test]
    fn load_scene_bytes_rejects_invalid_input() {
        let garbage = [0xFFu8, 0xFE, 0x00, 0x01, 0x02];
        assert!(load_scene_bytes(&garbage).is_err());
    }
    }
