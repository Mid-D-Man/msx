// core/msx-binary/src/scene_decode.rs
//! Binary → Scene decoding. The complement of `compiler::compile`.

use std::io;

use msx_ast::{
    Canvas, Circle, ConicGradient, Def, Element, Ellipse, GaussianSplat, Group, Layer, Line,
    LinearGradient, Path, Point, Polyline, RadialGradient, Rect, SdfNode, Scene, ShaderDef,
    ShaderUniform, ShaderUniformValue, Stop, Text, Use, ViewBox,
};

use crate::decoder::*;
use crate::effect_codec::read_effect;
use crate::header::{MsxHeader, COMPRESS_MBFA, HEADER_SIZE};
use crate::path_codec::decode_commands;
use crate::sdf_codec::read_sdf_tree;
use crate::tags::*;

/// Decode a binary MSX file (header + optionally MBFA-compressed payload)
/// back into a Scene.
pub fn decode(data: &[u8]) -> io::Result<Scene> {
    let header = MsxHeader::parse(data)?;

    // Decompress payload if needed.
    let payload_raw = &data[HEADER_SIZE..];
    let payload_owned: Vec<u8>;
    let payload: &[u8] = if header.compress == COMPRESS_MBFA {
        payload_owned = mbfa::decompress(payload_raw).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("MBFA decompress: {}", e))
        })?;
        &payload_owned
    } else {
        payload_raw
    };

    let mut cursor = 0;

    // Background
    let bg = read_color(payload, &mut cursor)?;

    // Viewbox
    let viewbox = if header.has_viewbox() {
        if cursor + 16 > payload.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "viewbox truncated"));
        }
        let bytes: [u8; 16] = payload[cursor..cursor + 16].try_into().unwrap();
        cursor += 16;
        Some(ViewBox::from_bytes(&bytes))
    } else {
        None
    };

    // String pool
    let pool = read_string_pool(payload, &mut cursor)?;

    // Defs
    let mut defs: Vec<Def> = Vec::with_capacity(header.def_count as usize);
    for _ in 0..header.def_count {
        defs.push(decode_def(payload, &mut cursor, &pool)?);
    }

    // Elements
    let mut elements: Vec<Element> = Vec::with_capacity(header.elem_count as usize);
    for _ in 0..header.elem_count {
        elements.push(decode_element(payload, &mut cursor, &pool)?);
    }

    let mut canvas = Canvas::new(header.width as f64, header.height as f64, bg);
    canvas.viewbox = viewbox;

    let mut scene = Scene::new(canvas);
    scene.defs     = defs;
    scene.elements = elements;
    Ok(scene)
}

// ── Def decoding ─────────────────────────────────────────────────────────────

