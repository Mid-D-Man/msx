// core/msx-binary/src/compiler.rs
//! Scene → binary MSX payload.
//!
//! Encoding order (matches docs/format-spec.md):
//!   Header (32 bytes)
//!   Payload (optionally MBFA-compressed):
//!     Background RGBA   4 bytes
//!     Viewbox           16 bytes  (if flags.bit0)
//!     String pool       variable
//!     Def section       variable
//!     Element stream    variable

use std::io;

use msx_ast::{
    Circle, Def, Element, Ellipse, GaussianSplat, Group, Layer, Line, Path, Polyline, Rect,
    Scene, SdfNode, ShaderUniformValue, Style, Text, Use,
};

use crate::encoder::*;
use crate::effect_codec::write_effect;
use crate::header::{MsxHeader, COMPRESS_MBFA, COMPRESS_NONE};
use crate::path_codec::encode_commands;
use crate::sdf_codec::write_sdf_tree;
use crate::tags::*;

// ── Public API ───────────────────────────────────────────────────────────────

pub fn compile(scene: &Scene, compress: bool) -> io::Result<Vec<u8>> {
    let mut pool:    Vec<String> = Vec::new();
    let mut payload: Vec<u8>     = Vec::new();

    // Background
    payload.extend_from_slice(&scene.canvas.background.to_bytes());

    // Viewbox
    if let Some(ref vb) = scene.canvas.viewbox {
        payload.extend_from_slice(&vb.to_bytes());
    }

    // Placeholder string-pool count (2 bytes, patched below)
    let pool_len_offset = payload.len();
    payload.push(0);
    payload.push(0);

    // Defs
    let mut def_payload: Vec<u8> = Vec::new();
    for def in &scene.defs {
        encode_def(def, &mut def_payload, &mut pool);
    }

    // Elements
    let mut elem_payload: Vec<u8> = Vec::new();
    for elem in &scene.elements {
        encode_element(elem, &mut elem_payload, &mut pool);
    }
    write_u8(&mut elem_payload, TAG_END);

    // Patch pool count
    let count_bytes = (pool.len() as u16).to_le_bytes();
    payload[pool_len_offset]     = count_bytes[0];
    payload[pool_len_offset + 1] = count_bytes[1];

    // Append pool entries
    for s in &pool {
        let bytes = s.as_bytes();
        write_u16(&mut payload, bytes.len() as u16);
        payload.extend_from_slice(bytes);
    }

    // Append defs + elements
    payload.extend_from_slice(&def_payload);
    payload.extend_from_slice(&elem_payload);

    // Header
    let mut header = MsxHeader::new(scene.canvas.width as f32, scene.canvas.height as f32);
    header.elem_count   = scene.elements.len() as u32;
    header.str_pool_len = pool.iter().map(|s| 2 + s.len()).sum::<usize>() as u32 + 2;
    header.def_count    = scene.defs.len() as u32;
    header.compress     = if compress { COMPRESS_MBFA } else { COMPRESS_NONE };

    if scene.canvas.viewbox.is_some() { header.set_viewbox(true); }
    if !scene.defs.is_empty()         { header.set_defs(true); }

    // Optionally compress
    let final_payload = if compress {
        mbfa::compress(&payload, 8)
            .map_err(|e| io::Error::other(format!("MBFA compress: {}", e)))?
    } else {
        payload
    };

    let mut out = Vec::with_capacity(32 + final_payload.len());
    out.extend_from_slice(&header.serialize());
    out.extend_from_slice(&final_payload);
    Ok(out)
}

// ── Def encoding ─────────────────────────────────────────────────────────────

