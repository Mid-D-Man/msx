// primitives/msx-splat/src/compositor.rs
//! Accumulates the painter's-algorithm "over" composite of however many
//! Gaussian splats touch a given pixel, in back-to-front (list) order.
//! Doesn't know how to cull which splats are relevant — that's a spatial
//! query `msx-render-cpu`/`-gpu` own — this just does the blending math
//! once you've already decided which contribute.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Accumulator {
    color: [f32; 3],
    alpha: f32,
}

impl Accumulator {
    pub fn new() -> Self {
        Self { color: [0.0; 3], alpha: 0.0 }
    }

    /// Composite one more contribution "over" everything accumulated so
    /// far. `src_alpha` should already be the splat's Gaussian falloff
    /// multiplied by its peak opacity (`gaussian::evaluate_opacity`) — this
    /// only does the blending, not the Gaussian math.
    pub fn accumulate(&mut self, src_color: [f32; 3], src_alpha: f32) {
        let src_alpha = src_alpha.clamp(0.0, 1.0);
        let inv = 1.0 - src_alpha;
        for c in 0..3 {
            self.color[c] = src_color[c] * src_alpha + self.color[c] * inv;
        }
        self.alpha = src_alpha + self.alpha * inv;
    }

    pub fn result(&self) -> Rgba {
        Rgba { r: self.color[0], g: self.color[1], b: self.color[2], a: self.alpha }
    }

    pub fn is_fully_opaque(&self) -> bool {
        self.alpha >= 1.0 - f32::EPSILON
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_full_alpha_contribution_replaces_background() {
        let mut acc = Accumulator::new();
        acc.accumulate([1.0, 0.0, 0.0], 1.0);
        let r = acc.result();
        assert_eq!(r.r, 1.0);
        assert_eq!(r.a, 1.0);
    }

    #[test]
    fn zero_alpha_contribution_is_a_no_op() {
        let mut acc = Accumulator::new();
        acc.accumulate([1.0, 0.0, 0.0], 0.0);
        assert_eq!(acc.result(), Rgba::TRANSPARENT);
    }

    #[test]
    fn two_half_alpha_layers_accumulate_more_coverage() {
        let mut acc = Accumulator::new();
        acc.accumulate([1.0, 1.0, 1.0], 0.5);
        acc.accumulate([1.0, 1.0, 1.0], 0.5);
        assert!((acc.result().a - 0.75).abs() < 1e-5); // 0.5 + 0.5*(1-0.5)
    }

    #[test]
    fn fully_opaque_detection() {
        let mut acc = Accumulator::new();
        assert!(!acc.is_fully_opaque());
        acc.accumulate([0.0, 0.0, 0.0], 1.0);
        assert!(acc.is_fully_opaque());
    }
  }
