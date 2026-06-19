// core/msx-ast/src/sdf.rs
//! Signed Distance Field primitive types and compound operations.
//!
//! An SDF is f(x,y)→f32: negative inside, positive outside, zero on boundary.
//! This enables smooth boolean operations (smooth union, subtract, intersect)
//! impossible in traditional vector graphics.
//!
//! The SdfTree here is the scene-graph representation (data only).
//! The actual per-pixel evaluation lives in the `msx-sdf` crate which
//! provides the SdfField trait. GPU evaluation is transcribed to WGSL in
//! `msx-render-gpu`.

use crate::color::Paint;
use crate::transform::Transform;

/// Recursive SDF tree — a primitive or a compound operation.
#[derive(Debug, Clone, PartialEq)]
pub enum SdfTree {
    // ── Primitives ────────────────────────────────────────────────────────────

    /// `length(p - center) - r`
    Circle { cx: f64, cy: f64, r: f64 },

    /// Rounded rectangle.
    Box {
        x: f64, y: f64,
        width: f64, height: f64,
        corner_radius: f64,
    },

    /// Thick line segment SDF.
    Line { x1: f64, y1: f64, x2: f64, y2: f64, thickness: f64 },

    /// Annulus (ring).
    Ring { cx: f64, cy: f64, r: f64, thickness: f64 },

    /// Arc segment.
    Arc {
        cx: f64, cy: f64, r: f64,
        angle_start: f64,  // radians
        angle_end:   f64,
        thickness:   f64,
    },

    // ── Boolean operations ────────────────────────────────────────────────────

    /// Hard union: `min(d₁, d₂, …)` — sharp seam at boundary.
    Union(Vec<SdfTree>),

    /// Smooth union via polynomial blend — shapes merge like clay.
    /// `k` is the blend radius: 0.0 = sharp, larger = heavier blend.
    SmoothUnion { children: Vec<SdfTree>, k: f64 },

    /// Hard subtraction: `max(dₐ, -d_b)` — removes B from A.
    Subtract { a: Box<SdfTree>, b: Box<SdfTree> },

    /// Smooth subtraction.
    SmoothSubtract { a: Box<SdfTree>, b: Box<SdfTree>, k: f64 },

    /// Hard intersection: `max(dₐ, d_b)` — only the overlap remains.
    Intersect { a: Box<SdfTree>, b: Box<SdfTree> },

    /// Smooth intersection.
    SmoothIntersect { a: Box<SdfTree>, b: Box<SdfTree>, k: f64 },

    /// Inflate (positive) or deflate (negative) by `amount` scene units.
    Offset { child: Box<SdfTree>, amount: f64 },
}

impl SdfTree {
    pub fn offset(self, amount: f64) -> SdfTree {
        SdfTree::Offset { child: Box::new(self), amount }
    }

    pub fn subtract(self, b: SdfTree) -> SdfTree {
        SdfTree::Subtract { a: Box::new(self), b: Box::new(b) }
    }

    pub fn smooth_union(children: Vec<SdfTree>, k: f64) -> SdfTree {
        SdfTree::SmoothUnion { children, k }
    }

    pub fn smooth_subtract(self, b: SdfTree, k: f64) -> SdfTree {
        SdfTree::SmoothSubtract { a: Box::new(self), b: Box::new(b), k }
    }
}

/// An SDF element in the scene tree — a renderable compound SDF shape.
///
/// Rendered by evaluating `tree` at each pixel: fill where d ≤ 0,
/// stroke in the band |d| ≤ stroke_width/2.
#[derive(Debug, Clone)]
pub struct SdfNode {
    pub tree:         SdfTree,
    pub fill:         Paint,
    pub stroke:       Option<Paint>,
    pub stroke_width: Option<f64>,
    pub id:           Option<String>,
    pub transform:    Option<Transform>,
}

impl SdfNode {
    pub fn new(tree: SdfTree, fill: Paint) -> Self {
        SdfNode { tree, fill, stroke: None, stroke_width: None, id: None, transform: None }
    }

    pub fn with_stroke(mut self, stroke: Paint, width: f64) -> Self {
        self.stroke = Some(stroke);
        self.stroke_width = Some(width);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Paint;

    #[test]
    fn sdf_tree_constructs() {
        let c = SdfTree::Circle { cx: 0.0, cy: 0.0, r: 50.0 };
        let b = SdfTree::Box { x: 20.0, y: 20.0, width: 60.0, height: 40.0, corner_radius: 5.0 };
        let u = SdfTree::Union(vec![c, b]);
        assert!(matches!(u, SdfTree::Union(_)));
    }

    #[test]
    fn smooth_union_preserves_k() {
        let a = SdfTree::Circle { cx: 0.0, cy: 0.0, r: 30.0 };
        let b = SdfTree::Circle { cx: 60.0, cy: 0.0, r: 30.0 };
        let m = SdfTree::smooth_union(vec![a, b], 0.3);
        assert!(matches!(m, SdfTree::SmoothUnion { k, .. } if (k - 0.3).abs() < 1e-9));
    }

    #[test]
    fn subtract_chaining() {
        let ring = SdfTree::Circle { cx: 0.0, cy: 0.0, r: 50.0 }
            .subtract(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 30.0 });
        assert!(matches!(ring, SdfTree::Subtract { .. }));
    }
            }
