// src/compiler.rs
//! Scene → binary MSX payload.
//!
//! Encoding order (matches format-spec.md exactly):
//!   Header (32 bytes)
//!   Payload (optionally MBFA-compressed):
//!     Background RGBA   4 bytes
//!     Viewbox           16 bytes  (if flags.bit0)
//!     String pool       variable
//!     Def section       variable
//!     Element stream    variable

use std::io;
use crate::ast::*;
use crate::color::{LinearGradient, Paint, RadialGradient, Stop};
use crate::encoder::*;
use crate::header::{
    MsxHeader, COMPRESS_MBFA, COMPRESS_NONE,
    FLAG_HAS_DEFS, FLAG_HAS_VIEWBOX,
};
use crate::path::encode_commands;
use crate::primitives::Point;
use crate::style::Style;
use crate::transform::Transform;

// ── Element type tags (matches format-spec) ───────────────────────────────────

pub const TAG_RECT:             u8 = 0x00;
pub const TAG_CIRCLE:           u8 = 0x01;
pub const TAG_ELLIPSE:          u8 = 0x02;
pub const TAG_LINE:             u8 = 0x03;
pub const TAG_POLYLINE:         u8 = 0x04;
pub const TAG_POLYGON:          u8 = 0x05;
pub const TAG_PATH:             u8 = 0x06;
pub const TAG_TEXT:             u8 = 0x07;
pub const TAG_GROUP:            u8 = 0x08;
pub const TAG_USE:              u8 = 0x09;
pub const TAG_LINEAR_GRADIENT:  u8 = 0x0A;
pub const TAG_RADIAL_GRADIENT:  u8 = 0x0B;
pub const TAG_END:              u8 = 0xFF;

// ── Public API ────────────────────────────────────────────────────────────────

/// Compile a Scene to a self-contained binary MSX file.
///
/// When `compress = true` the payload is MBFA-compressed.
/// The 32-byte header is always written uncompressed so readers
/// can detect the format without decompressing first.
pub fn compile(scene: &Scene, compress: bool) -> io::Result<Vec<u8>> {
    let mut pool:    Vec<String> = Vec::new();
    let mut payload: Vec<u8>    = Vec::new();

    // ── Background ────────────────────────────────────────────────────────────
    payload.extend_from_slice(&scene.canvas.background.to_bytes());

    // ── Viewbox (if present) ──────────────────────────────────────────────────
    if let Some(ref vb) = scene.canvas.viewbox {
        payload.extend_from_slice(&vb.to_bytes());
    }

    // ── Temporarily reserve string pool slot — we'll overwrite it ─────────────
    // Pool position is after background + viewbox.  We write a placeholder
    // length field now and patch it after all elements are encoded.
    let pool_len_offset = payload.len();
    // Reserve 2 bytes for pool count; actual content appended after elements
    payload.push(0); payload.push(0);

    // ── Encode defs ───────────────────────────────────────────────────────────
    let mut def_payload: Vec<u8> = Vec::new();
    for def in &scene.defs {
        encode_def(def, &mut def_payload, &mut pool);
    }

    // ── Encode elements ───────────────────────────────────────────────────────
    let mut elem_payload: Vec<u8> = Vec::new();
    for elem in &scene.elements {
        encode_element(elem, &mut elem_payload, &mut pool);
    }
    write_u8(&mut elem_payload, TAG_END);

    // ── Now write the real string pool, patching the count ────────────────────
    // Overwrite the 2 placeholder bytes with actual count
    let count_bytes = (pool.len() as u16).to_le_bytes();
    payload[pool_len_offset]     = count_bytes[0];
    payload[pool_len_offset + 1] = count_bytes[1];

    // Append pool entries (len + bytes) — no count re-write needed
    for s in &pool {
        let bytes = s.as_bytes();
        write_u16(&mut payload, bytes.len() as u16);
        payload.extend_from_slice(bytes);
    }

    // ── Append defs + elements ────────────────────────────────────────────────
    payload.extend_from_slice(&def_payload);
    payload.extend_from_slice(&elem_payload);

    // ── Build header ──────────────────────────────────────────────────────────
    let mut header = MsxHeader::new(
        scene.canvas.width  as f32,
        scene.canvas.height as f32,
    );
    header.elem_count   = scene.elements.len() as u32;
    header.str_pool_len = pool.iter().map(|s| 2 + s.len()).sum::<usize>() as u32 + 2;
    header.def_count    = scene.defs.len() as u32;
    header.compress     = if compress { COMPRESS_MBFA } else { COMPRESS_NONE };

    if scene.canvas.viewbox.is_some() {
        header.set_viewbox(true);
    }
    if !scene.defs.is_empty() {
        header.set_defs(true);
    }

    // ── Optionally compress payload ───────────────────────────────────────────
    let final_payload = if compress {
        mbfa::compress(&payload, 8)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("MBFA compress: {}", e)))?
    } else {
        payload
    };

    // ── Assemble output ───────────────────────────────────────────────────────
    let mut out = Vec::with_capacity(32 + final_payload.len());
    out.extend_from_slice(&header.serialize());
    out.extend_from_slice(&final_payload);
    Ok(out)
}

