// src/decoder.rs
//! Low-level binary read helpers.
//! All read_ functions advance a &mut usize cursor.

use std::io;
use crate::color::{Color, Paint};
use crate::style::{FillRule, FontWeight, LineCap, LineJoin, Style, TextAnchor};
use crate::transform::{Matrix2D, Transform,
    TRANSFORM_MATRIX, TRANSFORM_MULTIPLE, TRANSFORM_NONE, TRANSFORM_ROTATE,
    TRANSFORM_SCALE, TRANSFORM_SKEW_X, TRANSFORM_SKEW_Y, TRANSFORM_TRANSLATE};

// ── Error helper ──────────────────────────────────────────────────────────────

fn eof(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, msg.to_string())
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

// ── Primitives ────────────────────────────────────────────────────────────────

pub fn read_u8(data: &[u8], cursor: &mut usize) -> io::Result<u8> {
    if *cursor >= data.len() { return Err(eof("read_u8: unexpected eof")); }
    let v = data[*cursor];
    *cursor += 1;
    Ok(v)
}

pub fn read_u16(data: &[u8], cursor: &mut usize) -> io::Result<u16> {
    if *cursor + 2 > data.len() { return Err(eof("read_u16: unexpected eof")); }
    let v = u16::from_le_bytes(data[*cursor..*cursor + 2].try_into().unwrap());
    *cursor += 2;
    Ok(v)
}

pub fn read_u32(data: &[u8], cursor: &mut usize) -> io::Result<u32> {
    if *cursor + 4 > data.len() { return Err(eof("read_u32: unexpected eof")); }
    let v = u32::from_le_bytes(data[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    Ok(v)
}

pub fn read_f32(data: &[u8], cursor: &mut usize) -> io::Result<f64> {
    if *cursor + 4 > data.len() { return Err(eof("read_f32: unexpected eof")); }
    let v = f32::from_le_bytes(data[*cursor..*cursor + 4].try_into().unwrap()) as f64;
    *cursor += 4;
    Ok(v)
}

pub fn read_point(data: &[u8], cursor: &mut usize) -> io::Result<(f64, f64)> {
    let x = read_f32(data, cursor)?;
    let y = read_f32(data, cursor)?;
    Ok((x, y))
}

// ── String pool ───────────────────────────────────────────────────────────────

/// Deserialise the string pool from the payload.
/// Layout: `[u16 count] ([u16 byte_len][utf8 bytes])*`
pub fn read_string_pool(data: &[u8], cursor: &mut usize) -> io::Result<Vec<String>> {
    let count = read_u16(data, cursor)? as usize;
    let mut pool = Vec::with_capacity(count);
    for i in 0..count {
        let len = read_u16(data, cursor)? as usize;
        if *cursor + len > data.len() {
            return Err(eof(&format!("string pool entry {} truncated", i)));
        }
        let s = std::str::from_utf8(&data[*cursor..*cursor + len])
            .map_err(|e| bad(&format!("string pool entry {} invalid utf8: {}", i, e)))?
            .to_string();
        *cursor += len;
        pool.push(s);
    }
    Ok(pool)
}

pub fn lookup_string<'a>(pool: &'a [String], idx: u16) -> io::Result<&'a str> {
    pool.get(idx as usize)
        .map(|s| s.as_str())
        .ok_or_else(|| bad(&format!("string pool index {} out of range (pool len={})", idx, pool.len())))
}

// ── Color ─────────────────────────────────────────────────────────────────────

pub fn read_color(data: &[u8], cursor: &mut usize) -> io::Result<Color> {
    if *cursor + 4 > data.len() { return Err(eof("read_color: unexpected eof")); }
    let bytes: [u8; 4] = data[*cursor..*cursor + 4].try_into().unwrap();
    *cursor += 4;
    Ok(Color::from_bytes(bytes))
}

// ── Paint ─────────────────────────────────────────────────────────────────────

pub fn read_paint(data: &[u8], cursor: &mut usize, pool: &[String]) -> io::Result<Paint> {
    let tag = read_u8(data, cursor)?;
    match tag {
        t if t == Paint::TAG_NONE => Ok(Paint::None),
        t if t == Paint::TAG_COLOR => {
            let c = read_color(data, cursor)?;
            Ok(Paint::Color(c))
        }
        t if t == Paint::TAG_GRADIENT_REF || t == Paint::TAG_PATTERN_REF => {
            let idx = read_u16(data, cursor)?;
            let s   = lookup_string(pool, idx)?.to_string();
            if s == "currentColor" {
                Ok(Paint::CurrentColor)
            } else {
                Ok(Paint::Ref(s))
            }
        }
        other => Err(bad(&format!("unknown paint tag 0x{:02x}", other))),
    }
}

// ── Transform ─────────────────────────────────────────────────────────────────

