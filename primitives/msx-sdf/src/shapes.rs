// primitives/msx-sdf/src/shapes.rs
//! Primitive SDF shape evaluators. Every function takes a point already in
//! the shape's own coordinate space and returns the signed distance:
//! negative inside, positive outside, zero on the boundary. These are the
//! standard formulas from Inigo Quilez's distance-function catalogue —
//! nothing exotic, just careful about sign conventions so they compose
//! correctly with `ops.rs`.

use glam::Vec2;

pub fn circle(p: Vec2, center: Vec2, r: f32) -> f32 {
    (p - center).length() - r
}

/// Axis-aligned rounded box, given as center + half-extents (not the
/// `x,y,width,height` rect convention — callers convert once on entry).
pub fn rounded_box(p: Vec2, center: Vec2, half_extents: Vec2, corner_radius: f32) -> f32 {
    let q = (p - center).abs() - half_extents + Vec2::splat(corner_radius);
    q.x.max(q.y).min(0.0) + q.max(Vec2::ZERO).length() - corner_radius
}

/// Thick line segment ("capsule" without round caps) from `a` to `b`.
pub fn segment(p: Vec2, a: Vec2, b: Vec2, thickness: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);
    (pa - ba * h).length() - thickness * 0.5
}

/// Annulus (ring) — the boundary band of a circle, given a thickness.
pub fn ring(p: Vec2, center: Vec2, r: f32, thickness: f32) -> f32 {
    ((p - center).length() - r).abs() - thickness * 0.5
}

/// Circular arc between `angle_start` and `angle_end` (radians), with the
/// two open ends capped as rounded points — a thick arc reads as a fat
/// curved stroke, not a sharp-edged pie slice.
pub fn arc(p: Vec2, center: Vec2, r: f32, angle_start: f32, angle_end: f32, thickness: f32) -> f32 {
    let rel = p - center;
    let theta = rel.y.atan2(rel.x);

    if angle_in_range(theta, angle_start, angle_end) {
        (rel.length() - r).abs() - thickness * 0.5
    } else {
        let cap_a = center + Vec2::new(angle_start.cos(), angle_start.sin()) * r;
        let cap_b = center + Vec2::new(angle_end.cos(), angle_end.sin()) * r;
        (p - cap_a).length().min((p - cap_b).length()) - thickness * 0.5
    }
}

fn angle_in_range(theta: f32, start: f32, end: f32) -> bool {
    use std::f32::consts::TAU;
    let norm = |a: f32| a.rem_euclid(TAU);
    let (t, s, e) = (norm(theta), norm(start), norm(end));
    if s <= e { t >= s && t <= e } else { t >= s || t <= e }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_signs() {
        assert!(circle(Vec2::ZERO, Vec2::ZERO, 10.0) < 0.0);
        assert!((circle(Vec2::new(10.0, 0.0), Vec2::ZERO, 10.0)).abs() < 1e-4);
        assert!(circle(Vec2::new(20.0, 0.0), Vec2::ZERO, 10.0) > 0.0);
    }

    #[test]
    fn rounded_box_zero_radius_matches_sharp_box() {
        let d = rounded_box(Vec2::new(60.0, 0.0), Vec2::ZERO, Vec2::new(50.0, 20.0), 0.0);
        assert!((d - 10.0).abs() < 1e-3);
    }

    #[test]
    fn segment_distance_at_midpoint_is_zero() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(100.0, 0.0);
        let d = segment(Vec2::new(50.0, 0.0), a, b, 0.0);
        assert!(d.abs() < 1e-4);
    }

    #[test]
    fn ring_inside_band_is_negative() {
        let d = ring(Vec2::new(10.0, 0.0), Vec2::ZERO, 10.0, 4.0);
        assert!(d < 0.0);
        let d_far = ring(Vec2::new(0.0, 0.0), Vec2::ZERO, 10.0, 4.0);
        assert!(d_far > 0.0);
    }

    #[test]
    fn arc_within_range_behaves_like_ring() {
        // BUGFIX: this used to assert `d.abs() < 1e-3` while passing
        // thickness=2.0. arc()'s in-range branch is the exact same formula
        // as ring(), and for a point exactly on the arc's centerline
        // radius, that formula correctly evaluates to -thickness/2 (you're
        // a half-thickness inside the band, not sitting on its edge) — so
        // the old assertion expected ~0.0 when the correct value is -1.0.
        // Confirmed via a standalone reimplementation of the formula before
        // touching this. arc()'s implementation was always correct; only
        // this assertion was wrong. Rewritten to verify what the test name
        // actually claims — that arc's in-range branch numerically matches
        // ring()'s formula — rather than re-hardcoding a number.
        use std::f32::consts::PI;
        let p = Vec2::new(10.0, 0.0);
        let center = Vec2::ZERO;
        let r = 10.0;
        let thickness = 2.0;

        let d = arc(p, center, r, -PI / 4.0, PI / 4.0, thickness);
        let expected = ring(p, center, r, thickness);
        assert!((d - expected).abs() < 1e-4, "arc in-range should match ring(): d={d}, expected={expected}");
    }

    #[test]
    fn arc_outside_range_distances_to_nearest_cap() {
        use std::f32::consts::PI;
        let d = arc(Vec2::new(-10.0, 0.0), Vec2::ZERO, 10.0, -PI / 4.0, PI / 4.0, 0.0);
        assert!(d > 0.0);
    }
    }
