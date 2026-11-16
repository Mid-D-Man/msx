// core/msx-svg-import/src/color_matrix.rs
//! The 4x5 matrices behind `feColorMatrix` and the CSS color filter functions.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Filters". A matrix is the 20
//! numbers of `Effect::ColorMatrix`, row major, one row each for the new red,
//! green, blue and alpha. The formulas are the ones of the SVG 1.1 and Filter
//! Effects specifications.

pub(crate) type Matrix = [f64; 20];

#[rustfmt::skip]
pub(crate) const IDENTITY: Matrix = [
    1.0, 0.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.0, 1.0, 0.0,
];

/// The matrix rounded to six decimals, so the trigonometry of a hue rotation
/// does not leave values like 0.00000000000000005 in the written source.
pub(crate) fn tidy(m: Matrix) -> Matrix {
    m.map(|v| {
        let r = (v * 1e6).round() / 1e6;
        if r == 0.0 { 0.0 } else { r }
    })
}

pub(crate) fn is_identity(m: &Matrix) -> bool {
    m.iter().zip(IDENTITY.iter()).all(|(a, b)| (a - b).abs() < 1e-9)
}

/// `feColorMatrix type="saturate"`. Values from 0 to 1 are what the SVG 1.1
/// specification allows. Larger values oversaturate, negative values invert
/// the chroma, and both are left to the caller to decide on.
#[rustfmt::skip]
pub(crate) fn saturate(s: f64) -> Matrix {
    [
        0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s, 0.0, 0.0,
        0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s, 0.0, 0.0,
        0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s, 0.0, 0.0,
        0.0,               0.0,               0.0,               1.0, 0.0,
    ]
}

/// `feColorMatrix type="hueRotate"`, the angle in degrees.
#[rustfmt::skip]
pub(crate) fn hue_rotate(degrees: f64) -> Matrix {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [
        0.213 + cos * 0.787 - sin * 0.213, 0.715 - cos * 0.715 - sin * 0.715, 0.072 - cos * 0.072 + sin * 0.928, 0.0, 0.0,
        0.213 - cos * 0.213 + sin * 0.143, 0.715 + cos * 0.285 + sin * 0.140, 0.072 - cos * 0.072 - sin * 0.283, 0.0, 0.0,
        0.213 - cos * 0.213 - sin * 0.787, 0.715 - cos * 0.715 + sin * 0.715, 0.072 + cos * 0.928 + sin * 0.072, 0.0, 0.0,
        0.0,                               0.0,                               0.0,                               1.0, 0.0,
    ]
}

/// `feColorMatrix type="luminanceToAlpha"`: the color goes to black and the
/// alpha becomes the luminance.
#[rustfmt::skip]
pub(crate) fn luminance_to_alpha() -> Matrix {
    [
        0.0,    0.0,    0.0,    0.0, 0.0,
        0.0,    0.0,    0.0,    0.0, 0.0,
        0.0,    0.0,    0.0,    0.0, 0.0,
        0.2125, 0.7154, 0.0721, 0.0, 0.0,
    ]
}

/// The matrix of one `feColorMatrix` from its `type` and `values`. A
/// missing or unusable `values` leaves the identity, the way a missing `type`
/// does, and a `type` that is not one of the four falls back to `matrix`.
pub(crate) fn from_primitive(kind: Option<&str>, values: Option<&[f64]>) -> Matrix {
    match kind.map(str::trim) {
        Some("saturate") => match values {
            Some([]) => IDENTITY,
            Some(v) => saturate(v[0].clamp(0.0, 1.0)),
            None => IDENTITY,
        },
        Some("hueRotate") => match values {
            Some([]) | None => IDENTITY,
            Some(v) => hue_rotate(v[0]),
        },
        Some("luminanceToAlpha") => luminance_to_alpha(),
        _ => match values {
            Some(v) if v.len() == 20 => v.try_into().unwrap_or(IDENTITY),
            _ => IDENTITY,
        },
    }
}

