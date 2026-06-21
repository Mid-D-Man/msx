// render/msx-render-cpu/src/geom.rs
//! Shared affine-matrix helpers for `sdf_raster.rs`/`splat_raster.rs`:
//! point application, bbox transformation, and matrix inversion.
//! `msx_ast::Matrix2D` doesn't carry an inverse itself (it's built for SVG
//! `transform=` serialization, where you never need to invert), so that's
//! derived here directly — standard 2×2-plus-translation affine inverse,
//! nothing exotic.

use msx_ast::Matrix2D;

pub fn apply_matrix(m: Matrix2D, p: (f32, f32)) -> (f32, f32) {
    (
        m.a as f32 * p.0 + m.c as f32 * p.1 + m.e as f32,
        m.b as f32 * p.0 + m.d as f32 * p.1 + m.f as f32,
    )
}

pub fn invert_matrix(m: Matrix2D) -> Option<Matrix2D> {
    let det = m.a * m.d - m.b * m.c;
    if det.abs() < 1e-9 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m.d * inv_det;
    let b = -m.b * inv_det;
    let c = -m.c * inv_det;
    let d = m.a * inv_det;
    let e = -(a * m.e + c * m.f);
    let f = -(b * m.e + d * m.f);
    Some(Matrix2D { a, b, c, d, e, f })
}

/// Maps a local-space bbox into screen space by transforming all four
/// corners (not just two diagonal points — matters once rotation or skew
/// is involved) and taking the axis-aligned bounds of the result.
pub fn transform_bounds(b: (f32, f32, f32, f32), m: Matrix2D) -> (f32, f32, f32, f32) {
    let corners = [
        apply_matrix(m, (b.0, b.1)),
        apply_matrix(m, (b.2, b.1)),
        apply_matrix(m, (b.0, b.3)),
        apply_matrix(m, (b.2, b.3)),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (x, y) in corners {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trips() {
        let id = Matrix2D::identity();
        let inv = invert_matrix(id).unwrap();
        assert_eq!(apply_matrix(inv, (5.0, 7.0)), (5.0, 7.0));
    }

    #[test]
    fn translate_then_invert_cancels_out() {
        let m = Matrix2D::translate(10.0, -20.0);
        let inv = invert_matrix(m).unwrap();
        let p = apply_matrix(m, (3.0, 4.0));
        let back = apply_matrix(inv, p);
        assert!((back.0 - 3.0).abs() < 1e-4);
        assert!((back.1 - 4.0).abs() < 1e-4);
    }

    #[test]
    fn rotate_then_invert_cancels_out() {
        let m = Matrix2D::rotate_deg(37.0);
        let inv = invert_matrix(m).unwrap();
        let p = apply_matrix(m, (12.0, -6.0));
        let back = apply_matrix(inv, p);
        assert!((back.0 - 12.0).abs() < 1e-3);
        assert!((back.1 - -6.0).abs() < 1e-3);
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        let m = Matrix2D::scale(0.0, 1.0);
        assert!(invert_matrix(m).is_none());
    }
}
