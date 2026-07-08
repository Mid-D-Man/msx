// core/msx-anim/src/lib.rs
//! Animation resolver — turns an animated `Scene` + a point in time into a
//! fully static `Scene` that any existing renderer (SVG/CPU/GPU) can draw
//! without knowing animation exists.
//!
//! `msx-ast` owns the data (`AnimationTrack`, `Keyframe`, ...) with zero
//! logic beyond per-track evaluation. This crate owns the one thing that
//! isn't pure per-track math: walking the scene tree, matching tracks to
//! elements by `id`, composing a TRS+opacity delta, and folding it onto
//! whatever static transform the element already has. `msx-viewer`, a WASM
//! runtime, and `msx-cli` (for baking a single frame to SVG) can all call
//! the same `resolve_at_time` and get an ordinary `Scene` back.

mod delta;
mod resolver;

pub use resolver::resolve_at_time;

#[cfg(test)]
mod tests;
