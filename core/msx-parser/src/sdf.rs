// core/msx-parser/src/sdf.rs
use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{Paint, SdfNode, SdfTree};

use crate::dix_helpers::{array_len, opt, paint_from, raw, type_tag};
use crate::transform::parse_transform;

/// `{ type = "sdf", fill = #c83232, stroke = "none"?, stroke_width = ..?,
///    tree = { ...recursive SdfTree... } }`
pub fn parse_sdf_node(data: &DixData, prefix: &str) -> Result<SdfNode, String> {
    let id = opt::<String>(data, prefix, "id")?;
    let transform = parse_transform(data, prefix)?;
    let tree = parse_sdf_tree(data, &dix_path(prefix, "tree"))?;
    let fill = paint_from(raw(data, prefix, "fill")).unwrap_or(Paint::None);

    let stroke = paint_from(raw(data, prefix, "stroke"));
    let stroke_width = if stroke.is_some() {
        opt::<f64>(data, prefix, "stroke_width")?
    } else {
        None
    };

    Ok(SdfNode { tree, fill, stroke, stroke_width, id, transform })
}

fn parse_sdf_tree(data: &DixData, prefix: &str) -> Result<SdfTree, String> {
    let kind = type_tag(data, prefix)?;
    match kind.as_str() {
        "circle" => Ok(SdfTree::Circle {
            cx: opt::<f64>(data, prefix, "cx")?.unwrap_or(0.0),
            cy: opt::<f64>(data, prefix, "cy")?.unwrap_or(0.0),
            r:  opt::<f64>(data, prefix, "r")?.ok_or_else(|| format!("{}.r: required", prefix))?,
        }),
        "box" => Ok(SdfTree::Box {
            x: opt::<f64>(data, prefix, "x")?.unwrap_or(0.0),
            y: opt::<f64>(data, prefix, "y")?.unwrap_or(0.0),
            width:  opt::<f64>(data, prefix, "width")?.ok_or_else(|| format!("{}.width: required", prefix))?,
            height: opt::<f64>(data, prefix, "height")?.ok_or_else(|| format!("{}.height: required", prefix))?,
            corner_radius: opt::<f64>(data, prefix, "corner_radius")?.unwrap_or(0.0),
        }),
        "line" => Ok(SdfTree::Line {
            x1: opt::<f64>(data, prefix, "x1")?.unwrap_or(0.0),
            y1: opt::<f64>(data, prefix, "y1")?.unwrap_or(0.0),
            x2: opt::<f64>(data, prefix, "x2")?.unwrap_or(0.0),
            y2: opt::<f64>(data, prefix, "y2")?.unwrap_or(0.0),
            thickness: opt::<f64>(data, prefix, "thickness")?.unwrap_or(1.0),
        }),
        "ring" => Ok(SdfTree::Ring {
            cx: opt::<f64>(data, prefix, "cx")?.unwrap_or(0.0),
            cy: opt::<f64>(data, prefix, "cy")?.unwrap_or(0.0),
            r:  opt::<f64>(data, prefix, "r")?.ok_or_else(|| format!("{}.r: required", prefix))?,
            thickness: opt::<f64>(data, prefix, "thickness")?.unwrap_or(1.0),
        }),
        "arc" => Ok(SdfTree::Arc {
            cx: opt::<f64>(data, prefix, "cx")?.unwrap_or(0.0),
            cy: opt::<f64>(data, prefix, "cy")?.unwrap_or(0.0),
            r:  opt::<f64>(data, prefix, "r")?.ok_or_else(|| format!("{}.r: required", prefix))?,
            angle_start: opt::<f64>(data, prefix, "angle_start")?.unwrap_or(0.0),
            angle_end:   opt::<f64>(data, prefix, "angle_end")?.unwrap_or(std::f64::consts::TAU),
            thickness:   opt::<f64>(data, prefix, "thickness")?.unwrap_or(1.0),
        }),
        "union" => Ok(SdfTree::Union(parse_sdf_children(data, prefix)?)),
        "smooth_union" => Ok(SdfTree::SmoothUnion {
            children: parse_sdf_children(data, prefix)?,
            k: opt::<f64>(data, prefix, "k")?.unwrap_or(0.1),
        }),
        "subtract" => {
            let (a, b) = parse_sdf_pair(data, prefix)?;
            Ok(SdfTree::Subtract { a: Box::new(a), b: Box::new(b) })
        }
        "smooth_subtract" => {
            let (a, b) = parse_sdf_pair(data, prefix)?;
            Ok(SdfTree::SmoothSubtract {
                a: Box::new(a), b: Box::new(b),
                k: opt::<f64>(data, prefix, "k")?.unwrap_or(0.1),
            })
        }
        "intersect" => {
            let (a, b) = parse_sdf_pair(data, prefix)?;
            Ok(SdfTree::Intersect { a: Box::new(a), b: Box::new(b) })
        }
        "smooth_intersect" => {
            let (a, b) = parse_sdf_pair(data, prefix)?;
            Ok(SdfTree::SmoothIntersect {
                a: Box::new(a), b: Box::new(b),
                k: opt::<f64>(data, prefix, "k")?.unwrap_or(0.1),
            })
        }
        "offset" => {
            let child = parse_sdf_tree(data, &dix_path(prefix, "child"))?;
            Ok(SdfTree::Offset {
                child: Box::new(child),
                amount: opt::<f64>(data, prefix, "amount")?.unwrap_or(0.0),
            })
        }
        other => Err(format!("{}: unknown sdf node type '{}'", prefix, other)),
    }
}

fn parse_sdf_children(data: &DixData, prefix: &str) -> Result<Vec<SdfTree>, String> {
    let path = dix_path(prefix, "children");
    let count = array_len(data, prefix, "children")?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(parse_sdf_tree(data, &format!("{}[{}]", path, i))?);
    }
    Ok(out)
}

fn parse_sdf_pair(data: &DixData, prefix: &str) -> Result<(SdfTree, SdfTree), String> {
    let a = parse_sdf_tree(data, &dix_path(prefix, "a"))?;
    let b = parse_sdf_tree(data, &dix_path(prefix, "b"))?;
    Ok((a, b))
}
