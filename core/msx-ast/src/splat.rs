// core/msx-ast/src/splat.rs
//! 2D Gaussian splat — a soft, painterly elliptical blob.
//!
//! Evaluated as:
//!   opacity * exp(-( (dx·cos r + dy·sin r)² / (2·σx²)
//!                  + (dy·cos r - dx·sin r)² / (2·σy²) ))
//! where (dx,dy) = (px-x, py-y), r = rotation.
//!
//! CPU: per-pixel loop in msx-render-cpu via msx-splat.
//! GPU: instanced quad per splat, gaussian in fragment shader (msx-render-gpu).
//! Stack hundreds of splats for painterly results.

use crate::color::Color;

/// A single 2D Gaussian splat scene element.
#[derive(Debug, Clone, PartialEq)]
pub struct GaussianSplat {
    /// Center in scene user-units.
    pub x:        f64,
    pub y:        f64,
    /// Standard deviation along local x axis.
    pub sigma_x:  f64,
    /// Standard deviation along local y axis.
    pub sigma_y:  f64,
    /// Rotation of the ellipse in radians.
    pub rotation: f64,
    /// Peak color at center.
    pub color:    Color,
    /// Peak opacity at center (0.0..=1.0).
    pub opacity:  f64,
    pub id:       Option<String>,
}

impl GaussianSplat {
    pub fn new(x: f64, y: f64, sigma_x: f64, sigma_y: f64, color: Color, opacity: f64) -> Self {
        GaussianSplat { x, y, sigma_x, sigma_y, rotation: 0.0, color, opacity, id: None }
    }

    /// Convenience: circular (isotropic) splat.
    pub fn circle(cx: f64, cy: f64, sigma: f64, color: Color, opacity: f64) -> Self {
        GaussianSplat::new(cx, cy, sigma, sigma, color, opacity)
    }

    /// Evaluate the unnormalized gaussian value at (px, py) → 0.0..=1.0.
    /// Does not multiply by `self.opacity` — call `evaluate_opacity_at` for that.
    pub fn evaluate_at(&self, px: f64, py: f64) -> f64 {
        let dx = px - self.x;
        let dy = py - self.y;
        let (sin_r, cos_r) = self.rotation.sin_cos();
        let lx = dx * cos_r + dy * sin_r;
        let ly = dy * cos_r - dx * sin_r;
        (-(lx * lx) / (2.0 * self.sigma_x * self.sigma_x)
         -(ly * ly) / (2.0 * self.sigma_y * self.sigma_y)).exp()
    }

    /// Evaluate with peak opacity applied.
    pub fn evaluate_opacity_at(&self, px: f64, py: f64) -> f64 {
        self.evaluate_at(px, py) * self.opacity
    }

    /// Radius in pixels within which the splat contributes more than `threshold`.
    pub fn effective_radius(&self, threshold: f64) -> f64 {
        self.sigma_x.max(self.sigma_y) * (-2.0 * threshold.ln()).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn center_is_one() {
        let s = GaussianSplat::circle(100.0, 100.0, 20.0, Color::WHITE, 1.0);
        assert!((s.evaluate_at(100.0, 100.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn falls_off_at_sigma() {
        let s = GaussianSplat::circle(0.0, 0.0, 10.0, Color::WHITE, 1.0);
        let expected = (-0.5f64).exp();
        assert!((s.evaluate_at(10.0, 0.0) - expected).abs() < 1e-9);
    }

    #[test]
    fn effective_radius_range() {
        let s = GaussianSplat::circle(0.0, 0.0, 20.0, Color::WHITE, 1.0);
        let r = s.effective_radius(0.01);
        assert!(r > 40.0 && r < 500.0);
    }
  }
