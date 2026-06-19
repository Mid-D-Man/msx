// core/msx-parser/src/element.rs
use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{
    path::parse_d, Circle, Element, Ellipse, Group, Line, Path, Point, Polyline, Rect, Text,
    Transform, Use,
};

use crate::dix_helpers::{array_len, opt, path_present, type_tag};
use crate::layer::parse_layer;
use crate::sdf::parse_sdf_node;
use crate::splat::parse_splat;
use crate::style::parse_style;
use crate::transform::parse_transform;

pub fn parse_elements(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Element>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(parse_element(data, &format!("{}[{}]", path, i))?);
    }
    Ok(out)
}

pub fn parse_element(data: &DixData, prefix: &str) -> Result<Element, String> {
    let kind = type_tag(data, prefix)?;
    match kind.as_str() {
        "rect"     => parse_rect(data, prefix).map(Element::Rect),
        "circle"   => parse_circle(data, prefix).map(Element::Circle),
        "ellipse"  => parse_ellipse(data, prefix).map(Element::Ellipse),
        "line"     => parse_line(data, prefix).map(Element::Line),
        "polyline" => parse_polyline(data, prefix, false).map(Element::Polyline),
        "polygon"  => parse_polyline(data, prefix, true).map(Element::Polygon),
        "path"     => parse_path(data, prefix).map(Element::Path),
        "text"     => parse_text(data, prefix).map(Element::Text),
        "group"    => parse_group(data, prefix).map(Element::Group),
        "use"      => parse_use(data, prefix).map(Element::Use),
        "sdf"      => parse_sdf_node(data, prefix).map(Element::Sdf),
        "splat"    => parse_splat(data, prefix).map(Element::Splat),
        "layer"    => parse_layer(data, prefix).map(Element::Layer),
        other => Err(format!("{}: unknown element type '{}'", prefix, other)),
    }
}

/// `id` + `transform` are shared by every element type except `splat`
/// (which has no transform field in msx-ast) — read together to keep each
/// shape parser below to one line per field.
fn id_and_transform(data: &DixData, prefix: &str) -> Result<(Option<String>, Option<Transform>), String> {
    let id = opt::<String>(data, prefix, "id")?;
    let transform = parse_transform(data, prefix)?;
    Ok((id, transform))
}

fn parse_rect(data: &DixData, prefix: &str) -> Result<Rect, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    Ok(Rect {
        x:      opt::<f64>(data, prefix, "x")?.ok_or_else(|| format!("{}.x: required", prefix))?,
        y:      opt::<f64>(data, prefix, "y")?.ok_or_else(|| format!("{}.y: required", prefix))?,
        width:  opt::<f64>(data, prefix, "width")?.ok_or_else(|| format!("{}.width: required", prefix))?,
        height: opt::<f64>(data, prefix, "height")?.ok_or_else(|| format!("{}.height: required", prefix))?,
        rx: opt::<f64>(data, prefix, "rx")?,
        ry: opt::<f64>(data, prefix, "ry")?,
        id, transform,
        style: parse_style(data, prefix)?,
    })
}

fn parse_circle(data: &DixData, prefix: &str) -> Result<Circle, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    Ok(Circle {
        cx: opt::<f64>(data, prefix, "cx")?.ok_or_else(|| format!("{}.cx: required", prefix))?,
        cy: opt::<f64>(data, prefix, "cy")?.ok_or_else(|| format!("{}.cy: required", prefix))?,
        r:  opt::<f64>(data, prefix, "r")?.ok_or_else(|| format!("{}.r: required", prefix))?,
        id, transform,
        style: parse_style(data, prefix)?,
    })
}

fn parse_ellipse(data: &DixData, prefix: &str) -> Result<Ellipse, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    Ok(Ellipse {
        cx: opt::<f64>(data, prefix, "cx")?.ok_or_else(|| format!("{}.cx: required", prefix))?,
        cy: opt::<f64>(data, prefix, "cy")?.ok_or_else(|| format!("{}.cy: required", prefix))?,
        rx: opt::<f64>(data, prefix, "rx")?.ok_or_else(|| format!("{}.rx: required", prefix))?,
        ry: opt::<f64>(data, prefix, "ry")?.ok_or_else(|| format!("{}.ry: required", prefix))?,
        id, transform,
        style: parse_style(data, prefix)?,
    })
}

fn parse_line(data: &DixData, prefix: &str) -> Result<Line, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    Ok(Line {
        x1: opt::<f64>(data, prefix, "x1")?.ok_or_else(|| format!("{}.x1: required", prefix))?,
        y1: opt::<f64>(data, prefix, "y1")?.ok_or_else(|| format!("{}.y1: required", prefix))?,
        x2: opt::<f64>(data, prefix, "x2")?.ok_or_else(|| format!("{}.x2: required", prefix))?,
        y2: opt::<f64>(data, prefix, "y2")?.ok_or_else(|| format!("{}.y2: required", prefix))?,
        id, transform,
        style: parse_style(data, prefix)?,
    })
}

fn parse_polyline(data: &DixData, prefix: &str, closed: bool) -> Result<Polyline, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    let path = dix_path(prefix, "points");
    let count = array_len(data, prefix, "points")?;
    let mut points = Vec::with_capacity(count);
    for i in 0..count {
        let pair_path = format!("{}[{}]", path, i);
        let x: f64 = data.get(&format!("{}[0]", pair_path))?;
        let y: f64 = data.get(&format!("{}[1]", pair_path))?;
        points.push(Point::new(x, y));
    }
    Ok(Polyline { points, closed, id, transform, style: parse_style(data, prefix)? })
}

fn parse_path(data: &DixData, prefix: &str) -> Result<Path, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    let d_raw = opt::<String>(data, prefix, "d")?.ok_or_else(|| format!("{}.d: required", prefix))?;
    let commands = parse_d(&d_raw).map_err(|e| format!("{}.d: {}", prefix, e))?;
    Ok(Path { commands, d_raw, id, transform, style: parse_style(data, prefix)? })
}

fn parse_text(data: &DixData, prefix: &str) -> Result<Text, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    Ok(Text {
        x: opt::<f64>(data, prefix, "x")?.ok_or_else(|| format!("{}.x: required", prefix))?,
        y: opt::<f64>(data, prefix, "y")?.ok_or_else(|| format!("{}.y: required", prefix))?,
        content: opt::<String>(data, prefix, "content")?
            .ok_or_else(|| format!("{}.content: required", prefix))?,
        id, transform,
        style: parse_style(data, prefix)?,
    })
}

fn parse_group(data: &DixData, prefix: &str) -> Result<Group, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    let children = parse_elements(data, prefix, "elements")?;
    let style = if path_present(data, &dix_path(prefix, "style")) {
        Some(parse_style(data, prefix)?)
    } else {
        None
    };
    Ok(Group { children, id, transform, style })
}

fn parse_use(data: &DixData, prefix: &str) -> Result<Use, String> {
    let (id, transform) = id_and_transform(data, prefix)?;
    Ok(Use {
        href: opt::<String>(data, prefix, "href")?.ok_or_else(|| format!("{}.href: required", prefix))?,
        x: opt::<f64>(data, prefix, "x")?.unwrap_or(0.0),
        y: opt::<f64>(data, prefix, "y")?.unwrap_or(0.0),
        id, transform,
    })
  }