/// `grayscale(amount)`, the amount from 0 (unchanged) to 1 (gray).
#[rustfmt::skip]
pub(crate) fn grayscale(amount: f64) -> Matrix {
    let s = 1.0 - amount.clamp(0.0, 1.0);
    [
        0.2126 + 0.7874 * s, 0.7152 - 0.7152 * s, 0.0722 - 0.0722 * s, 0.0, 0.0,
        0.2126 - 0.2126 * s, 0.7152 + 0.2848 * s, 0.0722 - 0.0722 * s, 0.0, 0.0,
        0.2126 - 0.2126 * s, 0.7152 - 0.7152 * s, 0.0722 + 0.9278 * s, 0.0, 0.0,
        0.0,                 0.0,                 0.0,                 1.0, 0.0,
    ]
}

/// `sepia(amount)`, the amount from 0 (unchanged) to 1 (sepia).
#[rustfmt::skip]
pub(crate) fn sepia(amount: f64) -> Matrix {
    let s = 1.0 - amount.clamp(0.0, 1.0);
    [
        0.393 + 0.607 * s, 0.769 - 0.769 * s, 0.189 - 0.189 * s, 0.0, 0.0,
        0.349 - 0.349 * s, 0.686 + 0.314 * s, 0.168 - 0.168 * s, 0.0, 0.0,
        0.272 - 0.272 * s, 0.534 - 0.534 * s, 0.131 + 0.869 * s, 0.0, 0.0,
        0.0,               0.0,               0.0,               1.0, 0.0,
    ]
}

/// `brightness(amount)`: every channel times the amount.
#[rustfmt::skip]
pub(crate) fn brightness(amount: f64) -> Matrix {
    [
        amount, 0.0,    0.0,    0.0, 0.0,
        0.0,    amount, 0.0,    0.0, 0.0,
        0.0,    0.0,    amount, 0.0, 0.0,
        0.0,    0.0,    0.0,    1.0, 0.0,
    ]
}

/// `contrast(amount)`: every channel scaled around the middle gray.
#[rustfmt::skip]
pub(crate) fn contrast(amount: f64) -> Matrix {
    let c = 0.5 - 0.5 * amount;
    [
        amount, 0.0,    0.0,    0.0, c,
        0.0,    amount, 0.0,    0.0, c,
        0.0,    0.0,    amount, 0.0, c,
        0.0,    0.0,    0.0,    1.0, 0.0,
    ]
}

/// `invert(amount)`, the amount from 0 (unchanged) to 1 (inverted).
#[rustfmt::skip]
pub(crate) fn invert(amount: f64) -> Matrix {
    let a = amount.clamp(0.0, 1.0);
    let d = 1.0 - 2.0 * a;
    [
        d,   0.0, 0.0, 0.0, a,
        0.0, d,   0.0, 0.0, a,
        0.0, 0.0, d,   0.0, a,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ]
}