fn decode_def(data: &[u8], cursor: &mut usize, pool: &[String]) -> io::Result<Def> {
    let tag = read_u8(data, cursor)?;
    match tag {
        TAG_LINEAR_GRADIENT => {
            let id_idx = read_u16(data, cursor)?;
            let id     = pool.get(id_idx as usize).cloned().unwrap_or_default();
            let x1 = read_f32(data, cursor)?;
            let y1 = read_f32(data, cursor)?;
            let x2 = read_f32(data, cursor)?;
            let y2 = read_f32(data, cursor)?;
            let stops = read_stops(data, cursor)?;
            Ok(Def::LinearGradient(LinearGradient::new(id, x1, y1, x2, y2, stops)))
        }
        TAG_RADIAL_GRADIENT => {
            let id_idx = read_u16(data, cursor)?;
            let id     = pool.get(id_idx as usize).cloned().unwrap_or_default();
            let cx = read_f32(data, cursor)?;
            let cy = read_f32(data, cursor)?;
            let r  = read_f32(data, cursor)?;
            let fx = read_f32(data, cursor)?;
            let fy = read_f32(data, cursor)?;
            let stops = read_stops(data, cursor)?;
            Ok(Def::RadialGradient(RadialGradient::new(id, cx, cy, r, fx, fy, stops)))
        }
        TAG_CONIC_GRADIENT => {
            let id_idx = read_u16(data, cursor)?;
            let id     = pool.get(id_idx as usize).cloned().unwrap_or_default();
            let cx    = read_f32(data, cursor)?;
            let cy    = read_f32(data, cursor)?;
            let angle = read_f32(data, cursor)?;
            let stops = read_stops(data, cursor)?;
            Ok(Def::ConicGradient(ConicGradient::new(id, cx, cy, angle, stops)))
        }
        TAG_SHADER => {
            let id_idx          = read_u16(data, cursor)?;
            let source_ref_idx  = read_u16(data, cursor)?;
            let entry_point_idx = read_u16(data, cursor)?;
            let id          = pool.get(id_idx as usize).cloned().unwrap_or_default();
            let source_ref  = pool.get(source_ref_idx as usize).cloned().unwrap_or_default();
            let entry_point = pool.get(entry_point_idx as usize).cloned().unwrap_or_default();
            let fallback_color = read_color(data, cursor)?;

            let uniform_count = read_u16(data, cursor)? as usize;
            let mut uniforms = Vec::with_capacity(uniform_count);
            for _ in 0..uniform_count {
                let name_idx = read_u16(data, cursor)?;
                let name = pool.get(name_idx as usize).cloned().unwrap_or_default();
                let kind = read_u8(data, cursor)?;
                let value = match kind {
                    0 => ShaderUniformValue::Float(read_f32(data, cursor)? as f32),
                    1 => ShaderUniformValue::Vec2(
                        read_f32(data, cursor)? as f32,
                        read_f32(data, cursor)? as f32,
                    ),
                    2 => ShaderUniformValue::Vec3(
                        read_f32(data, cursor)? as f32,
                        read_f32(data, cursor)? as f32,
                        read_f32(data, cursor)? as f32,
                    ),
                    3 => ShaderUniformValue::Vec4(
                        read_f32(data, cursor)? as f32,
                        read_f32(data, cursor)? as f32,
                        read_f32(data, cursor)? as f32,
                        read_f32(data, cursor)? as f32,
                    ),
                    other => return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unknown shader uniform kind 0x{:02x}", other),
                    )),
                };
                uniforms.push(ShaderUniform { name, value });
            }

            let mut shader = ShaderDef::new(id, source_ref, fallback_color).with_entry_point(entry_point);
            shader.uniforms = uniforms;
            Ok(Def::Shader(shader))
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown def tag 0x{:02x}", other),
        )),
    }
}

fn read_stops(data: &[u8], cursor: &mut usize) -> io::Result<Vec<Stop>> {
    let count = read_u16(data, cursor)? as usize;
    let mut stops = Vec::with_capacity(count);
    for _ in 0..count {
        if *cursor + 8 > data.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "gradient stop truncated"));
        }
        let bytes: [u8; 8] = data[*cursor..*cursor + 8].try_into().unwrap();
        *cursor += 8;
        stops.push(Stop::from_bytes(&bytes));
    }
    Ok(stops)
}

// ── Element decoding ─────────────────────────────────────────────────────────

