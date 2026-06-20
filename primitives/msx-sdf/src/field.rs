// primitives/msx-sdf/src/field.rs
//! A small, composable SDF algebra: every primitive shape and boolean
//! combinator is also a zero-cost struct implementing `SdfField`, so they
//! compose with ordinary Rust generics instead of needing a boxed tree —
//! `Union(Circle{..}, Circle{..}).evaluate(p)` works with no allocation.
//!
//! `shapes.rs`/`ops.rs` are what actually do the math; everything here is a
//! thin wrapper. Reach for the free functions directly in a hot per-pixel
//! loop where the shape is already known ahead of time; reach for this when
//! you want to build a region up as a value and pass it around.

use glam::Vec2;

use crate::ops;
use crate::shapes;

/// Anything that can be evaluated as a signed distance at a point.
/// Negative = inside, positive = outside, zero = boundary.
pub trait SdfField {
    fn evaluate(&self, p: Vec2) -> f32;
}

// ── Primitive wrappers ─────────────────────────────────────────────────────

pub struct Circle {
    pub center: Vec2,
    pub radius: f32,
}
impl SdfField for Circle {
    fn evaluate(&self, p: Vec2) -> f32 {
        shapes::circle(p, self.center, self.radius)
    }
}

pub struct RoundedBox {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub corner_radius: f32,
}
impl SdfField for RoundedBox {
    fn evaluate(&self, p: Vec2) -> f32 {
        shapes::rounded_box(p, self.center, self.half_extents, self.corner_radius)
    }
}

pub struct Segment {
    pub a: Vec2,
    pub b: Vec2,
    pub thickness: f32,
}
impl SdfField for Segment {
    fn evaluate(&self, p: Vec2) -> f32 {
        shapes::segment(p, self.a, self.b, self.thickness)
    }
}

pub struct Ring {
    pub center: Vec2,
    pub radius: f32,
    pub thickness: f32,
}
impl SdfField for Ring {
    fn evaluate(&self, p: Vec2) -> f32 {
        shapes::ring(p, self.center, self.radius, self.thickness)
    }
}

pub struct Arc {
    pub center: Vec2,
    pub radius: f32,
    pub angle_start: f32,
    pub angle_end: f32,
    pub thickness: f32,
}
impl SdfField for Arc {
    fn evaluate(&self, p: Vec2) -> f32 {
        shapes::arc(p, self.center, self.radius, self.angle_start, self.angle_end, self.thickness)
    }
}

// ── Combinators ────────────────────────────────────────────────────────────

pub struct Union<A, B>(pub A, pub B);
impl<A: SdfField, B: SdfField> SdfField for Union<A, B> {
    fn evaluate(&self, p: Vec2) -> f32 {
        ops::union(self.0.evaluate(p), self.1.evaluate(p))
    }
}

pub struct SmoothUnion<A, B> {
    pub a: A,
    pub b: B,
    pub k: f32,
}
impl<A: SdfField, B: SdfField> SdfField for SmoothUnion<A, B> {
    fn evaluate(&self, p: Vec2) -> f32 {
        ops::smooth_union(self.a.evaluate(p), self.b.evaluate(p), self.k)
    }
}

pub struct Subtract<A, B>(pub A, pub B);
impl<A: SdfField, B: SdfField> SdfField for Subtract<A, B> {
    fn evaluate(&self, p: Vec2) -> f32 {
        ops::subtract(self.0.evaluate(p), self.1.evaluate(p))
    }
}

pub struct SmoothSubtract<A, B> {
    pub a: A,
    pub b: B,
    pub k: f32,
}
impl<A: SdfField, B: SdfField> SdfField for SmoothSubtract<A, B> {
    fn evaluate(&self, p: Vec2) -> f32 {
        ops::smooth_subtract(self.a.evaluate(p), self.b.evaluate(p), self.k)
    }
}

pub struct Intersect<A, B>(pub A, pub B);
impl<A: SdfField, B: SdfField> SdfField for Intersect<A, B> {
    fn evaluate(&self, p: Vec2) -> f32 {
        ops::intersect(self.0.evaluate(p), self.1.evaluate(p))
    }
}

pub struct SmoothIntersect<A, B> {
    pub a: A,
    pub b: B,
    pub k: f32,
}
impl<A: SdfField, B: SdfField> SdfField for SmoothIntersect<A, B> {
    fn evaluate(&self, p: Vec2) -> f32 {
        ops::smooth_intersect(self.a.evaluate(p), self.b.evaluate(p), self.k)
    }
}

pub struct Offset<A> {
    pub field: A,
    pub amount: f32,
}
impl<A: SdfField> SdfField for Offset<A> {
    fn evaluate(&self, p: Vec2) -> f32 {
        ops::offset(self.field.evaluate(p), self.amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composed_field_matches_free_function_call() {
        let a = Circle { center: Vec2::ZERO, radius: 10.0 };
        let b = Circle { center: Vec2::new(40.0, 0.0), radius: 10.0 };
        let composed = SmoothUnion { a, b, k: 15.0 };

        let p = Vec2::new(20.0, 0.0);
        let expected = ops::smooth_union(
            shapes::circle(p, Vec2::ZERO, 10.0),
            shapes::circle(p, Vec2::new(40.0, 0.0), 10.0),
            15.0,
        );
        assert!((composed.evaluate(p) - expected).abs() < 1e-5);
    }

    #[test]
    fn subtract_struct_carves_a_hole() {
        let ring_shape = Subtract(
            Circle { center: Vec2::ZERO, radius: 50.0 },
            Circle { center: Vec2::ZERO, radius: 30.0 },
        );
        assert!(ring_shape.evaluate(Vec2::new(40.0, 0.0)) < 0.0);
        assert!(ring_shape.evaluate(Vec2::new(10.0, 0.0)) > 0.0);
    }

    #[test]
    fn smooth_union_struct_softens_the_seam_between_separate_shapes() {
        let hard = Union(
            Circle { center: Vec2::ZERO, radius: 10.0 },
            Circle { center: Vec2::new(40.0, 0.0), radius: 10.0 },
        );
        let smooth = SmoothUnion {
            a: Circle { center: Vec2::ZERO, radius: 10.0 },
            b: Circle { center: Vec2::new(40.0, 0.0), radius: 10.0 },
            k: 15.0,
        };
        let midpoint = Vec2::new(20.0, 0.0);
        assert!(smooth.evaluate(midpoint) < hard.evaluate(midpoint));
    }
}
