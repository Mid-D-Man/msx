// core/msx-parser/src/dix_helpers.rs
//! Shared helpers bridging `dixscript::Runtime::DixData` reads to msx-ast
//! types. Everything here calls into dixscript's *own* `DixDeserialize` /
//! `TryFrom<DixValue>` impls — it never tries to implement those traits on
//! foreign types, so there's no orphan-rule conflict.

use dixscript::Runtime::{DixData, DixDeserialize, DixValue, dix_path};
use msx_ast::{Color, MediaSource, Paint};

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

/// Shared by `element.rs`'s `image` parser and `gradient.rs`'s `audio` def
/// parser — exactly one of `data` (an embedded base64 string) or
/// `source_ref` (a file path, same field name and meaning as
/// `Def::Shader`'s own `source_ref` — see `gradient.rs`'s shader parsing
/// for why DixScript's lack of a raw/heredoc string type makes a plain
/// base64 string literal the only way to embed binary content inline at
/// all today) must be present.
///
/// Base64 decoding happens here, once — this is the ONLY place in the
/// whole pipeline that ever touches base64 in the decode direction (see
/// `core/msx-ast/src/media.rs`'s own module doc for the full boundary:
/// `msx-ast`/`msx-binary` only ever see already-decoded raw bytes;
/// `msx-render-svg` is the only place that touches base64 again at all,
/// and only to RE-encode for a `data:` URI).
pub fn parse_media_source(data: &DixData, prefix: &str) -> Result<MediaSource, String> {
    let embedded = opt::<String>(data, prefix, "data")?;
    let source_ref = opt::<String>(data, prefix, "source_ref")?;
    match (embedded, source_ref) {
        (Some(_), Some(_)) => Err(format!(
            "{}: specify exactly one of `data` (embedded base64) or `source_ref` (external file path), not both", prefix
        )),
        (None, None) => Err(format!(
            "{}: requires either `data` (embedded base64) or `source_ref` (external file path)", prefix
        )),
        (Some(b64), None) => {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            // `.trim()` — DixScript string literals can't contain a raw
            // newline (see this crate's own lexer), so a genuinely
            // multi-line-formatted base64 blob isn't possible here
            // either way, but trimming stray leading/trailing whitespace
            // from how someone pasted it in is free and harmless.
            let bytes = STANDARD.decode(b64.trim())
                .map_err(|e| format!("{}.data: invalid base64 ({})", prefix, e))?;
            Ok(MediaSource::Embedded(bytes))
        }
        (None, Some(path)) => Ok(MediaSource::FileRef(path)),
    }
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
