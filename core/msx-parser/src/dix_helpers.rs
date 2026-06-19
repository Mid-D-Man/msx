// core/msx-parser/src/dix_helpers.rs
//! Shared helpers bridging `dixscript::Runtime::DixData` reads to msx-ast
//! types. Everything here calls into dixscript's *own* `DixDeserialize` /
//! `TryFrom<DixValue>` impls — it never tries to implement those traits on
//! foreign types, so there's no orphan-rule conflict.

use dixscript::Runtime::{DixData, DixDeserialize, DixValue, dix_path};
use msx_ast::{Color, Paint};

/// Raw `DixValue` at `prefix.field` (or just `field` if `prefix` is empty).
pub fn raw<'a>(data: &'a DixData, prefix: &str, field: &str) -> Option<&'a DixValue> {
    data.get_value(&dix_path(prefix, field))
}

/// Optional typed read at `prefix.field`. `Ok(None)` means absent;
/// `Err` means present but the wrong shape. Works for any `T` that already
/// implements dixscript's `DixDeserialize` — every primitive (`f64`,
/// `String`, `bool`, `i32`, `i64`) and `Vec<T>` of those, via dixscript's own
/// blanket impls.
pub fn opt<T: DixDeserialize>(data: &DixData, prefix: &str, field: &str) -> Result<Option<T>, String> {
    data.deserialize_at::<Option<T>>(&dix_path(prefix, field))
}

/// `true` if `prefix` resolves to a value itself, or has at least one child
/// key `prefix.*` — i.e. "is this aggregate present at all", mirroring the
/// presence check dixscript's own `Option<T>` impl uses internally.
pub fn path_present(data: &DixData, path: &str) -> bool {
    data.exists(path) || !data.get_keys(path).is_empty()
}

/// Length of the array at `prefix.field`, or 0 if absent.
pub fn array_len(data: &DixData, prefix: &str, field: &str) -> Result<usize, String> {
    match raw(data, prefix, field) {
        None => Ok(0),
        Some(DixValue::Array(items)) => Ok(items.len()),
        Some(other) => Err(format!(
            "{}: expected array, got {}",
            dix_path(prefix, field),
            other.type_name()
        )),
    }
}

/// Convert a string-like `DixValue` (`String` or `HexColor`) into a `Paint`.
/// Anything else (missing, wrong type) yields `None` — callers decide the
/// fallback (usually `Paint::None`, since that's the SVG-safe default).
pub fn paint_from(value: Option<&DixValue>) -> Option<Paint> {
    match value {
        Some(DixValue::String(s)) | Some(DixValue::HexColor(s)) => Some(Paint::parse(s)),
        _ => None,
    }
}

/// Same idea, but resolving straight to a `Color` (for backgrounds, gradient
/// stops, splat tints, effect colors — places that don't accept `url(#..)`).
pub fn color_from(value: Option<&DixValue>) -> Option<Color> {
    match value {
        Some(DixValue::String(s)) | Some(DixValue::HexColor(s)) => Color::parse(s),
        _ => None,
    }
}

/// Pull the integer discriminant out of a `DixValue::Enum`, ignoring every
/// other variant. Used for fields authored as `BlendMode.Multiply` etc.
/// instead of a plain string — see `dix_helpers` callers for the
/// string-fallback half of each of these reads.
pub fn enum_discriminant(value: Option<&DixValue>) -> Option<i32> {
    match value {
        Some(DixValue::Enum { value, .. }) => Some(*value),
        _ => None,
    }
}

/// Read the `type` discriminator string off the object at `prefix`. Every
/// recursive dispatcher (element / sdf tree / effect / transform-object)
/// goes through this.
pub fn type_tag(data: &DixData, prefix: &str) -> Result<String, String> {
    match raw(data, prefix, "type") {
        Some(DixValue::String(s)) => Ok(s.clone()),
        Some(other) => Err(format!("{}.type: expected string, got {}", prefix, other.type_name())),
        None => Err(format!("{}.type: missing required field", prefix)),
    }
}