pub fn read_transform(data: &[u8], cursor: &mut usize) -> io::Result<Transform> {
    let tag = read_u8(data, cursor)?;
    match tag {
        TRANSFORM_NONE => Ok(Transform::None),
        TRANSFORM_MATRIX => {
            if *cursor + 24 > data.len() { return Err(eof("matrix truncated")); }
            let bytes: [u8; 24] = data[*cursor..*cursor + 24].try_into().unwrap();
            *cursor += 24;
            Ok(Transform::Matrix(Matrix2D::from_bytes(&bytes)))
        }
        TRANSFORM_TRANSLATE => {
            let x = read_f32(data, cursor)?;
            let y = read_f32(data, cursor)?;
            Ok(Transform::Translate { x, y })
        }
        TRANSFORM_SCALE => {
            let x = read_f32(data, cursor)?;
            let y = read_f32(data, cursor)?;
            Ok(Transform::Scale { x, y })
        }
        TRANSFORM_ROTATE => {
            let angle      = read_f32(data, cursor)?;
            let has_center = read_u8(data, cursor)? != 0;
            let (cx, cy) = if has_center {
                let cx = read_f32(data, cursor)?;
                let cy = read_f32(data, cursor)?;
                (Some(cx), Some(cy))
            } else {
                (None, None)
            };
            Ok(Transform::Rotate { angle, cx, cy })
        }
        TRANSFORM_SKEW_X => {
            let a = read_f32(data, cursor)?;
            Ok(Transform::SkewX(a))
        }
        TRANSFORM_SKEW_Y => {
            let a = read_f32(data, cursor)?;
            Ok(Transform::SkewY(a))
        }
        TRANSFORM_MULTIPLE => {
            let count = read_u8(data, cursor)? as usize;
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(read_transform(data, cursor)?);
            }
            Ok(Transform::Multiple(v))
        }
        other => Err(bad(&format!("unknown transform tag 0x{:02x}", other))),
    }
}

pub fn read_optional_transform(data: &[u8], cursor: &mut usize) -> io::Result<Option<Transform>> {
    let t = read_transform(data, cursor)?;
    Ok(if t.is_none() { None } else { Some(t) })
}

// ── id_flags ──────────────────────────────────────────────────────────────────

pub fn read_id_flags(
    data:   &[u8],
    cursor: &mut usize,
    pool:   &[String],
) -> io::Result<(Option<String>, Option<Transform>)> {
    let flags        = read_u8(data, cursor)?;
    let has_id       = flags & 1 != 0;
    let has_transform = flags & 2 != 0;

    let id = if has_id {
        let idx = read_u16(data, cursor)?;
        Some(lookup_string(pool, idx)?.to_string())
    } else {
        None
    };

    let transform = if has_transform {
        Some(read_transform(data, cursor)?)
    } else {
        None
    };

    Ok((id, transform))
}

// ── Style ─────────────────────────────────────────────────────────────────────

