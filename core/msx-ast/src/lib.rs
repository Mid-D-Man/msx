// core/msx-ast/src/lib.rs
pub mod animation;
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

pub use animation::{AnimatedProperty, AnimationTrack, Easing, Keyframe, LoopMode};
pub use canvas::{Canvas, Scene};
pub use color::{Color, Paint};
pub use effect::{Effect, EffectType};
pub use element::{Circle, Ellipse, Element, Group, Line, Path, Polyline, Polygon, Rect, Text, Use};
pub use gradient::{ConicGradient, Def, LinearGradient, RadialGradient, ShaderDef, ShaderUniform, ShaderUniformValue, Stop};
pub use layer::{layer_reordered, BlendMode, Layer};
pub use path::PathCommand;
pub use primitives::{BoundingBox, Point, ViewBox, fmt_f64};
pub use sdf::{SdfNode, SdfTree};
pub use splat::GaussianSplat;
pub use style::{FillRule, FontWeight, LineCap, LineJoin, Style, TextAnchor};
pub use transform::{Matrix2D, Transform};