// ── Def encoding ──────────────────────────────────────────────────────────────

fn encode_def(def: &Def, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    match def {
        Def::LinearGradient(g) => encode_linear_gradient(g, out, pool),
        Def::RadialGradient(g) => encode_radial_gradient(g, out, pool),
    }
}

fn encode_linear_gradient(g: &LinearGradient, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_LINEAR_GRADIENT);
    let id_idx = intern_string(pool, &g.id);
    write_u16(out, id_idx);
    write_f32(out, g.x1);
    write_f32(out, g.y1);
    write_f32(out, g.x2);
    write_f32(out, g.y2);
    write_u16(out, g.stops.len() as u16);
    for stop in &g.stops {
        out.extend_from_slice(&stop.to_bytes());
    }
}

fn encode_radial_gradient(g: &RadialGradient, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_RADIAL_GRADIENT);
    let id_idx = intern_string(pool, &g.id);
    write_u16(out, id_idx);
    write_f32(out, g.cx);
    write_f32(out, g.cy);
    write_f32(out, g.r);
    write_f32(out, g.fx);
    write_f32(out, g.fy);
    write_u16(out, g.stops.len() as u16);
    for stop in &g.stops {
        out.extend_from_slice(&stop.to_bytes());
    }
}

// ── Element encoding ──────────────────────────────────────────────────────────

fn encode_element(elem: &Element, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    match elem {
        Element::Rect(e)     => encode_rect(e, out, pool),
        Element::Circle(e)   => encode_circle(e, out, pool),
        Element::Ellipse(e)  => encode_ellipse(e, out, pool),
        Element::Line(e)     => encode_line(e, out, pool),
        Element::Polyline(e) => encode_polyline(e, out, pool),
        Element::Polygon(e)  => encode_polygon(e, out, pool),
        Element::Path(e)     => encode_path(e, out, pool),
        Element::Text(e)     => encode_text(e, out, pool),
        Element::Group(e)    => encode_group(e, out, pool),
        Element::Use(e)      => encode_use(e, out, pool),
    }
}

fn encode_rect(e: &Rect, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_RECT);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_f32(out, e.x);
    write_f32(out, e.y);
    write_f32(out, e.width);
    write_f32(out, e.height);
    write_f32(out, e.rx.unwrap_or(0.0));
    write_f32(out, e.ry.unwrap_or(e.rx.unwrap_or(0.0)));
    write_style(out, &e.style, pool);
}

fn encode_circle(e: &Circle, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_CIRCLE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_f32(out, e.cx);
    write_f32(out, e.cy);
    write_f32(out, e.r);
    write_style(out, &e.style, pool);
}

fn encode_ellipse(e: &Ellipse, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_ELLIPSE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_f32(out, e.cx);
    write_f32(out, e.cy);
    write_f32(out, e.rx);
    write_f32(out, e.ry);
    write_style(out, &e.style, pool);
}

fn encode_line(e: &Line, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_LINE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_f32(out, e.x1);
    write_f32(out, e.y1);
    write_f32(out, e.x2);
    write_f32(out, e.y2);
    write_style(out, &e.style, pool);
}

fn encode_polyline(e: &Polyline, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_POLYLINE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_u32(out, e.points.len() as u32);
    for p in &e.points {
        write_f32(out, p.x);
        write_f32(out, p.y);
    }
    write_style(out, &e.style, pool);
}

fn encode_polygon(e: &Polyline, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_POLYGON);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_u32(out, e.points.len() as u32);
    for p in &e.points {
        write_f32(out, p.x);
        write_f32(out, p.y);
    }
    write_style(out, &e.style, pool);
}

fn encode_path(e: &Path, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_PATH);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    let mut cmd_buf: Vec<u8> = Vec::new();
    encode_commands(&e.commands, &mut cmd_buf);
    write_u32(out, cmd_buf.len() as u32);
    out.extend_from_slice(&cmd_buf);
    write_style(out, &e.style, pool);
}