fn decode_element(data: &[u8], cursor: &mut usize, pool: &[String]) -> io::Result<Element> {
    let tag = read_u8(data, cursor)?;

    // Splat has no transform field; it gets its own simpler id header,
    // written before the shared id_flags read used by every other element.
    if tag == TAG_SPLAT {
        return decode_splat(data, cursor, pool);
    }

    let (id, transform) = read_id_flags(data, cursor, pool)?;

    match tag {
        TAG_RECT => {
            let x      = read_f32(data, cursor)?;
            let y      = read_f32(data, cursor)?;
            let width  = read_f32(data, cursor)?;
            let height = read_f32(data, cursor)?;
            let rx_v   = read_f32(data, cursor)?;
            let ry_v   = read_f32(data, cursor)?;
            let style  = read_style(data, cursor, pool)?;
            Ok(Element::Rect(Rect {
                x, y, width, height,
                rx: if rx_v == 0.0 { None } else { Some(rx_v) },
                ry: if ry_v == 0.0 { None } else { Some(ry_v) },
                id, transform, style,
            }))
        }
        TAG_CIRCLE => {
            let cx    = read_f32(data, cursor)?;
            let cy    = read_f32(data, cursor)?;
            let r     = read_f32(data, cursor)?;
            let style = read_style(data, cursor, pool)?;
            Ok(Element::Circle(Circle { cx, cy, r, id, transform, style }))
        }
        TAG_ELLIPSE => {
            let cx    = read_f32(data, cursor)?;
            let cy    = read_f32(data, cursor)?;
            let rx    = read_f32(data, cursor)?;
            let ry    = read_f32(data, cursor)?;
            let style = read_style(data, cursor, pool)?;
            Ok(Element::Ellipse(Ellipse { cx, cy, rx, ry, id, transform, style }))
        }
        TAG_LINE => {
            let x1    = read_f32(data, cursor)?;
            let y1    = read_f32(data, cursor)?;
            let x2    = read_f32(data, cursor)?;
            let y2    = read_f32(data, cursor)?;
            let style = read_style(data, cursor, pool)?;
            Ok(Element::Line(Line { x1, y1, x2, y2, id, transform, style }))
        }
        TAG_POLYLINE | TAG_POLYGON => {
            let count = read_u32(data, cursor)? as usize;
            let mut pts = Vec::with_capacity(count);
            for _ in 0..count {
                let (x, y) = read_point(data, cursor)?;
                pts.push(Point::new(x, y));
            }
            let style  = read_style(data, cursor, pool)?;
            let closed = tag == TAG_POLYGON;
            Ok(Element::Polyline(Polyline { points: pts, closed, id, transform, style }))
        }
        TAG_PATH => {
            let cmd_len = read_u32(data, cursor)? as usize;
            if *cursor + cmd_len > data.len() {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "path commands truncated"));
            }
            let cmd_bytes = &data[*cursor..*cursor + cmd_len];
            *cursor += cmd_len;
            let commands = decode_commands(cmd_bytes)?;
            let d_raw    = msx_ast::path::commands_to_d(&commands);
            let style    = read_style(data, cursor, pool)?;
            Ok(Element::Path(Path { commands, d_raw, id, transform, style }))
        }
        TAG_TEXT => {
            let str_idx = read_u16(data, cursor)?;
            let content = pool.get(str_idx as usize).cloned().unwrap_or_default();
            let x       = read_f32(data, cursor)?;
            let y       = read_f32(data, cursor)?;
            let style   = read_style(data, cursor, pool)?;
            Ok(Element::Text(Text { x, y, content, id, transform, style }))
        }
        TAG_GROUP => {
            let child_count = read_u32(data, cursor)? as usize;
            let mut children = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                children.push(decode_element(data, cursor, pool)?);
            }
            let style = read_style(data, cursor, pool)?;
            let group_style = if style == msx_ast::Style::empty() { None } else { Some(style) };
            Ok(Element::Group(Group { children, id, transform, style: group_style }))
        }
        TAG_USE => {
            let href_idx = read_u16(data, cursor)?;
            let href     = pool.get(href_idx as usize).cloned().unwrap_or_default();
            let x        = read_f32(data, cursor)?;
            let y        = read_f32(data, cursor)?;
            Ok(Element::Use(Use { href, x, y, id, transform }))
        }
        TAG_SDF => {
            let tree = read_sdf_tree(data, cursor)?;
            let fill = read_paint(data, cursor, pool)?;
            let has_stroke = read_u8(data, cursor)? != 0;
            let (stroke, stroke_width) = if has_stroke {
                let s = read_paint(data, cursor, pool)?;
                let w = read_f32(data, cursor)?;
                (Some(s), Some(w))
            } else {
                (None, None)
            };
            Ok(Element::Sdf(SdfNode { tree, fill, stroke, stroke_width, id, transform }))
        }
        TAG_LAYER => {
            let blend_mode = msx_ast::BlendMode::from_byte(read_u8(data, cursor)?);
            let opacity    = read_f32(data, cursor)?;
            let clip       = read_u8(data, cursor)? != 0;
            let effect_count = read_u16(data, cursor)? as usize;
            let mut effects = Vec::with_capacity(effect_count);
            for _ in 0..effect_count {
                effects.push(read_effect(data, cursor)?);
            }
            let child_count = read_u32(data, cursor)? as usize;
            let mut children = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                children.push(decode_element(data, cursor, pool)?);
            }
            Ok(Element::Layer(Layer { children, blend_mode, opacity, clip, effects, id, transform }))
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown element tag 0x{:02x}", other),
        )),
    }
}

fn decode_splat(data: &[u8], cursor: &mut usize, pool: &[String]) -> io::Result<Element> {
    let has_id = read_u8(data, cursor)? != 0;
    let id = if has_id {
        let idx = read_u16(data, cursor)?;
        Some(lookup_string(pool, idx)?.to_string())
    } else {
        None
    };
    let x        = read_f32(data, cursor)?;
    let y        = read_f32(data, cursor)?;
    let sigma_x  = read_f32(data, cursor)?;
    let sigma_y  = read_f32(data, cursor)?;
    let rotation = read_f32(data, cursor)?;
    let color    = read_color(data, cursor)?;
    let opacity  = read_f32(data, cursor)?;
    let has_fill = read_u8(data, cursor)? != 0;
    let fill = if has_fill {
        Some(read_paint(data, cursor, pool)?)
    } else {
        None
    };
    Ok(Element::Splat(GaussianSplat { x, y, sigma_x, sigma_y, rotation, color, fill, opacity, id }))
              }
