// primitives/msx-sdf/src/ops.rs
//! Boolean combinators over raw signed distances. Every function is a pure
//! `f32, f32 -> f32` (plus a `k` smoothing radius for the smooth variants) —
//! they don't know or care what shape produced the distances. `k <= 0.0`
//! collapses every `smooth_*` op to its hard equivalent.

pub fn union(a: f32, b: f32) -> f32 {
    a.min(b)
}

pub fn subtract(a: f32, b: f32) -> f32 {
    a.max(-b)
}

pub fn intersect(a: f32, b: f32) -> f32 {
    a.max(b)
}

pub fn offset(d: f32, amount: f32) -> f32 {
    d - amount
}

pub fn smooth_union(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 { union(a, b) } else { smin(a, b, k) }
}

pub fn smooth_subtract(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 { subtract(a, b) } else { smax(a, -b, k) }
}

pub fn smooth_intersect(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 { intersect(a, b) } else { smax(a, b, k) }
}

/// Inigo Quilez's polynomial smooth minimum — the standard formula behind
/// nearly every "smooth blend" SDF demo.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    lerp(b, a, h) - k * h * (1.0 - h)
}

/// `smax(a,b,k) = -smin(-a,-b,k)` — holds for this particular smin
/// construction (verified directly: substituting and simplifying recovers
/// exactly `max(a,b)` in the limit, with the same smoothing behavior at the
/// crossover as `smin` has for `min`).
fn smax(a: f32, b: f32, k: f32) -> f32 {
    -smin(-a, -b, k)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_picks_closer_surface() {
        assert_eq!(union(-2.0, 3.0), -2.0);
        assert_eq!(union(5.0, 1.0), 1.0);
    }

    #[test]
    fn subtract_excludes_b_region() {
        assert!(subtract(-1.0, -1.0) > 0.0); // inside both -> excluded
        assert!(subtract(-1.0, 2.0) < 0.0);  // inside a only -> stays inside
    }

    #[test]
    fn intersect_requires_both() {
        assert!(intersect(-1.0, 2.0) > 0.0);
        assert!(intersect(-1.0, -1.0) < 0.0);
    }

    #[test]
    fn smooth_union_zero_k_matches_hard_union() {
        assert_eq!(smooth_union(-2.0, 3.0, 0.0), union(-2.0, 3.0));
    }

    #[test]
    fn smooth_union_blends_below_hard_minimum() {
        let hard = union(1.0, 1.0);
        let smooth = smooth_union(1.0, 1.0, 0.5);
        assert!(smooth < hard);
    }

    #[test]
    fn smooth_subtract_zero_k_matches_hard_subtract() {
        assert_eq!(smooth_subtract(-1.0, -1.0, 0.0), subtract(-1.0, -1.0));
    }
      }
