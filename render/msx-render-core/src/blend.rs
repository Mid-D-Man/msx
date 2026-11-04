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

/// W3C `color-dodge`: a black backdrop stays black, a white source gives
/// white, everything else brightens the backdrop by `1 / (1 - cs)`.
pub fn color_dodge(cb: f32, cs: f32) -> f32 {
    if cb <= 0.0 {
        0.0
    } else if cs >= 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

/// W3C `color-burn`: a white backdrop stays white, a black source gives
/// black, everything else darkens the backdrop by `(1 - cb) / cs`.
pub fn color_burn(cb: f32, cs: f32) -> f32 {
    if cb >= 1.0 {
        1.0
    } else if cs <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

// Non-separable helpers, as written in the W3C Compositing and Blending spec.
// A color is three straight channels in 0..=1.

pub type Rgb = [f32; 3];

pub fn lum(c: Rgb) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

pub fn sat(c: Rgb) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// Pulls a color back into the unit cube along the line to its luminosity.
pub fn clip_color(c: Rgb) -> Rgb {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        for v in &mut out {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut out {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    out
}

pub fn set_lum(c: Rgb, l: f32) -> Rgb {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

/// Scales a color so its largest channel equals `s` and its smallest is zero.
/// A gray color (no spread) becomes black, as in the spec.
pub fn set_sat(c: Rgb, s: f32) -> Rgb {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx > mn {
        let k = s / (mx - mn);
        [(c[0] - mn) * k, (c[1] - mn) * k, (c[2] - mn) * k]
    } else {
        [0.0; 3]
    }
}

pub fn hue(cb: Rgb, cs: Rgb) -> Rgb {
    set_lum(set_sat(cs, sat(cb)), lum(cb))
}

pub fn saturation(cb: Rgb, cs: Rgb) -> Rgb {
    set_lum(set_sat(cb, sat(cs)), lum(cb))
}

pub fn color(cb: Rgb, cs: Rgb) -> Rgb {
    set_lum(cs, lum(cb))
}

pub fn luminosity(cb: Rgb, cs: Rgb) -> Rgb {
    set_lum(cb, lum(cs))
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
        BlendMode::ColorDodge => color_dodge,
        BlendMode::ColorBurn => color_burn,
        // Non-separable modes and `PlusLighter` never reach this table:
        // `blend_rgb` and `composite` take them first.
        BlendMode::Hue
        | BlendMode::Saturation
        | BlendMode::Color
        | BlendMode::Luminosity
        | BlendMode::PlusLighter => |_cb, cs| cs,
    }
}

/// The blend function `B(Cb, Cs)` on all three channels. `PlusLighter` is not
/// a blend function (it replaces the compositing formula, see `composite`), so
/// it reads as `Normal` here.
pub fn blend_rgb(mode: BlendMode, cb: Rgb, cs: Rgb) -> Rgb {
    match mode {
        BlendMode::Hue => hue(cb, cs),
        BlendMode::Saturation => saturation(cb, cs),
        BlendMode::Color => color(cb, cs),
        BlendMode::Luminosity => luminosity(cb, cs),
        _ => {
            let f = blend_function(mode);
            [f(cb[0], cs[0]), f(cb[1], cs[1]), f(cb[2], cs[2])]
        }
    }
}

/// Porter-Duff plus on straight colors: premultiplied color and alpha are
/// added and clamped, then the color is straightened again.
fn plus_lighter(
    backdrop: (f32, f32, f32, f32),
    source: (f32, f32, f32, f32),
) -> (f32, f32, f32, f32) {
    let ao = (backdrop.3 + source.3).min(1.0);
    if ao <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let ch = |cb: f32, cs: f32| ((cb * backdrop.3 + cs * source.3).min(1.0)) / ao;
    (ch(backdrop.0, source.0), ch(backdrop.1, source.1), ch(backdrop.2, source.2), ao)
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
    if mode == BlendMode::PlusLighter {
        return plus_lighter(backdrop, source);
    }

    let (cb_r, cb_g, cb_b, ab) = backdrop;
    let (cs_r, cs_g, cs_b, a_src) = source;

    let ao = a_src + ab * (1.0 - a_src);
    if ao <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    let blended = blend_rgb(mode, [cb_r, cb_g, cb_b], [cs_r, cs_g, cs_b]);
    let mix = |cb: f32, cs: f32, b: f32| -> f32 {
        let premixed = (1.0 - ab) * cs + ab * b;
        (1.0 - a_src / ao) * cb + (a_src / ao) * premixed
    };

    (mix(cb_r, cs_r, blended[0]), mix(cb_g, cs_g, blended[1]), mix(cb_b, cs_b, blended[2]), ao)
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

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn color_dodge_edges() {
        assert_eq!(color_dodge(0.0, 1.0), 0.0);
        assert_eq!(color_dodge(0.5, 1.0), 1.0);
        assert!(close(color_dodge(0.25, 0.5), 0.5));
        assert_eq!(color_dodge(0.5, 0.5), 1.0);
        assert!(close(color_dodge(0.4, 0.0), 0.4));
    }

    #[test]
    fn color_burn_edges() {
        assert_eq!(color_burn(1.0, 0.0), 1.0);
        assert_eq!(color_burn(0.5, 0.0), 0.0);
        assert!(close(color_burn(0.75, 0.5), 0.5));
        assert_eq!(color_burn(0.25, 0.5), 0.0);
        assert!(close(color_burn(0.4, 1.0), 0.4));
    }

    #[test]
    fn lum_and_sat_basics() {
        assert!(close(lum([1.0, 1.0, 1.0]), 1.0));
        assert_eq!(lum([0.0, 0.0, 0.0]), 0.0);
        assert_eq!(sat([0.4, 0.4, 0.4]), 0.0);
        assert!(close(sat([0.1, 0.9, 0.4]), 0.8));
    }

    #[test]
    fn set_sat_pins_max_to_s_and_min_to_zero() {
        let c = set_sat([0.2, 0.6, 0.4], 0.5);
        assert!(close(c[0], 0.0));
        assert!(close(c[1], 0.5));
        assert!(close(c[2], 0.25));
        assert_eq!(set_sat([0.3, 0.3, 0.3], 0.9), [0.0; 3]);
    }

    #[test]
    fn color_mode_worked_example_clips_into_gamut() {
        // Hand computed: set_lum((0,1,0), 0.3) first lands at (-0.29, 0.71, -0.29)
        // and clip_color pulls it back to (0, 0.50847, 0).
        let c = color([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(close(c[0], 0.0));
        assert!(close(c[1], 0.50847));
        assert!(close(c[2], 0.0));
        assert!(close(lum(c), 0.3));
    }

    #[test]
    fn luminosity_with_the_same_color_is_the_identity() {
        for c in [[0.2, 0.5, 0.9], [1.0, 0.0, 0.0], [0.3, 0.3, 0.3], [0.9, 0.8, 0.1]] {
            let r = luminosity(c, c);
            for i in 0..3 {
                assert!(close(r[i], c[i]), "{c:?} -> {r:?}");
            }
        }
    }

    #[test]
    fn color_and_hue_keep_the_backdrop_luminosity() {
        let cb = [0.6, 0.4, 0.3];
        let cs = [0.2, 0.7, 0.5];
        assert!(close(lum(color(cb, cs)), lum(cb)));
        assert!(close(lum(hue(cb, cs)), lum(cb)));
        assert!(close(lum(saturation(cb, cs)), lum(cb)));
    }

    #[test]
    fn hue_and_saturation_against_gray_give_gray_of_the_backdrop_luminosity() {
        let cb = [0.6, 0.4, 0.3];
        let gray = [0.5, 0.5, 0.5];
        let h = saturation(cb, gray);
        for v in h {
            assert!(close(v, lum(cb)));
        }
        let h = hue([0.4, 0.4, 0.4], [0.9, 0.2, 0.1]);
        for v in h {
            assert!(close(v, 0.4));
        }
    }

    #[test]
    fn non_separable_modes_through_composite_on_opaque_layers() {
        let cb = [0.6, 0.4, 0.3];
        let cs = [0.2, 0.7, 0.5];
        let want = luminosity(cb, cs);
        let got = composite(BlendMode::Luminosity, (cb[0], cb[1], cb[2], 1.0), (cs[0], cs[1], cs[2], 1.0));
        assert!(close(got.0, want[0]) && close(got.1, want[1]) && close(got.2, want[2]) && close(got.3, 1.0));
    }

    #[test]
    fn plus_lighter_adds_premultiplied_color_and_alpha() {
        let r = composite(BlendMode::PlusLighter, (0.2, 0.2, 0.2, 0.5), (0.4, 0.4, 0.4, 0.25));
        // premultiplied: 0.1 + 0.1 = 0.2 over alpha 0.75
        assert!(close(r.0, 0.2 / 0.75));
        assert!(close(r.3, 0.75));
    }

    #[test]
    fn plus_lighter_clamps_and_matches_add_when_everything_is_opaque() {
        let r = composite(BlendMode::PlusLighter, (0.7, 0.2, 0.0, 1.0), (0.6, 0.2, 0.0, 1.0));
        assert!(close(r.0, 1.0) && close(r.1, 0.4) && close(r.2, 0.0) && close(r.3, 1.0));
        let a = composite(BlendMode::Add, (0.7, 0.2, 0.0, 1.0), (0.6, 0.2, 0.0, 1.0));
        assert!(close(a.0, r.0) && close(a.1, r.1) && close(a.3, r.3));
    }

    #[test]
    fn plus_lighter_with_nothing_is_transparent() {
        assert_eq!(composite(BlendMode::PlusLighter, (0.5, 0.5, 0.5, 0.0), (0.9, 0.9, 0.9, 0.0)), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn every_mode_stays_inside_the_unit_range() {
        let vals = [0.0f32, 0.3, 0.7, 1.0];
        let alphas = [0.0f32, 0.5, 1.0];
        for mode in BlendMode::ALL {
            for &r1 in &vals { for &g1 in &vals { for &b1 in &vals {
                for &r2 in &[0.0f32, 0.5, 1.0] { for &g2 in &[0.2f32, 1.0] { for &b2 in &[0.0f32, 0.6] {
                    for &ab in &alphas { for &asrc in &alphas {
                        let (r, g, b, a) = composite(mode, (r1, g1, b1, ab), (r2, g2, b2, asrc));
                        for v in [r, g, b, a] {
                            assert!(v.is_finite() && (-1e-4..=1.0 + 1e-4).contains(&v), "{mode:?}: {v} out of range");
                        }
                    }}
                }}}
            }}}
        }
    }
}
