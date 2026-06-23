// apps/msx-viewer/src/input.rs
//! Drag-and-drop file loading. `WindowEvent::DroppedFile` is a
//! long-standing, stable winit event — confirmed unchanged across the
//! ApplicationHandler-era API checked for this crate.

use std::path::PathBuf;

/// Returns `Some(path)` if `event` is a dropped file, `None` for every
/// other event — a thin filter so `window.rs`'s `window_event` match stays
/// readable.
pub fn dropped_file(event: &winit::event::WindowEvent) -> Option<PathBuf> {
    match event {
        winit::event::WindowEvent::DroppedFile(path) => Some(path.clone()),
        _ => None,
    }
}
