// primitives/msx-sdf/src/lib.rs
//! Pure SDF math — no rendering, no knowledge of MSX's scene format.
//! `evaluate(p) -> f32`: negative inside, positive outside, zero on the
//! boundary — the same convention for every shape and every combinator, so
//! they compose. Used identically by a per-pixel CPU loop
//! (`msx-render-cpu`) and a fragment-shader transcription
//! (`msx-render-gpu`) — one set of formulas, two evaluation contexts.
//!
//! This crate deliberately doesn't know `msx_ast::SdfTree` exists — the
//! recursive "walk a scene-graph SdfTree and call into these functions"
//! dispatcher belongs in whichever renderer crate already depends on both
//! `msx-ast` and `msx-sdf` (i.e. `msx-render-cpu`), since `SdfField` is this
//! crate's trait and `SdfTree` is msx-ast's type — the orphan rule means
//! `impl SdfField for SdfTree` can only legally live here or there, and
//! "here" would mean adding a dependency this crate intentionally doesn't
//! have.

pub mod field;
pub mod ops;
pub mod shapes;

pub use field::{
    Arc, Circle, Intersect, Offset, Ring, RoundedBox, SdfField, Segment, SmoothIntersect,
    SmoothSubtract, SmoothUnion, Subtract, Union,
};
pub use ops::{intersect, offset, smooth_intersect, smooth_subtract, smooth_union, subtract, union};
pub use shapes::{arc, circle, ring, rounded_box, segment};

pub use glam::Vec2;
