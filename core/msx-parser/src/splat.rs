// core/msx-parser/src/splat.rs
use dixscript::Runtime::DixData;
use msx_ast::{Color, GaussianSplat};

use crate::dix_helpers::{color_from, opt, paint_from, raw};

/// `{ type = "splat", x=.., y=.., sigma_x=.., sigma_y=.. (defaults to
///   sigma_x, i.e. circular), rotation=0.0, color=#.., fill="url(#id)"?,
///   opacity=1.0 }`
///
/// `fill`, if present, takes priority over `color` at render time — see
/// `GaussianSplat::fill`'s doc comment. Both keys are still parsed either
/// way (cheap, and lets a scene keep `color` around as a fallback/preview
/// value even when `fill` is set) — it's only the *render-time* choice
/// between them that's exclusive, not the parse.
///
/// No `transform` field — `GaussianSplat` has none in msx-ast (its position
/// is already fully expressed by `x`/`y`/`sigma_x`/`sigma_y`/`rotation`).
pub fn parse_splat(data: &DixData, prefix: &str) -> Result<GaussianSplat, String> {
    let x = opt::<f64>(data, prefix, "x")?.ok_or_else(|| format!("{}.x: required", prefix))?;
    let y = opt::<f64>(data, prefix, "y")?.ok_or_else(|| format!("{}.y: required", prefix))?;
    let sigma_x = opt::<f64>(data, prefix, "sigma_x")?
        .ok_or_else(|| format!("{}.sigma_x: required", prefix))?;
    let sigma_y = opt::<f64>(data, prefix, "sigma_y")?.unwrap_or(sigma_x);
    let rotation = opt::<f64>(data, prefix, "rotation")?.unwrap_or(0.0);
    let color = color_from(raw(data, prefix, "color")).unwrap_or(Color::WHITE);
    let fill = paint_from(raw(data, prefix, "fill"));
    let opacity = opt::<f64>(data, prefix, "opacity")?.unwrap_or(1.0);
    let id = opt::<String>(data, prefix, "id")?;

    Ok(GaussianSplat { x, y, sigma_x, sigma_y, rotation, color, fill, opacity, id })
}
