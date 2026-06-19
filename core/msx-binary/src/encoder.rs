// core/msx-binary/src/encoder.rs
//! Low-level binary write helpers. All write_ functions append to a &mut Vec<u8>.

use msx_ast::{Color, FillRule, LineCap, LineJoin, Paint, Style, Transform};

// ── Primitives ───────────────────────────────────────────────────────────────

#[inline] pub fn write_u8(out: &mut Vec<u8>, v: u8)   { out.push(v); }
#[inline] pub fn write_u16(out: &mut Vec<u8>, v: u16) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub fn write_u32(out: &mut Vec<u8>, v: u32) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub fn write_f32(out: &mut Vec<u8>, v: f64) { out.extend_from_slice(&(v as f32).to_le_bytes()); }

#[inline]
pub fn write_point(out: &mut Vec<u8>, x: f64, y: f64) {
    write_f32(out, x);
    write_f32(out, y);
}

// ── String pool ───────────────────────────────────────────────────────────────

pub fn intern_string(pool: &mut Vec<String>, s: &str) -> u16 {
    if let Some(idx) = pool.iter().position(|p| p == s) {
        return idx as u16;
    }
    let idx = pool.len() as u16;
    pool.push(s.to_string());
    idx
}

pub fn write_string_pool(out: &mut Vec<u8>, pool: &[String]) {
    write_u16(out, pool.len() as u16);
    for s in pool {
        let bytes = s.as_bytes();
        write_u16(out, bytes.len() as u16);
        out.extend_from_slice(bytes);
    }
}

// ── Color / Paint ────────────────────────────────────────────────────────────

pub fn write_color(out: &mut Vec<u8>, c: Color) {
    out.extend_from_slice(&c.to_bytes());
}

pub fn write_paint(out: &mut Vec<u8>, paint: &Paint, pool: &mut Vec<String>) {
    match paint {
        Paint::None => {
            write_u8(out, Paint::TAG_NONE);
        }
        Paint::Color(c) => {
            write_u8(out, Paint::TAG_COLOR);
            write_color(out, *c);
        }
        Paint::Ref(r) => {
            write_u8(out, Paint::TAG_GRADIENT_REF);
            let idx = intern_string(pool, r);
            write_u16(out, idx);
        }
        Paint::CurrentColor => {
            write_u8(out, Paint::TAG_GRADIENT_REF);
            let idx = intern_string(pool, "currentColor");
            write_u16(out, idx);
        }
    }
}

// ── Transform ────────────────────────────────────────────────────────────────

use msx_ast::transform::{
    TRANSFORM_MATRIX, TRANSFORM_MULTIPLE, TRANSFORM_NONE, TRANSFORM_ROTATE,
    TRANSFORM_SCALE, TRANSFORM_SKEW_X, TRANSFORM_SKEW_Y, TRANSFORM_TRANSLATE,
};

pub fn write_transform(out: &mut Vec<u8>, t: &Transform) {
    match t {
        Transform::None => {
            write_u8(out, TRANSFORM_NONE);
        }
        Transform::Matrix(m) => {
            write_u8(out, TRANSFORM_MATRIX);
            out.extend_from_slice(&m.to_bytes());
        }
        Transform::Translate { x, y } => {
            write_u8(out, TRANSFORM_TRANSLATE);
            write_f32(out, *x);
            write_f32(out, *y);
        }
        Transform::Scale { x, y } => {
            write_u8(out, TRANSFORM_SCALE);
            write_f32(out, *x);
            write_f32(out, *y);
        }
        Transform::Rotate { angle, cx, cy } => {
            write_u8(out, TRANSFORM_ROTATE);
            write_f32(out, *angle);
            let has_center = cx.is_some() && cy.is_some();
            write_u8(out, has_center as u8);
            if has_center {
                write_f32(out, cx.unwrap());
                write_f32(out, cy.unwrap());
            }
        }
        Transform::SkewX(a) => {
            write_u8(out, TRANSFORM_SKEW_X);
            write_f32(out, *a);
        }
        Transform::SkewY(a) => {
            write_u8(out, TRANSFORM_SKEW_Y);
            write_f32(out, *a);
        }
        Transform::Multiple(v) => {
            write_u8(out, TRANSFORM_MULTIPLE);
            write_u8(out, v.len().min(255) as u8);
            for sub in v.iter().take(255) {
                write_transform(out, sub);
            }
        }
    }
}

