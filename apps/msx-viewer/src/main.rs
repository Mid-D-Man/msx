// apps/msx-viewer/src/main.rs
//! MSX native viewer — open an `.msx` file (source or compiled binary) and
//! see it rendered, in a real resizable window, with drag-and-drop support
//! to load a different file without restarting.
//!
//! CPU-rendered via `msx-render-cpu`, displayed via `pixels`. See
//! `renderer.rs`'s module doc for why the GPU path isn't wired in yet.

mod input;
mod renderer;
mod window;

use std::path::PathBuf;

use winit::event_loop::EventLoop;

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: msx-viewer <file.msx>");
            std::process::exit(1);
        }
    };

    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("error: failed to create event loop: {}", e);
            std::process::exit(1);
        }
    };

    let mut app = window::App::new(path);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
