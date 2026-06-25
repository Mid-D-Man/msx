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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_file_extracts_the_path() {
        let path = PathBuf::from("/tmp/example.msx");
        let event = winit::event::WindowEvent::DroppedFile(path.clone());
        assert_eq!(dropped_file(&event), Some(path));
    }

    #[test]
    fn dropped_file_ignores_other_events() {
        let event = winit::event::WindowEvent::CloseRequested;
        assert_eq!(dropped_file(&event), None);
    }
            }
