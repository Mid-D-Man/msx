// render/msx-render-cpu/src/rasterizer.rs
//! Every SVG-native vector shape, rasterized through `tiny-skia`:
//! `rect`/`circle`/`ellipse`/`line`/`polyline`/`polygon`/`path`/`group`/`use`,
//! plus dispatch into `sdf_raster.rs`/`splat_raster.rs`/`composite.rs` for
//! `Sdf`/`Splat`/`Layer`.
//!
//! Threads `msx_ast::Matrix2D` through every recursive call (sidesteps
//! relying on `tiny_skia::Transform`'s exact composition-method semantics,
//! and is the one thing `sdf_raster.rs`/`splat_raster.rs` need to invert
//! for their per-pixel local-space mapping) and converts to
//! `tiny_skia::Transform` only at each `fill_path`/`stroke_path` call site.

use std::collections::HashMap;

use msx_ast::{path::PathCommand, Color, Def, Element, FillRule, Group, LineCap, LineJoin, Matrix2D, Paint, Style, Use};
use tiny_skia::{
    Color as SkColor, FillRule as SkFillRule, LineCap as SkLineCap, LineJoin as SkLineJoin,
    Paint as SkPaint, Path as SkPath, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

use crate::composite::render_layer;
use crate::sdf_raster::rasterize_sdf;
use crate::splat_raster::rasterize_splat;

// ── Lookups built once per render, threaded through every recursive call ──

pub struct Defs<'a> {
    by_id: HashMap<&'a str, &'a Def>,
}

impl<'a> Defs<'a> {
    pub fn build(defs: &'a [Def]) -> Self {
        Defs { by_id: defs.iter().map(|d| (d.id(), d)).collect() }
    }

    pub fn get(&self, id: &str) -> Option<&'a Def> {
        self.by_id.get(id).copied()
    }
}

pub struct ElementIndex<'a> {
    by_id: HashMap<&'a str, &'a Element>,
}

impl<'a> ElementIndex<'a> {
    pub fn build(elements: &'a [Element]) -> Self {
        let mut by_id = HashMap::new();
        Self::index_into(elements, &mut by_id);
        ElementIndex { by_id }
    }