fn encode_text(e: &Text, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_TEXT);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    let str_idx = intern_string(pool, &e.content);
    write_u16(out, str_idx);
    write_f32(out, e.x);
    write_f32(out, e.y);
    write_style(out, &e.style, pool);
}

fn encode_group(e: &Group, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_GROUP);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_u32(out, e.children.len() as u32);
    for child in &e.children {
        encode_element(child, out, pool);
    }
    // Group style — write a style block; empty if None
    let empty = Style::empty();
    write_style(out, e.style.as_ref().unwrap_or(&empty), pool);
}

fn encode_use(e: &Use, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_USE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    let href_idx = intern_string(pool, &e.href);
    write_u16(out, href_idx);
    write_f32(out, e.x);
    write_f32(out, e.y);
}

// ── Stats helper ──────────────────────────────────────────────────────────────

/// Returns (uncompressed_payload_bytes, element_count, def_count)
pub fn compile_stats(scene: &Scene) -> (usize, usize, usize) {
    match compile(scene, false) {
        Ok(data) => (data.len() - 32, scene.element_count(), scene.defs.len()),
        Err(_)   => (0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::style::Style;
    use crate::ast::{Canvas, Circle, Rect, Scene};

    fn basic_scene() -> Scene {
        let mut style = Style::empty();
        style.fill         = Some(Paint::Color(Color::rgb(255, 0, 0)));
        style.stroke       = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity      = Some(1.0);

        let mut scene = Scene::new(Canvas::new(200.0, 200.0, Color::WHITE));
        scene.elements.push(Element::Circle(Circle {
            cx: 100.0, cy: 100.0, r: 50.0,
            id: None, transform: None, style,
        }));
        scene
    }

    #[test]
    fn compile_produces_valid_header_magic() {
        let data = compile(&basic_scene(), false).unwrap();
        assert_eq!(&data[0..4], b"MSX\0");
    }

    #[test]
    fn compile_header_dimensions() {
        let data = compile(&basic_scene(), false).unwrap();
        let w = f32::from_le_bytes(data[8..12].try_into().unwrap());
        let h = f32::from_le_bytes(data[12..16].try_into().unwrap());
        assert!((w - 200.0f32).abs() < 1e-4);
        assert!((h - 200.0f32).abs() < 1e-4);
    }

    #[test]
    fn compile_uncompressed_no_compress_flag() {
        let data = compile(&basic_scene(), false).unwrap();
        assert_eq!(data[5], crate::header::COMPRESS_NONE);
    }

    #[test]
    fn compile_compressed_sets_flag() {
        let data = compile(&basic_scene(), true).unwrap();
        assert_eq!(data[5], crate::header::COMPRESS_MBFA);
    }

    #[test]
    fn compile_elem_count_in_header() {
        let data = compile(&basic_scene(), false).unwrap();
        let count = u32::from_le_bytes(data[16..20].try_into().unwrap());
        assert_eq!(count, 1);
    }

    #[test]
    fn compile_multi_element_scene() {
        let mut scene = basic_scene();
        let mut style = Style::empty();
        style.fill = Some(Paint::Color(Color::rgb(0, 0, 255)));
        style.stroke = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity = Some(1.0);
        scene.elements.push(Element::Rect(Rect::new(
            10.0, 10.0, 50.0, 50.0, style
        )));
        let data = compile(&scene, false).unwrap();
        let count = u32::from_le_bytes(data[16..20].try_into().unwrap());
        assert_eq!(count, 2);
    }

    #[test]
    fn compile_size_less_than_raw_svg_for_many_circles() {
        // 100 circles — binary should beat a verbose SVG representation
        let mut scene = Scene::new(Canvas::new(1000.0, 1000.0, Color::WHITE));
        for i in 0..100 {
            let mut style = Style::empty();
            style.fill = Some(Paint::Color(Color::rgb((i * 2) as u8, 128, 200)));
            style.stroke = Some(Paint::None);
            style.stroke_width = Some(0.0);
            style.opacity = Some(1.0);
            scene.elements.push(Element::Circle(Circle {
                cx: (i * 10) as f64,
                cy: (i * 5) as f64,
                r: 20.0,
                id: None,
                transform: None,
                style,
            }));
        }
        let binary = compile(&scene, false).unwrap();
        // Each circle in SVG is ~90 bytes; 100 circles = ~9000 bytes + overhead
        // Binary: 32 header + ~30 bytes/circle = ~3032 bytes
        println!("100 circles binary size: {} bytes", binary.len());
        assert!(binary.len() < 9_000, "binary should be smaller than verbose SVG");
    }
  }
