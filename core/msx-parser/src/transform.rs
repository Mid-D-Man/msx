// core/msx-parser/src/transform.rs
use dixscript::Runtime::{DixData, DixValue, dix_path};
use msx_ast::{Matrix2D, Transform};

use crate::dix_helpers::{opt, type_tag};

/// Parse an optional `transform` field on the element at `element_prefix`.
///
/// Accepts three source shapes:
///   - SVG string:    `transform = "translate(10,20) rotate(45)"`
///   - Single object: `transform = { type = "translate", x = 10, y = 20 }`
///   - Chain array:   `transform = [ { type = "rotate", angle = 45 }, ... ]`
pub fn parse_transform(data: &DixData, element_prefix: &str) -> Result<Option<Transform>, String> {
    let path = dix_path(element_prefix, "transform");
    match data.get_value(&path) {
        None => Ok(None),
        Some(DixValue::String(s)) => {
            let t = Transform::parse_svg(s);
            Ok(if t.is_none() { None } else { Some(t) })
        }
        Some(DixValue::Object(_)) => Ok(Some(parse_transform_object(data, &path)?)),
        Some(DixValue::Array(items)) => {
            let mut chain = Vec::with_capacity(items.len());
            for i in 0..items.len() {
                chain.push(parse_transform_object(data, &format!("{}[{}]", path, i))?);
            }
            Ok(Some(Transform::Multiple(chain)))
        }
        Some(other) => Err(format!("{}: cannot parse transform from {}", path, other.type_name())),
    }
}

fn parse_transform_object(data: &DixData, prefix: &str) -> Result<Transform, String> {
    let kind = type_tag(data, prefix)?;
    match kind.as_str() {
        "translate" => Ok(Transform::Translate {
            x: opt::<f64>(data, prefix, "x")?.unwrap_or(0.0),
            y: opt::<f64>(data, prefix, "y")?.unwrap_or(0.0),
        }),
        "scale" => {
            let x = opt::<f64>(data, prefix, "x")?.unwrap_or(1.0);
            let y = opt::<f64>(data, prefix, "y")?.unwrap_or(x);
            Ok(Transform::Scale { x, y })
        }
        "rotate" => Ok(Transform::Rotate {
            angle: opt::<f64>(data, prefix, "angle")?.unwrap_or(0.0),
            cx: opt::<f64>(data, prefix, "cx")?,
            cy: opt::<f64>(data, prefix, "cy")?,
        }),
        "skew_x" => Ok(Transform::SkewX(opt::<f64>(data, prefix, "angle")?.unwrap_or(0.0))),
        "skew_y" => Ok(Transform::SkewY(opt::<f64>(data, prefix, "angle")?.unwrap_or(0.0))),
        "matrix" => Ok(Transform::Matrix(Matrix2D {
            a: opt::<f64>(data, prefix, "a")?.unwrap_or(1.0),
            b: opt::<f64>(data, prefix, "b")?.unwrap_or(0.0),
            c: opt::<f64>(data, prefix, "c")?.unwrap_or(0.0),
            d: opt::<f64>(data, prefix, "d")?.unwrap_or(1.0),
            e: opt::<f64>(data, prefix, "e")?.unwrap_or(0.0),
            f: opt::<f64>(data, prefix, "f")?.unwrap_or(0.0),
        })),
        other => Err(format!("{}: unknown transform type '{}'", prefix, other)),
    }
                           }
