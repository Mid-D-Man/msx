// core/msx-parser/src/splat.rs
use dixscript::Runtime::DixData;
use msx_ast::{Color, GaussianSplat};

use crate::dix_helpers::{color_from, opt, raw};

/// `{ type = "splat", x=.., y=.., sigma_x=.., sigma_y=.. (defaults to
///   sigma_x, i.e. circular), rotation=0.0, color=#.., opacity=1.0 }`
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
    let opacity = opt::<f64>(data, prefix, "opacity")?.unwrap_or(1.0);
    let id = opt::<String>(data, prefix, "id")?;

    Ok(GaussianSplat { x, y, sigma_x, sigma_y, rotation, color, opacity, id })
}
