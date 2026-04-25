// src/lib.rs

pub mod ast;
pub mod color;
pub mod compiler;
pub mod decoder;
pub mod encoder;
pub mod header;
pub mod parser;
pub mod path;
pub mod primitives;
pub mod renderer;
pub mod style;
pub mod transform;

pub use ast::Scene;
pub use color::{Color, Paint};
pub use compiler::compile;
pub use header::{MsxHeader, HEADER_SIZE};
pub use parser::{parse_scene, parse_scene_file, parse_scene_from_data};
pub use renderer::render;

use std::io;

/// Parse an MSX source string and render it directly to SVG.
pub fn source_to_svg(source: &str) -> Result<String, String> {
    let scene = parse_scene(source)?;
    Ok(render(&scene))
}

/// Parse an MSX source file from disk and render to SVG.
pub fn file_to_svg(path: &str) -> Result<String, String> {
    let scene = parse_scene_file(path)?;
    Ok(render(&scene))
}

/// Parse an MSX source string and compile to binary.
pub fn source_to_binary(source: &str, compress: bool) -> Result<Vec<u8>, String> {
    let scene = parse_scene(source)?;
    compile(&scene, compress)
        .map_err(|e| format!("compile error: {}", e))
}

/// Decode a binary MSX file back to a Scene.
pub fn decode(data: &[u8]) -> io::Result<Scene> {
    use crate::header::{MsxHeader, HEADER_SIZE, COMPRESS_MBFA};
    use crate::decoder::*;
    use crate::ast::*;
    use crate::color::{LinearGradient, RadialGradient, Stop};
    use crate::path::decode_commands;
    use crate::primitives::{Point, ViewBox};
    use crate::compiler::{
        TAG_RECT, TAG_CIRCLE, TAG_ELLIPSE, TAG_LINE,
        TAG_POLYLINE, TAG_POLYGON, TAG_PATH, TAG_TEXT,
        TAG_GROUP, TAG_USE, TAG_LINEAR_GRADIENT, TAG_RADIAL_GRADIENT, TAG_END,
    };

    let header = MsxHeader::parse(data)?;

    // Decompress payload if needed
    let payload_raw = &data[HEADER_SIZE..];
    let payload_owned: Vec<u8>;
    let payload: &[u8] = if header.compress == COMPRESS_MBFA {
        payload_owned = mbfa::decompress(payload_raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData,
                format!("MBFA decompress: {}", e)))?;
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
    let mut defs: Vec<Def> = Vec::new();
    for _ in 0..header.def_count {
        let def = decode_def(payload, &mut cursor, &pool)?;
        defs.push(def);
    }

    // Elements
    let mut elements: Vec<Element> = Vec::new();
    for _ in 0..header.elem_count {
        let elem = decode_element(payload, &mut cursor, &pool)?;
        elements.push(elem);
    }

    let mut canvas = Canvas::new(
        header.width  as f64,
        header.height as f64,
        bg,
    );
    canvas.viewbox = viewbox;

    let mut scene = Scene::new(canvas);
    scene.defs     = defs;
    scene.elements = elements;
    Ok(scene)
}

// ── Decode helpers ────────────────────────────────────────────────────────────

fn decode_def(data: &[u8], cursor: &mut usize, pool: &[String]) -> io::Result<Def> {
    use crate::decoder::*;
    use crate::compiler::{TAG_LINEAR_GRADIENT, TAG_RADIAL_GRADIENT};
    use crate::color::{LinearGradient, RadialGradient, Stop};
    use crate::ast::Def;

    let tag = read_u8(data, cursor)?;
    match tag {
        TAG_LINEAR_GRADIENT => {
            let id_idx = read_u16(data, cursor)?;
            let id     = pool.get(id_idx as usize)
                .cloned()
                .unwrap_or_default();
            let x1 = read_f32(data, cursor)?;
            let y1 = read_f32(data, cursor)?;
            let x2 = read_f32(data, cursor)?;
            let y2 = read_f32(data, cursor)?;
            let stop_count = read_u16(data, cursor)? as usize;
            let mut stops = Vec::with_capacity(stop_count);
            for _ in 0..stop_count {
                if *cursor + 8 > data.len() {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stop truncated"));
                }
                let bytes: [u8; 8] = data[*cursor..*cursor + 8].try_into().unwrap();
                *cursor += 8;
                stops.push(Stop::from_bytes(&bytes));
            }
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
            let stop_count = read_u16(data, cursor)? as usize;
            let mut stops = Vec::with_capacity(stop_count);
            for _ in 0..stop_count {
                if *cursor + 8 > data.len() {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stop truncated"));
                }
                let bytes: [u8; 8] = data[*cursor..*cursor + 8].try_into().unwrap();
                *cursor += 8;
                stops.push(Stop::from_bytes(&bytes));
            }
            Ok(Def::RadialGradient(RadialGradient::new(id, cx, cy, r, fx, fy, stops)))
        }
        other => Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("unknown def tag 0x{:02x}", other))),
    }
}

fn decode_element(data: &[u8], cursor: &mut usize, pool: &[String]) -> io::Result<Element> {
    use crate::decoder::*;
    use crate::compiler::*;
    use crate::ast::*;
    use crate::path::decode_commands;
    use crate::primitives::Point;

    let tag = read_u8(data, cursor)?;
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
            let count  = read_u32(data, cursor)? as usize;
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
            let d_raw    = crate::path::commands_to_d(&commands);
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
            let group_style = if style == crate::style::Style::empty() { None } else { Some(style) };
            Ok(Element::Group(Group { children, id, transform, style: group_style }))
        }
        TAG_USE => {
            let href_idx = read_u16(data, cursor)?;
            let href     = pool.get(href_idx as usize).cloned().unwrap_or_default();
            let x        = read_f32(data, cursor)?;
            let y        = read_f32(data, cursor)?;
            Ok(Element::Use(Use { href, x, y, id, transform }))
        }
        other => Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("unknown element tag 0x{:02x}", other))),
    }
}
