// core/msx-parser/src/gradient.rs
use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{Color, ConicGradient, Def, LinearGradient, RadialGradient, Stop};

use crate::dix_helpers::{array_len, color_from, opt, raw, type_tag};

/// Parse the `defs::` (or nested `defs = [...]`) array on the canvas, or on
/// any future scope that wants its own local defs.
pub fn parse_defs(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Def>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(parse_def(data, &format!("{}[{}]", path, i))?);
    }
    Ok(out)
}

fn parse_def(data: &DixData, prefix: &str) -> Result<Def, String> {
    let kind = type_tag(data, prefix)?;
    match kind.as_str() {
        "linear_gradient" => {
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            let x1 = opt::<f64>(data, prefix, "x1")?.unwrap_or(0.0);
            let y1 = opt::<f64>(data, prefix, "y1")?.unwrap_or(0.0);
            let x2 = opt::<f64>(data, prefix, "x2")?.unwrap_or(1.0);
            let y2 = opt::<f64>(data, prefix, "y2")?.unwrap_or(0.0);
            let stops = parse_stops(data, prefix, "stops")?;
            Ok(Def::LinearGradient(LinearGradient::new(id, x1, y1, x2, y2, stops)))
        }
        "radial_gradient" => {
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            let cx = opt::<f64>(data, prefix, "cx")?.unwrap_or(0.5);
            let cy = opt::<f64>(data, prefix, "cy")?.unwrap_or(0.5);
            let r  = opt::<f64>(data, prefix, "r")?.unwrap_or(0.5);
            let fx = opt::<f64>(data, prefix, "fx")?.unwrap_or(cx);
            let fy = opt::<f64>(data, prefix, "fy")?.unwrap_or(cy);
            let stops = parse_stops(data, prefix, "stops")?;
            Ok(Def::RadialGradient(RadialGradient::new(id, cx, cy, r, fx, fy, stops)))
        }
        "conic_gradient" => {
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            let cx    = opt::<f64>(data, prefix, "cx")?.unwrap_or(0.5);
            let cy    = opt::<f64>(data, prefix, "cy")?.unwrap_or(0.5);
            let angle = opt::<f64>(data, prefix, "angle")?.unwrap_or(0.0);
            let stops = parse_stops(data, prefix, "stops")?;
            Ok(Def::ConicGradient(ConicGradient::new(id, cx, cy, angle, stops)))
        }
        other => Err(format!("{}: unknown def type '{}'", prefix, other)),
    }
}

/// `stops = [ { offset = 0.0, color = #f7971e, opacity = 1.0 }, ... ]`
/// Opacity folds into `Stop.color.a` — msx-ast's `Stop` has no separate
/// opacity field, matching `gradient.rs::to_svg()`'s use of `color.opacity()`.
pub fn parse_stops(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Stop>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let item_path = format!("{}[{}]", path, i);
        let offset = opt::<f64>(data, &item_path, "offset")?.unwrap_or(0.0);
        let mut color = color_from(raw(data, &item_path, "color")).unwrap_or(Color::BLACK);
        if let Some(op) = opt::<f64>(data, &item_path, "opacity")? {
            color.a = (op.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        out.push(Stop::new(offset, color));
    }
    Ok(out)
  }
