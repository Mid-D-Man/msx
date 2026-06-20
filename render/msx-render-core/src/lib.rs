// render/msx-render-core/src/lib.rs
//! Shared types and math for every MSX renderer backend (`msx-render-cpu`,
//! eventually `msx-render-gpu`): the `Renderer` trait, `RenderTarget` pixel
//! buffer, premultiplied-alpha color helpers, and Porter-Duff + CSS-style
//! blend-mode compositing math.
//!
//! `msx-render-svg` doesn't depend on this — it emits text, not pixels, so
//! it never needed `RenderTarget` — but the blend-mode formulas here are
//! the ground truth its `BlendMode::to_css_blend_mode()` CSS fallback is
//! approximating, and what the real CPU/GPU compositors will use exactly.

pub mod blend;
pub mod color;
pub mod tile;
pub mod traits;

pub use blend::composite;
pub use color::PremulColor;
pub use tile::{RenderTarget, TileRegion};
pub use traits::Renderer;