fn encode_def(def: &Def, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    match def {
        Def::LinearGradient(g) => {
            write_u8(out, TAG_LINEAR_GRADIENT);
            let id_idx = intern_string(pool, &g.id);
            write_u16(out, id_idx);
            write_f32(out, g.x1); write_f32(out, g.y1);
            write_f32(out, g.x2); write_f32(out, g.y2);
            write_u16(out, g.stops.len() as u16);
            for stop in &g.stops { out.extend_from_slice(&stop.to_bytes()); }
        }
        Def::RadialGradient(g) => {
            write_u8(out, TAG_RADIAL_GRADIENT);
            let id_idx = intern_string(pool, &g.id);
            write_u16(out, id_idx);
            write_f32(out, g.cx); write_f32(out, g.cy); write_f32(out, g.r);
            write_f32(out, g.fx); write_f32(out, g.fy);
            write_u16(out, g.stops.len() as u16);
            for stop in &g.stops { out.extend_from_slice(&stop.to_bytes()); }
        }
        Def::ConicGradient(g) => {
            write_u8(out, TAG_CONIC_GRADIENT);
            let id_idx = intern_string(pool, &g.id);
            write_u16(out, id_idx);
            write_f32(out, g.cx); write_f32(out, g.cy); write_f32(out, g.angle);
            write_u16(out, g.stops.len() as u16);
            for stop in &g.stops { out.extend_from_slice(&stop.to_bytes()); }
        }
        Def::Shader(s) => {
            write_u8(out, TAG_SHADER);
            let id_idx          = intern_string(pool, &s.id);
            let source_ref_idx  = intern_string(pool, &s.source_ref);
            let entry_point_idx = intern_string(pool, &s.entry_point);
            write_u16(out, id_idx);
            write_u16(out, source_ref_idx);
            write_u16(out, entry_point_idx);
            write_color(out, s.fallback_color);
            write_u16(out, s.uniforms.len() as u16);
            for u in &s.uniforms {
                let name_idx = intern_string(pool, &u.name);
                write_u16(out, name_idx);
                match u.value {
                    ShaderUniformValue::Float(x) => {
                        write_u8(out, 0);
                        write_f32(out, x as f64);
                    }
                    ShaderUniformValue::Vec2(x, y) => {
                        write_u8(out, 1);
                        write_f32(out, x as f64); write_f32(out, y as f64);
                    }
                    ShaderUniformValue::Vec3(x, y, z) => {
                        write_u8(out, 2);
                        write_f32(out, x as f64); write_f32(out, y as f64); write_f32(out, z as f64);
                    }
                    ShaderUniformValue::Vec4(x, y, z, w) => {
                        write_u8(out, 3);
                        write_f32(out, x as f64); write_f32(out, y as f64);
                        write_f32(out, z as f64); write_f32(out, w as f64);
                    }
                }
            }
        }
    }
}

// ── Element encoding ─────────────────────────────────────────────────────────

fn encode_element(elem: &Element, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    match elem {
        Element::Rect(e)     => encode_rect(e, out, pool),
        Element::Circle(e)   => encode_circle(e, out, pool),
        Element::Ellipse(e)  => encode_ellipse(e, out, pool),
        Element::Line(e)     => encode_line(e, out, pool),
        Element::Polyline(e) => encode_polyline(e, out, pool, TAG_POLYLINE),
        Element::Polygon(e)  => encode_polyline(e, out, pool, TAG_POLYGON),
        Element::Path(e)     => encode_path(e, out, pool),
        Element::Text(e)     => encode_text(e, out, pool),
        Element::Group(e)    => encode_group(e, out, pool),
        Element::Use(e)      => encode_use(e, out, pool),
        Element::Sdf(e)      => encode_sdf(e, out, pool),
        Element::Splat(e)    => encode_splat(e, out, pool),
        Element::Layer(e)    => encode_layer(e, out, pool),
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
    // Always encode 0.0 when ry is None — never falls back to rx. Keeps
    // None → wire 0.0 → decode None a true roundtrip rather than aliasing
    // ry onto rx's value.
    write_f32(out, e.ry.unwrap_or(0.0));
    write_style(out, &e.style, pool);
}

fn encode_circle(e: &Circle, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_CIRCLE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_f32(out, e.cx); write_f32(out, e.cy); write_f32(out, e.r);
    write_style(out, &e.style, pool);
}

fn encode_ellipse(e: &Ellipse, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_ELLIPSE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_f32(out, e.cx); write_f32(out, e.cy);
    write_f32(out, e.rx); write_f32(out, e.ry);
    write_style(out, &e.style, pool);
}

fn encode_line(e: &Line, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_LINE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_f32(out, e.x1); write_f32(out, e.y1);
    write_f32(out, e.x2); write_f32(out, e.y2);
    write_style(out, &e.style, pool);
}

fn encode_polyline(e: &Polyline, out: &mut Vec<u8>, pool: &mut Vec<String>, tag: u8) {
    write_u8(out, tag);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_u32(out, e.points.len() as u32);
    for p in &e.points { write_f32(out, p.x); write_f32(out, p.y); }
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
    write_f32(out, e.x); write_f32(out, e.y);
    write_style(out, &e.style, pool);
}

fn encode_group(e: &Group, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_GROUP);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_u32(out, e.children.len() as u32);
    for child in &e.children { encode_element(child, out, pool); }
    let empty = Style::empty();
    write_style(out, e.style.as_ref().unwrap_or(&empty), pool);
}

fn encode_use(e: &Use, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_USE);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    let href_idx = intern_string(pool, &e.href);
    write_u16(out, href_idx);
    write_f32(out, e.x); write_f32(out, e.y);
}