pub fn write_optional_transform(out: &mut Vec<u8>, t: &Option<Transform>) {
    match t {
        None    => write_u8(out, TRANSFORM_NONE),
        Some(t) => write_transform(out, t),
    }
}

// ── id_flags helper ──────────────────────────────────────────────────────────

pub fn write_id_flags(
    out:       &mut Vec<u8>,
    id:        Option<&str>,
    transform: Option<&Transform>,
    pool:      &mut Vec<String>,
) {
    let has_id        = id.is_some();
    let has_transform = transform.is_some();
    let flags         = (has_id as u8) | ((has_transform as u8) << 1);
    write_u8(out, flags);

    if let Some(id_str) = id {
        let idx = intern_string(pool, id_str);
        write_u16(out, idx);
    }
    if let Some(t) = transform {
        write_transform(out, t);
    }
}

// ── Style ────────────────────────────────────────────────────────────────────

pub fn write_style(out: &mut Vec<u8>, style: &Style, pool: &mut Vec<String>) {
    let flags = style.present_flags();
    write_u8(out, flags);

    if flags & (1 << 0) != 0 {
        write_paint(out, style.fill.as_ref().unwrap_or(&Paint::None), pool);
    }
    if flags & (1 << 1) != 0 {
        write_paint(out, style.stroke.as_ref().unwrap_or(&Paint::None), pool);
    }
    if flags & (1 << 2) != 0 {
        write_f32(out, style.opacity.unwrap_or(1.0));
    }
    if flags & (1 << 3) != 0 {
        write_f32(out, style.stroke_width.unwrap_or(1.0));
    }
    if flags & (1 << 4) != 0 {
        write_u8(out, style.fill_rule.unwrap_or(FillRule::NonZero).to_byte());
        write_u8(out, style.stroke_linecap.unwrap_or(LineCap::Butt).to_byte());
        write_u8(out, style.stroke_linejoin.unwrap_or(LineJoin::Miter).to_byte());
        write_f32(out, style.stroke_miterlimit.unwrap_or(4.0));
    }
    if flags & (1 << 5) != 0 {
        let fs = (style.font_size.unwrap_or(12.0) * 100.0).round() as u16;
        write_u16(out, fs);

        let ff_idx = intern_string(pool, style.font_family.as_deref().unwrap_or(""));
        write_u16(out, ff_idx);

        let fw_byte = style.font_weight
            .as_ref()
            .map(|fw| fw.to_byte().saturating_add(1))
            .unwrap_or(0);
        write_u8(out, fw_byte);

        let ta_byte = style.text_anchor
            .map(|ta| ta.to_byte() + 1)
            .unwrap_or(0);
        write_u8(out, ta_byte);
    }
    if flags & (1 << 6) != 0 {
        let da = style.stroke_dasharray.as_deref().unwrap_or(&[]);
        write_u16(out, da.len() as u16);
        for &d in da {
            write_f32(out, d);
        }
        write_f32(out, style.stroke_dashoffset.unwrap_or(0.0));
    }
    if flags & (1 << 7) != 0 {
        let vd = (style.visibility_hidden as u8) | ((style.display_none as u8) << 1);
        write_u8(out, vd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_u16_le() {
        let mut buf = Vec::new();
        write_u16(&mut buf, 0x0102);
        assert_eq!(buf, [0x02, 0x01]);
    }

    #[test]
    fn intern_string_deduplicates() {
        let mut pool = Vec::new();
        let i1 = intern_string(&mut pool, "hello");
        let i2 = intern_string(&mut pool, "hello");
        let i3 = intern_string(&mut pool, "world");
        assert_eq!(i1, i2);
        assert_ne!(i1, i3);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn write_paint_none_is_single_byte() {
        let mut buf  = Vec::new();
        let mut pool = Vec::new();
        write_paint(&mut buf, &Paint::None, &mut pool);
        assert_eq!(buf, [Paint::TAG_NONE]);
    }

    #[test]
    fn write_paint_color() {
        let mut buf  = Vec::new();
        let mut pool = Vec::new();
        write_paint(&mut buf, &Paint::Color(Color::rgb(255, 0, 128)), &mut pool);
        assert_eq!(buf[0], Paint::TAG_COLOR);
        assert_eq!(buf[1], 255);
        assert_eq!(buf[2], 0);
        assert_eq!(buf[3], 128);
        assert_eq!(buf[4], 255);
    }
                          }
