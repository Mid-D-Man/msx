// primitives/msx-splat/src/lib.rs
//! Per-pixel Gaussian splat evaluation (`gaussian.rs`) and multi-splat
//! "over" compositing (`compositor.rs`). `msx_ast::GaussianSplat` already
//! carries the reference f64 math for single-splat evaluation (used by the
//! parser and its own unit tests) — this crate is the f32 + accumulation
//! layer that `msx-render-cpu`/`msx-render-gpu` actually run per pixel.

pub mod compositor;
pub mod gaussian;

pub use compositor::{Accumulator, Rgba};
pub use gaussian::{effective_radius, evaluate, evaluate_opacity};

pub use glam::Vec2;