pub fn read_style(data: &[u8], cursor: &mut usize, pool: &[String]) -> io::Result<Style> {
    let flags = read_u8(data, cursor)?;
    let mut s = Style::empty();

    if flags & (1 << 0) != 0 {
        s.fill = Some(read_paint(data, cursor, pool)?);
    }
    if flags & (1 << 1) != 0 {
        s.stroke = Some(read_paint(data, cursor, pool)?);
    }
    if flags & (1 << 2) != 0 {
        s.opacity = Some(read_f32(data, cursor)?);
    }
    if flags & (1 << 3) != 0 {
        s.stroke_width = Some(read_f32(data, cursor)?);
    }
    if flags & (1 << 4) != 0 {
        s.fill_rule          = Some(FillRule::from_byte(read_u8(data, cursor)?));
        s.stroke_linecap     = Some(LineCap::from_byte(read_u8(data, cursor)?));
        s.stroke_linejoin    = Some(LineJoin::from_byte(read_u8(data, cursor)?));
        s.stroke_miterlimit  = Some(read_f32(data, cursor)?);
    }
    if flags & (1 << 5) != 0 {
        let fs_x100       = read_u16(data, cursor)?;
        s.font_size       = Some(fs_x100 as f64 / 100.0);
        let ff_idx        = read_u16(data, cursor)?;
        let ff            = lookup_string(pool, ff_idx)?.to_string();
        s.font_family     = if ff.is_empty() { None } else { Some(ff) };
        s.font_weight     = Some(FontWeight::from_byte(read_u8(data, cursor)?));
        s.text_anchor     = Some(TextAnchor::from_byte(read_u8(data, cursor)?));
    }
    if flags & (1 << 6) != 0 {
        let count         = read_u16(data, cursor)? as usize;
        let mut da        = Vec::with_capacity(count);
        for _ in 0..count {
            da.push(read_f32(data, cursor)?);
        }
        s.stroke_dasharray  = if da.is_empty() { None } else { Some(da) };
        s.stroke_dashoffset = Some(read_f32(data, cursor)?);
    }
    if flags & (1 << 7) != 0 {
        let vd              = read_u8(data, cursor)?;
        s.visibility_hidden = vd & 1 != 0;
        s.display_none      = vd & 2 != 0;
    }

    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::{
        write_color, write_paint, write_style, write_string_pool,
        write_transform, intern_string,
    };
    use crate::color::Color;
    use crate::style::Style;
    use crate::transform::Transform;

    fn roundtrip_paint(paint: Paint) {
        let mut buf  = Vec::new();
        let mut pool = Vec::new();
        write_paint(&mut buf, &paint, &mut pool);
        let mut cursor = 0;
        let back = read_paint(&buf, &mut cursor, &pool).unwrap();
        assert_eq!(back, paint);
        assert_eq!(cursor, buf.len());
    }

    #[test]
    fn paint_none_roundtrip() {
        roundtrip_paint(Paint::None);
    }

    #[test]
    fn paint_color_roundtrip() {
        roundtrip_paint(Paint::Color(Color::rgba(10, 20, 30, 200)));
    }

    #[test]
    fn paint_ref_roundtrip() {
        roundtrip_paint(Paint::Ref("url(#sunset)".to_string()));
    }

    #[test]
    fn paint_current_color_roundtrip() {
        roundtrip_paint(Paint::CurrentColor);
    }

    #[test]
    fn transform_translate_roundtrip() {
        let t = Transform::Translate { x: 10.5, y: -20.0 };
        let mut buf = Vec::new();
        write_transform(&mut buf, &t);
        let mut cursor = 0;
        let back = read_transform(&buf, &mut cursor).unwrap();
        assert_eq!(cursor, buf.len());
        if let Transform::Translate { x, y } = back {
            assert!((x - 10.5).abs() < 1e-4);
            assert!((y + 20.0).abs() < 1e-4);
        } else {
            panic!("expected Translate");
        }
    }

    #[test]
    fn transform_rotate_with_center_roundtrip() {
        let t = Transform::Rotate { angle: 45.0, cx: Some(100.0), cy: Some(200.0) };
        let mut buf = Vec::new();
        write_transform(&mut buf, &t);
        let mut cursor = 0;
        let back = read_transform(&buf, &mut cursor).unwrap();
        if let Transform::Rotate { angle, cx, cy } = back {
            assert!((angle - 45.0).abs() < 1e-4);
            assert!((cx.unwrap() - 100.0).abs() < 1e-4);
            assert!((cy.unwrap() - 200.0).abs() < 1e-4);
        } else {
            panic!("expected Rotate");
        }
    }

    #[test]
    fn style_roundtrip() {
        let mut style = Style::empty();
        style.fill         = Some(Paint::Color(Color::rgb(255, 128, 0)));
        style.stroke       = Some(Paint::None);
        style.stroke_width = Some(2.5);
        style.opacity      = Some(0.8);

        let mut buf  = Vec::new();
        let mut pool = Vec::new();
        write_style(&mut buf, &style, &mut pool);

        let mut cursor = 0;
        let back = read_style(&buf, &mut cursor, &pool).unwrap();
        assert_eq!(cursor, buf.len());

        assert_eq!(back.fill, style.fill);
        assert_eq!(back.stroke, style.stroke);
        assert!((back.stroke_width.unwrap() - 2.5).abs() < 1e-3);
        assert!((back.opacity.unwrap() - 0.8).abs() < 1e-3);
    }

    #[test]
    fn string_pool_roundtrip() {
        let mut pool = Vec::new();
        intern_string(&mut pool, "hello");
        intern_string(&mut pool, "world");
        let mut buf = Vec::new();
        write_string_pool(&mut buf, &pool);
        let mut cursor = 0;
        let back = read_string_pool(&buf, &mut cursor).unwrap();
        assert_eq!(back, pool);
        assert_eq!(cursor, buf.len());
    }

    #[test]
    fn style_font_fields_roundtrip() {
        let mut style = Style::empty();
        style.font_size   = Some(14.0);
        style.font_family = Some("sans-serif".to_string());
        style.font_weight = Some(crate::style::FontWeight::Bold);
        style.text_anchor = Some(crate::style::TextAnchor::Middle);

        let mut buf  = Vec::new();
        let mut pool = Vec::new();
        write_style(&mut buf, &style, &mut pool);
        let mut cursor = 0;
        let back = read_style(&buf, &mut cursor, &pool).unwrap();

        assert!((back.font_size.unwrap() - 14.0).abs() < 0.01);
        assert_eq!(back.font_family.as_deref(), Some("sans-serif"));
        assert_eq!(back.text_anchor, Some(crate::style::TextAnchor::Middle));
    }
}
