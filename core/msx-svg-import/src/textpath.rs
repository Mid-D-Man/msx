// core/msx-svg-import/src/textpath.rs
//! The path a `textPath` follows: the `path` attribute, or the element `href`
//! points at, with that element's own transform, and the start offset.

use msx_ast::path::parse_d;
use msx_ast::{Matrix2D, PathCommand, Point, TextPath};
use roxmltree::Node;

use crate::parse::{parse_css_transform, parse_length, parse_transform};
use crate::{Importer, XLINK_NS};

impl<'a, 'i> Importer<'a, 'i> {
    /// The path and start offset of a `textPath`, or why it has none.
    pub(crate) fn text_path_source(&mut self, tp: Node<'a, 'i>, font_size: f64) -> Result<TextPath, String> {
        let (mut commands, target) = if let Some(d) = tp.attribute("path") {
            (parse_d(d).map_err(|e| format!("has a path=\"{}\" that cannot be read ({e})", d.trim()))?, None)
        } else {
            let href = tp.attribute("href").or_else(|| tp.attribute((XLINK_NS, "href"))).map(str::trim).and_then(|h| h.strip_prefix('#'));
            let target = href.and_then(|id| self.ids.get(id).copied()).ok_or("does not point at an element")?;
            (self.shape_commands(target, font_size)?, Some(target))
        };
        // Only the transform of the referenced element itself counts.
        if let Some(target) = target {
            let props = self.props_for(target);
            let parsed = match (props.get("transform"), target.attribute("transform")) {
                (Some(css), _) => parse_css_transform(css),
                (None, Some(attr)) => parse_transform(attr),
                (None, None) => Ok(None),
            };
            match parsed {
                Ok(Some(t)) => commands = flatten_through(&commands, t.to_matrix()),
                Ok(None) => {}
                Err(e) => {
                    let msg = format!("{}: {e}; transform ignored", Self::label(&target));
                    self.warn(msg);
                }
            }
        }
        let length = msx_text::path_length(&commands);
        if length <= 0.0 {
            return Err("has a path with no length".into());
        }
        let start_offset = tp.attribute("startOffset").and_then(parse_length).map_or(0.0, |l| l.to_px(font_size, length));
        Ok(TextPath { commands, start_offset })
    }

    /// The outline of a shape as path commands, first corner at the start.
    fn shape_commands(&self, n: Node<'a, 'i>, fs: f64) -> Result<Vec<PathCommand>, String> {
        let (w, h) = self.vp;
        let num = |name: &str, base: f64| self.first_coord(n, name, fs, base);
        let pt = |x: f64, y: f64| Point::new(x, y);
        let arc = |r: (f64, f64), to: Point| PathCommand::Arc { rx: r.0, ry: r.1, x_rotation: 0.0, large_arc: false, sweep: true, to };
        match n.tag_name().name() {
            "path" => parse_d(n.attribute("d").ok_or("points at a path with no d")?).map_err(|e| format!("points at a path that cannot be read ({e})")),
            "rect" => {
                let (x, y) = (num("x", w).unwrap_or(0.0), num("y", h).unwrap_or(0.0));
                let (rw, rh) = (num("width", w).unwrap_or(0.0), num("height", h).unwrap_or(0.0));
                if rw <= 0.0 || rh <= 0.0 {
                    return Err("points at a rect with no size".into());
                }
                let (rx, ry) = match (num("rx", w), num("ry", h)) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) | (None, Some(a)) => (a, a),
                    (None, None) => (0.0, 0.0),
                };
                let (rx, ry) = (rx.clamp(0.0, rw / 2.0), ry.clamp(0.0, rh / 2.0));
                if rx == 0.0 || ry == 0.0 {
                    return Ok(vec![PathCommand::MoveTo(pt(x, y)), PathCommand::HLineTo(x + rw), PathCommand::VLineTo(y + rh), PathCommand::HLineTo(x), PathCommand::ClosePath]);
                }
                Ok(vec![
                    PathCommand::MoveTo(pt(x + rx, y)),
                    PathCommand::HLineTo(x + rw - rx),
                    arc((rx, ry), pt(x + rw, y + ry)),
                    PathCommand::VLineTo(y + rh - ry),
                    arc((rx, ry), pt(x + rw - rx, y + rh)),
                    PathCommand::HLineTo(x + rx),
                    arc((rx, ry), pt(x, y + rh - ry)),
                    PathCommand::VLineTo(y + ry),
                    arc((rx, ry), pt(x + rx, y)),
                    PathCommand::ClosePath,
                ])
            }
            "circle" | "ellipse" => {
                let (cx, cy) = (num("cx", w).unwrap_or(0.0), num("cy", h).unwrap_or(0.0));
                let circle = n.tag_name().name() == "circle";
                let (rx, ry) = if circle {
                    let r = num("r", (w * w + h * h).sqrt() / 2f64.sqrt()).unwrap_or(0.0);
                    (r, r)
                } else {
                    (num("rx", w).unwrap_or(0.0), num("ry", h).unwrap_or(0.0))
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return Err(format!("points at a {} with no size", n.tag_name().name()));
                }
                Ok(vec![
                    PathCommand::MoveTo(pt(cx + rx, cy)),
                    arc((rx, ry), pt(cx, cy + ry)),
                    arc((rx, ry), pt(cx - rx, cy)),
                    arc((rx, ry), pt(cx, cy - ry)),
                    arc((rx, ry), pt(cx + rx, cy)),
                    PathCommand::ClosePath,
                ])
            }
            "line" => {
                let (x1, y1, x2, y2) = (num("x1", w).unwrap_or(0.0), num("y1", h).unwrap_or(0.0), num("x2", w).unwrap_or(0.0), num("y2", h).unwrap_or(0.0));
                Ok(vec![PathCommand::MoveTo(pt(x1, y1)), PathCommand::LineTo(pt(x2, y2))])
            }
            "polyline" | "polygon" => {
                let nums: Vec<f64> = n.attribute("points").unwrap_or("").split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).map_while(|t| t.parse().ok()).collect();
                if nums.len() < 4 {
                    return Err(format!("points at a {} with fewer than two points", n.tag_name().name()));
                }
                let mut cmds: Vec<PathCommand> = nums.chunks_exact(2).enumerate().map(|(i, p)| if i == 0 { PathCommand::MoveTo(pt(p[0], p[1])) } else { PathCommand::LineTo(pt(p[0], p[1])) }).collect();
                if n.tag_name().name() == "polygon" {
                    cmds.push(PathCommand::ClosePath);
                }
                Ok(cmds)
            }
            other => Err(format!("points at a <{other}>, which has no outline to follow")),
        }
    }
}

/// The first subpath as straight lines, moved by `m`. Curves become many short
/// lines, which is all a text on a path needs from them.
fn flatten_through(cmds: &[PathCommand], m: Matrix2D) -> Vec<PathCommand> {
    msx_text::path_points(cmds)
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = m.transform_point(Point::new(p.0, p.1));
            if i == 0 { PathCommand::MoveTo(q) } else { PathCommand::LineTo(q) }
        })
        .collect()
}
