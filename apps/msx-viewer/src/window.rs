// apps/msx-viewer/src/window.rs
//! The `ApplicationHandler` implementation — winit's current (post-0.30)
//! event-loop pattern. Window and `Pixels` are both created lazily inside
//! `resumed()`, per winit's own current guidance ("create windows inside
//! the actively running event loop"), and held as `Option` until then.
//!
//! Redraws are requested only when something actually changed (initial
//! load, resize, a dropped file) — not every frame — matching winit's own
//! stated preference for apps that don't render continuously.

use std::path::PathBuf;
use std::sync::Arc;

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::input::dropped_file;
use crate::renderer::{load_scene, render_scene, RenderedScene};

pub struct App {
    initial_path: PathBuf,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    current: Option<RenderedScene>,
}

impl App {
    pub fn new(initial_path: PathBuf) -> Self {
        App { initial_path, window: None, pixels: None, current: None }
    }

    fn load_and_render(&mut self, path: &std::path::Path) -> Result<(), String> {
        let scene = load_scene(path)?;
        self.current = Some(render_scene(&scene));
        Ok(())
    }

    fn sync_pixels_buffer_size(&mut self) {
        let (Some(pixels), Some(rendered)) = (self.pixels.as_mut(), self.current.as_ref()) else { return };
        if let Err(e) = pixels.resize_buffer(rendered.width, rendered.height) {
            eprintln!("warning: failed to resize pixel buffer: {}", e);
        }
    }

    fn write_frame(&mut self) {
        let (Some(pixels), Some(rendered)) = (self.pixels.as_mut(), self.current.as_ref()) else { return };
        let frame = pixels.frame_mut();
        if frame.len() == rendered.rgba.len() {
            frame.copy_from_slice(&rendered.rgba);
        } else {
            eprintln!(
                "warning: frame buffer size mismatch ({} vs {} bytes) — skipping this frame",
                frame.len(),
                rendered.rgba.len()
            );
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // already initialized — resumed() can fire more than once on some platforms
        }

        if let Err(e) = self.load_and_render(&self.initial_path.clone()) {
            eprintln!("error: {}", e);
            event_loop.exit();
            return;
        }
        let rendered = self.current.as_ref().expect("just loaded");

        let attrs = Window::default_attributes()
            .with_title(format!("MSX Viewer — {}", self.initial_path.display()))
            .with_inner_size(LogicalSize::new(rendered.width as f64, rendered.height as f64));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("error: failed to create window: {}", e);
                event_loop.exit();
                return;
            }
        };

        let size = window.inner_size();
        let surface_texture = SurfaceTexture::new(size.width.max(1), size.height.max(1), Arc::clone(&window));
        let pixels = match Pixels::new(rendered.width, rendered.height, surface_texture) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: failed to initialize pixel buffer: {}", e);
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.pixels = Some(pixels);
        self.write_frame();
        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        if let Some(path) = dropped_file(&event) {
            match self.load_and_render(&path) {
                Ok(()) => {
                    self.sync_pixels_buffer_size();
                    self.write_frame();
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Err(e) => eprintln!("error loading {}: {}", path.display(), e),
            }
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(pixels) = self.pixels.as_mut() {
                    if let Err(e) = pixels.resize_surface(size.width.max(1), size.height.max(1)) {
                        eprintln!("warning: failed to resize surface: {}", e);
                    }
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(pixels) = self.pixels.as_ref() {
                    if let Err(e) = pixels.render() {
                        eprintln!("warning: render failed: {}", e);
                    }
                }
            }
            _ => {}
        }
    }
}
