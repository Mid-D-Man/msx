// primitives/msx-splat/src/gaussian.rs
//! f32 re-derivation of `msx_ast::GaussianSplat`'s evaluation math.
//! `msx-ast`'s own `evaluate_at`/`evaluate_opacity_at` (f64) are the
//! reference implementation; this crate's job is the same math in `f32` for
//! per-pixel CPU loops and GPU fragment shaders, where that precision
//! tradeoff is the right one to make.

use glam::Vec2;
use msx_ast::GaussianSplat;

/// Unnormalized Gaussian value at `p` (0.0..=1.0), NOT multiplied by the
/// splat's peak opacity — mirrors `GaussianSplat::evaluate_at`.
pub fn evaluate(splat: &GaussianSplat, p: Vec2) -> f32 {
    let center = Vec2::new(splat.x as f32, splat.y as f32);
    let d = p - center;
    let (sin_r, cos_r) = (splat.rotation as f32).sin_cos();
    let lx = d.x * cos_r + d.y * sin_r;
    let ly = d.y * cos_r - d.x * sin_r;
    let sigma_x = splat.sigma_x as f32;
    let sigma_y = splat.sigma_y as f32;
    (-(lx * lx) / (2.0 * sigma_x * sigma_x) - (ly * ly) / (2.0 * sigma_y * sigma_y)).exp()
}

/// `evaluate(..)` scaled by the splat's peak opacity — mirrors
/// `GaussianSplat::evaluate_opacity_at`. This is the value
/// `compositor::Accumulator::accumulate` expects as `src_alpha`.
pub fn evaluate_opacity(splat: &GaussianSplat, p: Vec2) -> f32 {
    evaluate(splat, p) * splat.opacity as f32
}

/// Radius beyond which the splat contributes less than `threshold` —
/// mirrors `GaussianSplat::effective_radius`; useful for culling before the
/// accumulator loop even starts.
pub fn effective_radius(splat: &GaussianSplat, threshold: f32) -> f32 {
    splat.sigma_x.max(splat.sigma_y) as f32 * (-2.0 * threshold.ln()).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::Color;

    #[test]
    fn matches_f64_reference_at_center() {
        let s = GaussianSplat::circle(100.0, 100.0, 20.0, Color::WHITE, 1.0);
        let f32_val = evaluate(&s, Vec2::new(100.0, 100.0));
        let f64_val = s.evaluate_at(100.0, 100.0) as f32;
        assert!((f32_val - f64_val).abs() < 1e-4);
    }

    #[test]
    fn matches_f64_reference_off_center() {
        let s = GaussianSplat::circle(0.0, 0.0, 10.0, Color::WHITE, 1.0);
        let f32_val = evaluate(&s, Vec2::new(10.0, 0.0));
        let f64_val = s.evaluate_at(10.0, 0.0) as f32;
        assert!((f32_val - f64_val).abs() < 1e-4);
    }

    #[test]
    fn opacity_scales_peak_value() {
        let s = GaussianSplat::circle(0.0, 0.0, 10.0, Color::WHITE, 0.4);
        let v = evaluate_opacity(&s, Vec2::ZERO);
        assert!((v - 0.4).abs() < 1e-4);
    }

    #[test]
    fn effective_radius_matches_f64_reference() {
        let s = GaussianSplat::circle(0.0, 0.0, 20.0, Color::WHITE, 1.0);
        let f32_val = effective_radius(&s, 0.01);
        let f64_val = s.effective_radius(0.01) as f32;
        assert!((f32_val - f64_val).abs() < 1e-2);
    }
}