/// `opacity(amount)`, the amount from 0 (transparent) to 1 (unchanged).
#[rustfmt::skip]
pub(crate) fn opacity(amount: f64) -> Matrix {
    let mut m = IDENTITY;
    m[18] = amount.clamp(0.0, 1.0);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(m: &Matrix, rgba: [f64; 4]) -> [f64; 4] {
        let row = |k: usize| m[k] * rgba[0] + m[k + 1] * rgba[1] + m[k + 2] * rgba[2] + m[k + 3] * rgba[3] + m[k + 4];
        [row(0), row(5), row(10), row(15)]
    }

    fn close(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn the_neutral_amounts_are_the_identity() {
        for m in [saturate(1.0), hue_rotate(0.0), hue_rotate(360.0), grayscale(0.0), sepia(0.0), brightness(1.0), contrast(1.0), invert(0.0), opacity(1.0)] {
            let probe = [0.2, 0.5, 0.9, 0.7];
            assert!(close(apply(&m, probe), probe), "{m:?}");
        }
        assert!(is_identity(&IDENTITY) && !is_identity(&saturate(0.5)));
    }

    #[test]
    fn full_amounts_do_what_the_names_say() {
        let gray = apply(&grayscale(1.0), [1.0, 0.0, 0.0, 1.0]);
        assert!(close(gray, [0.2126, 0.2126, 0.2126, 1.0]), "{gray:?}");
        assert!(close(apply(&saturate(0.0), [0.0, 0.0, 1.0, 1.0]), [0.072, 0.072, 0.072, 1.0]));
        assert!(close(apply(&invert(1.0), [0.2, 0.5, 1.0, 0.4]), [0.8, 0.5, 0.0, 0.4]));
        assert!(close(apply(&opacity(0.25), [0.2, 0.5, 1.0, 0.8]), [0.2, 0.5, 1.0, 0.2]));
        assert!(close(apply(&brightness(2.0), [0.2, 0.5, 1.0, 1.0]), [0.4, 1.0, 2.0, 1.0]));
        assert!(close(apply(&contrast(0.0), [0.1, 0.9, 0.5, 1.0]), [0.5, 0.5, 0.5, 1.0]));
        assert!(close(apply(&sepia(1.0), [1.0, 1.0, 1.0, 1.0]), [1.351, 1.203, 0.937, 1.0]));
        assert!(close(apply(&luminance_to_alpha(), [1.0, 1.0, 1.0, 1.0]), [0.0, 0.0, 0.0, 0.9999]));
    }

    #[test]
    fn tidy_rounds_to_six_decimals_and_removes_negative_zero() {
        let mut m = IDENTITY;
        m[1] = 0.000_000_000_000_000_055;
        m[2] = -1e-12;
        m[3] = 0.356_000_000_000_000_4;
        m[4] = -0.211 - 1e-13;
        let t = tidy(m);
        assert_eq!(t[1], 0.0);
        assert!(t[2].is_sign_positive() && t[2] == 0.0, "no negative zero");
        assert_eq!(t[3], 0.356);
        assert_eq!(t[4], -0.211);
    }

    #[test]
    fn a_hue_rotation_of_120_degrees_turns_red_toward_green_or_blue() {
        let m = hue_rotate(120.0);
        let out = apply(&m, [1.0, 0.0, 0.0, 1.0]);
        assert!(out[1] > out[0] && out[1] > out[2], "{out:?}");
    }

    #[test]
    fn a_fe_color_matrix_reads_its_type_and_values_the_way_the_specification_and_resvg_do() {
        assert!(is_identity(&from_primitive(None, None)));
        assert!(is_identity(&from_primitive(Some("qwe"), Some(&[1.0, 2.0]))), "an unknown type reads values as a matrix, which needs 20");
        assert!(is_identity(&from_primitive(Some("matrix"), Some(&[]))));
        assert!(is_identity(&from_primitive(Some("matrix"), Some(&[0.3; 19]))));
        assert!(is_identity(&from_primitive(Some("matrix"), Some(&[0.3; 21]))));
        assert!(is_identity(&from_primitive(Some("saturate"), None)));
        assert!(is_identity(&from_primitive(Some("hueRotate"), None)));
        let twenty: Vec<f64> = (0..20).map(f64::from).collect();
        assert_eq!(from_primitive(Some("matrix"), Some(&twenty))[7], 7.0);
        assert_eq!(from_primitive(None, Some(&twenty))[19], 19.0, "no type means matrix");
        assert_eq!(from_primitive(Some("saturate"), Some(&[0.5])), saturate(0.5));
        assert_eq!(from_primitive(Some("saturate"), Some(&[99.0])), saturate(1.0), "held to 0 to 1");
        assert_eq!(from_primitive(Some("saturate"), Some(&[-0.5])), saturate(0.0));
        assert_eq!(from_primitive(Some("hueRotate"), Some(&[90.0])), hue_rotate(90.0));
        assert_eq!(from_primitive(Some("luminanceToAlpha"), None), luminance_to_alpha());
    }
}
