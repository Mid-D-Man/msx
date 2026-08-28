// apps/msx-viewer/src/window.rs
//! The `ApplicationHandler` implementation — winit's current (post-0.30)
//! event-loop pattern. Window and `Pixels` are both created lazily inside
//! `resumed()`, per winit's own current guidance ("create windows inside
//! the actively running event loop"), and held as `Option` until then.
//!
//! ## Redraw model
//!
//! A **static** scene (no `animations::` tracks, or an effective
//! duration of `0.0`) keeps exactly the original behaviour: redraws are
//! requested only when something actually changed (initial load, resize,
//! a dropped file) — matching winit's own stated preference for apps
//! that don't render continuously.
//!
//! An **animated** scene switches into live playback: `about_to_wait`
//! (called once per event-loop iteration, right before it would
//! otherwise sleep) schedules the next redraw via
//! `ControlFlow::WaitUntil`, and the actual per-frame work — resampling
//! `msx-anim`'s keyframe clock and writing the new pixels — happens in
//! `WindowEvent::RedrawRequested`, per winit's own guidance that
//! `about_to_wait` "is not an ideal event to drive application rendering
//! from." Once a `Once`-mode timeline settles on its final pose (see
//! `playback::should_keep_playing`), the viewer drops back to on-demand
//! redraws, same as any static scene — it doesn't keep polling a frame
//! that can never change again.
//!
//! Only the keyframe clock (`msx-anim`) plays live here. The shader
//! `time` uniform is a GPU-only clock this CPU-rendered viewer never
//! touches — see `renderer.rs`'s module doc for why GPU live playback is
//! deliberately out of scope for this pass.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use msx_ast::{LoopMode, Scene};
use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::input::dropped_file;
use crate::playback::{should_keep_playing, FRAME_INTERVAL};
use crate::renderer::{load_scene, render_scene, RenderedScene};

/// Tracks a live playback session for the currently-loaded scene. Only
/// exists while a scene has a real keyframe timeline (`Scene::is_animated`)
/// — a static scene never has one of these.
struct Playback {
    start: Instant,
    duration: f64,
    loop_mode: LoopMode,
}

pub struct App {
    initial_path: PathBuf,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    scene: Option<Scene>,
    playback: Option<Playback>,
    current: Option<RenderedScene>,
}

impl App {
    pub fn new(initial_path: PathBuf) -> Self {
        App { initial_path, window: None, pixels: None, scene: None, playback: None, current: None }
    }

    fn load_and_render(&mut self, path: &std::path::Path) -> Result<(), String> {
        let scene = load_scene(path)?;
        self.playback = if scene.is_animated() {
            Some(Playback {
                start: Instant::now(),
                duration: scene.effective_duration(),
                loop_mode: scene.loop_mode,
            })
        } else {
            None
        };
        self.scene = Some(scene);
        self.render_current_frame();
        Ok(())
    }

    /// Resamples the loaded scene at the current playback position (`0.0`,
    /// a no-op sample, for a non-animated scene — `resolve_at_time`
    /// returns the scene unchanged whenever it has no tracks at all) and
    /// re-renders it into `self.current`.
    fn render_current_frame(&mut self) {
        let Some(scene) = self.scene.as_ref() else { return };
        let t = self.playback.as_ref().map(|p| p.start.elapsed().as_secs_f64()).unwrap_or(0.0);
        let resolved = msx_anim::resolve_at_time(scene, t);
        self.current = Some(render_scene(&resolved));
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
                // Only an actively-playing scene needs a fresh sample —
                // a static scene's buffer already holds the right pixels
                // from whatever load/resize triggered this redraw.
                if self.playback.is_some() {
                    self.render_current_frame();
                    self.write_frame();
                }
                if let Some(pixels) = self.pixels.as_ref() {
                    if let Err(e) = pixels.render() {
                        eprintln!("warning: render failed: {}", e);
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(playback) = &self.playback else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        let elapsed = playback.start.elapsed().as_secs_f64();
        if !should_keep_playing(playback.loop_mode, elapsed, playback.duration) {
            // The pose already on screen (see `RedrawRequested`'s own
            // `resolve_at_time` call, which clamps internally) is already
            // the timeline's final frame — nothing further to sample.
            // Stop scheduling and fall back to on-demand redraws, same
            // as a static scene.
            self.playback = None;
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_INTERVAL));
    }
}