fn encode_sdf(e: &SdfNode, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_SDF);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_sdf_tree(out, &e.tree);
    write_paint(out, &e.fill, pool);
    let has_stroke = e.stroke.is_some();
    write_u8(out, has_stroke as u8);
    if has_stroke {
        write_paint(out, e.stroke.as_ref().unwrap(), pool);
        write_f32(out, e.stroke_width.unwrap_or(1.0));
    }
}

fn encode_splat(e: &GaussianSplat, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_SPLAT);
    // GaussianSplat has no transform field, so it uses a simpler id-only
    // header rather than the shared write_id_flags two-bit scheme.
    let has_id = e.id.is_some();
    write_u8(out, has_id as u8);
    if let Some(id) = &e.id {
        let idx = intern_string(pool, id);
        write_u16(out, idx);
    }
    write_f32(out, e.x);
    write_f32(out, e.y);
    write_f32(out, e.sigma_x);
    write_f32(out, e.sigma_y);
    write_f32(out, e.rotation);
    write_color(out, e.color);
    write_f32(out, e.opacity);
}

fn encode_layer(e: &Layer, out: &mut Vec<u8>, pool: &mut Vec<String>) {
    write_u8(out, TAG_LAYER);
    write_id_flags(out, e.id.as_deref(), e.transform.as_ref(), pool);
    write_u8(out, e.blend_mode.to_byte());
    write_f32(out, e.opacity);
    write_u8(out, e.clip as u8);
    write_u16(out, e.effects.len() as u16);
    for fx in &e.effects { write_effect(out, fx); }
    write_u32(out, e.children.len() as u32);
    for child in &e.children { encode_element(child, out, pool); }
}

// ── Stats ──────────────────────────────────────────────────────────────────────

pub fn compile_stats(scene: &Scene) -> (usize, usize, usize) {
    match compile(scene, false) {
        Ok(data) => (data.len() - 32, scene.element_count(), scene.defs.len()),
        Err(_)   => (0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Canvas, Color, Paint};

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
    fn rect_ry_none_does_not_inherit_rx() {
        use msx_ast::Rect as AstRect;
        let mut style = Style::empty();
        style.fill         = Some(Paint::Color(Color::rgb(0, 128, 255)));
        style.stroke       = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity      = Some(1.0);

        let mut scene = Scene::new(Canvas::new(400.0, 300.0, Color::WHITE));
        scene.elements.push(Element::Rect(AstRect {
            x: 10.0, y: 10.0, width: 100.0, height: 50.0,
            rx: Some(12.0),
            ry: None, // explicitly None
            id: None, transform: None, style,
        }));

        let binary  = compile(&scene, false).unwrap();
        let decoded = crate::decode(&binary).unwrap();

        if let Element::Rect(r) = &decoded.elements[0] {
            assert_eq!(r.rx, Some(12.0), "rx must survive roundtrip");
            assert_eq!(r.ry, None,       "ry=None must NOT become Some(12) after roundtrip");
        } else {
            panic!("expected Rect");
        }
    }

    #[test]
    fn sdf_splat_layer_roundtrip() {
        use msx_ast::{BlendMode, Effect, SdfTree};

        let mut scene = Scene::new(Canvas::new(500.0, 500.0, Color::WHITE));

        let sdf = SdfNode::new(
            SdfTree::Circle { cx: 0.0, cy: 0.0, r: 50.0 }
                .smooth_subtract(SdfTree::Circle { cx: 20.0, cy: 0.0, r: 20.0 }, 0.25),
            Paint::Color(Color::rgb(200, 50, 50)),
        );
        scene.elements.push(Element::Sdf(sdf));

        scene.elements.push(Element::Splat(GaussianSplat::circle(
            250.0, 250.0, 40.0, Color::rgb(255, 255, 200), 0.5,
        )));

        let layer = Layer::new(vec![])
            .with_blend(BlendMode::Multiply)
            .with_opacity(0.75)
            .with_effect(Effect::Blur { radius: 8.0 });
        scene.elements.push(Element::Layer(layer));

        let binary  = compile(&scene, true).unwrap();
        let decoded = crate::decode(&binary).unwrap();

        assert_eq!(decoded.elements.len(), 3);
        assert!(matches!(decoded.elements[0], Element::Sdf(_)));
        assert!(matches!(decoded.elements[1], Element::Splat(_)));
        if let Element::Layer(l) = &decoded.elements[2] {
            assert_eq!(l.blend_mode, BlendMode::Multiply);
            assert!((l.opacity - 0.75).abs() < 1e-3);
            assert_eq!(l.effects.len(), 1);
        } else {
            panic!("expected Layer");
        }
    }
    }
