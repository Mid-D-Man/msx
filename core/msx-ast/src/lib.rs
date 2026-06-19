// core/msx-ast/src/lib.rs
//! MSX scene graph — pure data types used by all other MSX crates.
//! Zero external dependencies.

pub mod canvas;
pub mod color;
pub mod effect;
pub mod element;
pub mod gradient;
pub mod layer;
pub mod path;
pub mod primitives;
pub mod sdf;
pub mod splat;
pub mod style;
pub mod transform;

// ── Top-level re-exports ──────────────────────────────────────────────────────

pub use canvas::{Canvas, Scene};
pub use color::{Color, Paint};
pub use effect::{Effect, EffectType};
pub use element::{Circle, Ellipse, Element, Group, Line, Path, Polyline, Polygon, Rect, Text, Use};
pub use gradient::{ConicGradient, Def, LinearGradient, RadialGradient, Stop};
pub use layer::{BlendMode, Layer};
pub use path::PathCommand;
pub use primitives::{BoundingBox, Point, ViewBox, fmt_f64};
pub use sdf::{SdfNode, SdfTree};
pub use splat::GaussianSplat;
pub use style::{FillRule, FontWeight, LineCap, LineJoin, Style, TextAnchor};
pub use transform::{Matrix2D, Transform};
