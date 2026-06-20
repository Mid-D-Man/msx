// render/msx-render-core/src/blend.rs
//! Per-channel separable blend-mode functions (W3C Compositing and Blending
//! spec for the CSS-standard modes; classic Photoshop-style formulas for
//! `Add`/`Subtract`/`Divide`, which aren't in that spec but are exactly
//! what `msx-render-svg`'s CSS fallback already leans on for `Add` via
//! `plus-lighter`), plus the general alpha-compositing formula that mixes a
//! blend function in with source/backdrop alpha.

use msx_ast::BlendMode;

pub fn multiply(cb: f32, cs: f32) -> f32 {
    cb * cs
}

pub fn screen(cb: f32, cs: f32) -> f32 {
    cb + cs - cb * cs
}

pub fn darken(cb: f32, cs: f32) -> f32 {
    cb.min(cs)
}

pub fn lighten(cb: f32, cs: f32) -> f32 {
    cb.max(cs)
}

pub fn difference(cb: f32, cs: f32) -> f32 {
    (cb - cs).abs()
}

pub fn exclusion(cb: f32, cs: f32) -> f32 {
    cb + cs - 2.0 * cb * cs
}

pub fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 { multiply(cb, 2.0 * cs) } else { screen(cb, 2.0 * cs - 1.0) }
}

/// `Overlay(Cb,Cs) = HardLight(Cs,Cb)` — defined exactly this way (swapped
/// arguments) in the W3C spec, not an independent formula.
pub fn overlay(cb: f32, cs: f32) -> f32 {
    hard_light(cs, cb)
}

pub fn soft_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 {
            ((16.0 * cb - 12.0) * cb + 4.0) * cb
        } else {
            cb.sqrt()
        };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

/// "Linear Dodge" in Photoshop terms — what CSS `plus-lighter` approximates.
pub fn add(cb: f32, cs: f32) -> f32 {
    (cb + cs).min(1.0)
}

pub fn subtract(cb: f32, cs: f32) -> f32 {
    (cb - cs).max(0.0)
}

pub fn divide(cb: f32, cs: f32) -> f32 {
    if cs <= 0.0 { 1.0 } else { (cb / cs).min(1.0) }
}

fn blend_function(mode: BlendMode) -> fn(f32, f32) -> f32 {
    match mode {
        BlendMode::Normal => |_cb, cs| cs,
        BlendMode::Multiply => multiply,
        BlendMode::Screen => screen,
        BlendMode::Overlay => overlay,
        BlendMode::Add => add,
        BlendMode::SoftLight => soft_light,
        BlendMode::HardLight => hard_light,
        BlendMode::Difference => difference,
        BlendMode::Exclusion => exclusion,
        BlendMode::Darken => darken,
        BlendMode::Lighten => lighten,
        BlendMode::Subtract => subtract,
        BlendMode::Divide => divide,
    }
}

/// General alpha compositing with a blend mode mixed in — the W3C "simple
/// alpha compositing" formula with `B(Cb,Cs)` substituted for the blend
/// function. All channels straight (non-premultiplied) 0.0..=1.0 —
/// `color.rs::PremulColor` is for callers who don't need a blend mode at
/// all and can skip straight to premultiplied "over".
pub fn composite(
    mode: BlendMode,
    backdrop: (f32, f32, f32, f32),
    source: (f32, f32, f32, f32),
) -> (f32, f32, f32, f32) {
    let (cb_r, cb_g, cb_b, ab) = backdrop;
    let (cs_r, cs_g, cs_b, a_src) = source;

    let ao = a_src + ab * (1.0 - a_src);
    if ao <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    let blend_fn = blend_function(mode);
    let mix = |cb: f32, cs: f32| -> f32 {
        let blended = blend_fn(cb, cs);
        let premixed = (1.0 - ab) * cs + ab * blended;
        (1.0 - a_src / ao) * cb + (a_src / ao) * premixed
    };

    (mix(cb_r, cs_r), mix(cb_g, cs_g), mix(cb_b, cs_b), ao)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_blend_opaque_source_replaces_backdrop() {
        let result = composite(BlendMode::Normal, (0.2, 0.4, 0.6, 1.0), (1.0, 0.0, 0.0, 1.0));
        assert!((result.0 - 1.0).abs() < 1e-5);
        assert!((result.1 - 0.0).abs() < 1e-5);
        assert!((result.3 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn multiply_with_white_backdrop_keeps_source() {
        let result = composite(BlendMode::Multiply, (1.0, 1.0, 1.0, 1.0), (0.5, 0.5, 0.5, 1.0));
        assert!((result.0 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn multiply_with_black_backdrop_yields_black() {
        let result = composite(BlendMode::Multiply, (0.0, 0.0, 0.0, 1.0), (0.9, 0.9, 0.9, 1.0));
        assert!(result.0.abs() < 1e-5);
    }

    #[test]
    fn zero_alpha_source_leaves_backdrop_untouched() {
        let result = composite(BlendMode::Multiply, (0.3, 0.6, 0.9, 1.0), (1.0, 1.0, 1.0, 0.0));
        assert!((result.0 - 0.3).abs() < 1e-5);
        assert!((result.1 - 0.6).abs() < 1e-5);
        assert!((result.2 - 0.9).abs() < 1e-5);
    }

    #[test]
    fn screen_is_brighter_than_either_input() {
        assert!(screen(0.5, 0.5) > 0.5);
    }

    #[test]
    fn overlay_matches_hardlight_with_swapped_args() {
        assert_eq!(overlay(0.3, 0.7), hard_light(0.7, 0.3));
    }

    #[test]
    fn divide_by_zero_saturates_to_white() {
        assert_eq!(divide(0.5, 0.0), 1.0);
    }
}