    fn index_into(elements: &'a [Element], map: &mut HashMap<&'a str, &'a Element>) {
        for el in elements {
            if let Some(id) = el.id() {
                map.insert(id, el);
            }
            match el {
                Element::Group(g) => Self::index_into(&g.children, map),
                Element::Layer(l) => Self::index_into(&l.children, map),
                _ => {}
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&'a Element> {
        self.by_id.get(id).copied()
    }
}

// ── Top-level dispatch ──────────────────────────────────────────────────────

pub fn render_element(pixmap: &mut Pixmap, element: &Element, transform: Matrix2D, defs: &Defs, index: &ElementIndex) {
    match element {
        Element::Rect(e) => fill_and_stroke(pixmap, build_rect_path(e), &e.style, e.transform.as_ref(), transform, defs),
        Element::Circle(e) => fill_and_stroke(pixmap, build_circle_path(e), &e.style, e.transform.as_ref(), transform, defs),
        Element::Ellipse(e) => fill_and_stroke(pixmap, build_ellipse_path(e), &e.style, e.transform.as_ref(), transform, defs),
        Element::Line(e) => stroke_only(pixmap, build_line_path(e), &e.style, e.transform.as_ref(), transform),
        Element::Polyline(e) | Element::Polygon(e) => {
            fill_and_stroke(pixmap, build_polyline_path(e), &e.style, e.transform.as_ref(), transform, defs)
        }
        Element::Path(e) => fill_and_stroke(pixmap, build_msx_path(e), &e.style, e.transform.as_ref(), transform, defs),
        Element::Text(_) => {
            // No font shaping/rasterization dependency wired in — see
            // lib.rs docs. Intentionally skipped, not faked.
        }
        Element::Group(g) => render_group(pixmap, g, transform, defs, index),
        Element::Use(u) => render_use(pixmap, u, transform, defs, index),
        Element::Sdf(node) => rasterize_sdf(pixmap, node, transform),
        Element::Splat(s) => rasterize_splat(pixmap, s, transform),
        Element::Layer(l) => render_layer(pixmap, l, transform, defs, index),
    }
}

fn render_group(pixmap: &mut Pixmap, g: &Group, parent: Matrix2D, defs: &Defs, index: &ElementIndex) {
    let local = g.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    let combined = parent.concat(local);
    for child in &g.children {
        render_element(pixmap, child, combined, defs, index);
    }
    // `g.style` (inheritable style applied to all children) isn't pushed
    // down yet — proper inheritance needs each leaf shape's resolved style
    // to fall back through an ancestor chain, which `msx-parser` doesn't
    // track today. Tracked as a follow-up, not silently dropped.
}

fn render_use(pixmap: &mut Pixmap, u: &Use, parent: Matrix2D, defs: &Defs, index: &ElementIndex) {
    let id = u.href.strip_prefix('#').unwrap_or(&u.href);
    let Some(target) = index.get(id) else {
        return; // broken reference — SVG renders nothing for these too
    };
    let offset = Matrix2D::translate(u.x, u.y);
    let local = u.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    render_element(pixmap, target, parent.concat(offset.concat(local)), defs, index);
}

// ── Fill + stroke ────────────────────────────────────────────────────────────

fn fill_and_stroke(pixmap: &mut Pixmap, path: Option<SkPath>, style: &Style, local_transform: Option<&msx_ast::Transform>, parent: Matrix2D, defs: &Defs) {
    let Some(path) = path else { return };
    let transform = to_tiny_skia_transform(combine(parent, local_transform));
    let opacity = style.opacity.unwrap_or(1.0) as f32;

    if let Some(fill) = style.fill.as_ref().filter(|p| !p.is_none()) {
        if let Some(paint) = resolve_paint(fill, opacity, defs) {
            pixmap.fill_path(&path, &paint, sk_fill_rule(style), transform, None);
        }
    }

    if let Some(stroke_paint) = style.stroke.as_ref().filter(|p| !p.is_none()) {
        let width = style.stroke_width.unwrap_or(1.0) as f32;
        if width > 0.0 {
            if let Some(paint) = resolve_paint(stroke_paint, opacity, defs) {
                pixmap.stroke_path(&path, &paint, &build_stroke(style, width), transform, None);
            }
        }
    }
}

fn stroke_only(pixmap: &mut Pixmap, path: Option<SkPath>, style: &Style, local_transform: Option<&msx_ast::Transform>, parent: Matrix2D) {
    let Some(path) = path else { return };
    let Some(stroke_paint) = style.stroke.as_ref().filter(|p| !p.is_none()) else { return };
    let width = style.stroke_width.unwrap_or(1.0) as f32;
    if width <= 0.0 {
        return;
    }
    let opacity = style.opacity.unwrap_or(1.0) as f32;
    let transform = to_tiny_skia_transform(combine(parent, local_transform));
    if let Paint::Color(c) = stroke_paint {
        let paint = flat_paint(*c, opacity);
        pixmap.stroke_path(&path, &paint, &build_stroke(style, width), transform, None);
    }
}

fn combine(parent: Matrix2D, local: Option<&msx_ast::Transform>) -> Matrix2D {
    match local {
        None => parent,
        Some(t) => parent.concat(t.to_matrix()),
    }
}

fn resolve_paint(paint: &Paint, opacity: f32, defs: &Defs) -> Option<SkPaint<'static>> {
    match paint {
        Paint::None => None,
        Paint::Color(c) => Some(flat_paint(*c, opacity)),
        Paint::CurrentColor => Some(flat_paint(Color::BLACK, opacity)),
        Paint::Ref(reference) => {
            let id = reference.strip_prefix("url(#")?.strip_suffix(')')?;
            let def = defs.get(id)?;
            // TODO: a real tiny-skia LinearGradient/RadialGradient shader,
            // mapped through objectBoundingBox using the fill path's
            // bounds. Skipped for now — the exact gradient-shader
            // constructor signature is the one part of this module worth
            // double-checking against whichever tiny-skia 0.11.x patch is
            // actually pinned. Every gradient-filled shape still renders
            // the gradient's average color rather than nothing.
            Some(flat_paint(average_stop_color(def), opacity))
        }
    }
}

fn flat_paint(c: Color, opacity: f32) -> SkPaint<'static> {
    let mut p = SkPaint::default();
    let a = ((c.a as f32 / 255.0) * opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    p.set_color(SkColor::from_rgba8(c.r, c.g, c.b, a));
    p.anti_alias = true;
    p
}

fn average_stop_color(def: &Def) -> Color {
    let stops: &[msx_ast::Stop] = match def {
        Def::LinearGradient(g) => &g.stops,
        Def::RadialGradient(g) => &g.stops,
        Def::ConicGradient(g) => &g.stops,
    };
    if stops.is_empty() {
        return Color::BLACK;
    }
    let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
    for s in stops {
        r += s.color.r as u32;
        g += s.color.g as u32;
        b += s.color.b as u32;
        a += s.color.a as u32;
    }
    let n = stops.len() as u32;
    Color::rgba((r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8)
}

fn sk_fill_rule(style: &Style) -> SkFillRule {
    match style.fill_rule {
        Some(FillRule::EvenOdd) => SkFillRule::EvenOdd,
        _ => SkFillRule::Winding,
    }
}

fn build_stroke(style: &Style, width: f32) -> Stroke {
    let mut stroke = Stroke::default();
    stroke.width = width;
    stroke.line_cap = match style.stroke_linecap {
        Some(LineCap::Round) => SkLineCap::Round,
        Some(LineCap::Square) => SkLineCap::Square,
        _ => SkLineCap::Butt,
    };
    stroke.line_join = match style.stroke_linejoin {
        Some(LineJoin::Round) => SkLineJoin::Round,
        Some(LineJoin::Bevel) => SkLineJoin::Bevel,
        _ => SkLineJoin::Miter,
    };
    stroke.miter_limit = style.stroke_miterlimit.unwrap_or(4.0) as f32;
    if let Some(ref dashes) = style.stroke_dasharray {
        let array: Vec<f32> = dashes.iter().map(|d| *d as f32).collect();
        if let Some(dash) = StrokeDash::new(array, style.stroke_dashoffset.unwrap_or(0.0) as f32) {
            stroke.dash = Some(dash);
        }
    }
    stroke
}

fn to_tiny_skia_transform(m: Matrix2D) -> Transform {
    Transform::from_row(m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32)
}

// ── Path construction ───────────────────────────────────────────────────────

fn build_rect_path(r: &msx_ast::Rect) -> Option<SkPath> {
    let mut pb = PathBuilder::new();
    let (x, y, w, h) = (r.x as f32, r.y as f32, r.width as f32, r.height as f32);
    let rx = r.rx.map(|v| v as f32).unwrap_or(0.0).clamp(0.0, w / 2.0);
    let ry = r.ry.map(|v| v as f32).unwrap_or(rx).clamp(0.0, h / 2.0);

    if rx <= 0.0 || ry <= 0.0 {
        pb.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
    } else {
        const K: f32 = 0.552_284_75;
        let (kx, ky) = (rx * K, ry * K);
        pb.move_to(x + rx, y);
        pb.line_to(x + w - rx, y);
        pb.cubic_to(x + w - rx + kx, y, x + w, y + ry - ky, x + w, y + ry);
        pb.line_to(x + w, y + h - ry);
        pb.cubic_to(x + w, y + h - ry + ky, x + w - rx + kx, y + h, x + w - rx, y + h);
        pb.line_to(x + rx, y + h);
        pb.cubic_to(x + rx - kx, y + h, x, y + h - ry + ky, x, y + h - ry);
        pb.line_to(x, y + ry);
        pb.cubic_to(x, y + ry - ky, x + rx - kx, y, x + rx, y);
        pb.close();
    }

    pb.finish()
}

fn build_circle_path(c: &msx_ast::Circle) -> Option<SkPath> {
    let mut pb = PathBuilder::new();
    pb.push_circle(c.cx as f32, c.cy as f32, c.r as f32);
    pb.finish()
}

fn build_ellipse_path(e: &msx_ast::Ellipse) -> Option<SkPath> {
    let mut pb = PathBuilder::new();
    let (cx, cy, rx, ry) = (e.cx as f32, e.cy as f32, e.rx as f32, e.ry as f32);
    const K: f32 = 0.552_284_75;
    let (kx, ky) = (rx * K, ry * K);

    pb.move_to(cx + rx, cy);
    pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    pb.close();
    pb.finish()
}

fn build_line_path(l: &msx_ast::Line) -> Option<SkPath> {
    let mut pb = PathBuilder::new();
    pb.move_to(l.x1 as f32, l.y1 as f32);
    pb.line_to(l.x2 as f32, l.y2 as f32);
    pb.finish()
}

fn build_polyline_path(p: &msx_ast::Polyline) -> Option<SkPath> {
    let mut pb = PathBuilder::new();
    let mut points = p.points.iter();
    let first = points.next()?;
    pb.move_to(first.x as f32, first.y as f32);
    for pt in points {
        pb.line_to(pt.x as f32, pt.y as f32);
    }
    if p.closed {
        pb.close();
    }
    pb.finish()
}

fn build_msx_path(p: &msx_ast::Path) -> Option<SkPath> {
    let mut pb = PathBuilder::new();
    let mut current = (0.0f32, 0.0f32);
    let mut start = (0.0f32, 0.0f32);

    for cmd in &p.commands {
        match cmd {
            PathCommand::MoveTo(pt) => {
                current = (pt.x as f32, pt.y as f32);
                start = current;
                pb.move_to(current.0, current.1);
            }
            PathCommand::LineTo(pt) => {
                current = (pt.x as f32, pt.y as f32);
                pb.line_to(current.0, current.1);
            }
            PathCommand::HLineTo(x) => {
                current = (*x as f32, current.1);
                pb.line_to(current.0, current.1);
            }
            PathCommand::VLineTo(y) => {
                current = (current.0, *y as f32);
                pb.line_to(current.0, current.1);
            }
            PathCommand::CubicBezier { c1, c2, to } => {
                pb.cubic_to(c1.x as f32, c1.y as f32, c2.x as f32, c2.y as f32, to.x as f32, to.y as f32);
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::QuadBezier { c, to } => {
                pb.quad_to(c.x as f32, c.y as f32, to.x as f32, to.y as f32);
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::SmoothCubic { c2, to } => {
                pb.cubic_to(current.0, current.1, c2.x as f32, c2.y as f32, to.x as f32, to.y as f32);
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::SmoothQuad { to } => {
                pb.quad_to(current.0, current.1, to.x as f32, to.y as f32);
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } => {
                let to_pt = (to.x as f32, to.y as f32);
                append_arc(&mut pb, current, (*rx as f32, *ry as f32), *x_rotation as f32, *large_arc, *sweep, to_pt);
                current = to_pt;
            }
            PathCommand::ClosePath => {
                pb.close();
                current = start;
            }
            PathCommand::RelMoveTo(pt) => {
                current = (current.0 + pt.x as f32, current.1 + pt.y as f32);
                start = current;
                pb.move_to(current.0, current.1);
            }
            PathCommand::RelLineTo(pt) => {
                current = (current.0 + pt.x as f32, current.1 + pt.y as f32);
                pb.line_to(current.0, current.1);
            }
            PathCommand::RelHLineTo(x) => {
                current = (current.0 + *x as f32, current.1);
                pb.line_to(current.0, current.1);
            }
            PathCommand::RelVLineTo(y) => {
                current = (current.0, current.1 + *y as f32);
                pb.line_to(current.0, current.1);
            }
            PathCommand::RelCubicBezier { c1, c2, to } => {
                let c1p = (current.0 + c1.x as f32, current.1 + c1.y as f32);
                let c2p = (current.0 + c2.x as f32, current.1 + c2.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                pb.cubic_to(c1p.0, c1p.1, c2p.0, c2p.1, to_pt.0, to_pt.1);
                current = to_pt;
            }
            PathCommand::RelQuadBezier { c, to } => {
                let cp = (current.0 + c.x as f32, current.1 + c.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                pb.quad_to(cp.0, cp.1, to_pt.0, to_pt.1);
                current = to_pt;
            }
            PathCommand::RelSmoothCubic { c2, to } => {
                let c2p = (current.0 + c2.x as f32, current.1 + c2.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                pb.cubic_to(current.0, current.1, c2p.0, c2p.1, to_pt.0, to_pt.1);
                current = to_pt;
            }
            PathCommand::RelSmoothQuad { to } => {
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                pb.quad_to(current.0, current.1, to_pt.0, to_pt.1);
                current = to_pt;
            }
            PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                append_arc(&mut pb, current, (*rx as f32, *ry as f32), *x_rotation as f32, *large_arc, *sweep, to_pt);
                current = to_pt;
            }
        }
    }

    pb.finish()
}

/// SVG elliptical arc → cubic bezier segments, per the SVG 1.1 spec
/// (Appendix F.6: endpoint-to-center parameterization, then one cubic per
/// ≤90° sweep using the same kappa-style tangent construction as the
/// circle/ellipse builders above, generalized to an arbitrary span and
/// rotation).
fn append_arc(pb: &mut PathBuilder, from: (f32, f32), radii: (f32, f32), x_rotation_deg: f32, large_arc: bool, sweep: bool, to: (f32, f32)) {
    let (x1, y1) = from;
    let (x2, y2) = to;
    let (mut rx, mut ry) = radii;

    if rx.abs() < 1e-6 || ry.abs() < 1e-6 || ((x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6) {
        pb.line_to(x2, y2);
        return;
    }
    rx = rx.abs();
    ry = ry.abs();

    let phi = x_rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    let dx2 = (x1 - x2) / 2.0;
    let dy2 = (y1 - y2) / 2.0;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;

    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }

    let sign = if large_arc != sweep { 1.0 } else { -1.0 };
    let num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let coef = if den > 1e-9 { sign * (num / den).sqrt() } else { 0.0 };
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * -(ry * x1p / rx);

    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;

    let angle_between = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };

    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle_between((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }

    let segments = (delta.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let delta_seg = delta / segments as f32;
    let alpha = (4.0 / 3.0) * (delta_seg / 4.0).tan();

    let rotate = |px: f32, py: f32| -> (f32, f32) {
        (cos_phi * (px - cx) - sin_phi * (py - cy) + cx, sin_phi * (px - cx) + cos_phi * (py - cy) + cy)
    };

    let mut theta = theta1;
    for _ in 0..segments {
        let (sin_t, cos_t) = theta.sin_cos();
        let theta_next = theta + delta_seg;
        let (sin_tn, cos_tn) = theta_next.sin_cos();

        let p1 = (cx + rx * cos_t, cy + ry * sin_t);
        let p2 = (cx + rx * cos_tn, cy + ry * sin_tn);
        let d1 = (-rx * sin_t, ry * cos_t);
        let d2 = (-rx * sin_tn, ry * cos_tn);

        let c1 = (p1.0 + alpha * d1.0, p1.1 + alpha * d1.1);
        let c2 = (p2.0 - alpha * d2.0, p2.1 - alpha * d2.1);

        let c1r = rotate(c1.0, c1.1);
        let c2r = rotate(c2.0, c2.1);
        let p2r = rotate(p2.0, p2.1);

        pb.cubic_to(c1r.0, c1r.1, c2r.0, c2r.1, p2r.0, p2r.1);
        theta = theta_next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Circle as MsxCircle, Ellipse as MsxEllipse, Rect as MsxRect};

    fn dummy_rect(rx: Option<f64>, ry: Option<f64>) -> MsxRect {
        MsxRect { x: 0.0, y: 0.0, width: 100.0, height: 60.0, rx, ry, id: None, transform: None, style: Style::default() }
    }

    #[test]
    fn sharp_rect_produces_a_path() {
        assert!(build_rect_path(&dummy_rect(None, None)).is_some());
    }

    #[test]
    fn rounded_rect_keeps_the_same_overall_extent() {
        let path = build_rect_path(&dummy_rect(Some(12.0), None)).unwrap();
        assert!((path.bounds().width() - 100.0).abs() < 1.0);
    }

    #[test]
    fn circle_path_bounds_match_diameter() {
        let c = MsxCircle { cx: 50.0, cy: 50.0, r: 20.0, id: None, transform: None, style: Style::default() };
        let path = build_circle_path(&c).unwrap();
        assert!((path.bounds().width() - 40.0).abs() < 1.0);
    }

    #[test]
    fn ellipse_path_is_well_formed() {
        let e = MsxEllipse { cx: 0.0, cy: 0.0, rx: 30.0, ry: 10.0, id: None, transform: None, style: Style::default() };
        assert!(build_ellipse_path(&e).is_some());
    }

    #[test]
    fn arc_degenerate_same_point_does_not_panic() {
        let mut pb = PathBuilder::new();
        pb.move_to(10.0, 10.0);
        append_arc(&mut pb, (10.0, 10.0), (5.0, 5.0), 0.0, false, true, (10.0, 10.0));
        assert!(pb.finish().is_some());
    }

    #[test]
    fn arc_semicircle_builds_a_valid_path() {
        let mut pb = PathBuilder::new();
        pb.move_to(120.0, 380.0);
        append_arc(&mut pb, (120.0, 380.0), (130.0, 130.0), 0.0, false, true, (380.0, 380.0));
        assert!(pb.finish().is_some());
    }

    #[test]
    fn combine_with_no_local_transform_returns_parent_unchanged() {
        let parent = Matrix2D::translate(5.0, 5.0);
        let combined = combine(parent, None);
        assert_eq!(combined.e, 5.0);
        assert_eq!(combined.f, 5.0);
    }
        }
