// core/msx-svg-import/src/geom.rs
//! Geometry the importer needs to convert gradients: matrix inversion and
//! the exact bounding box of a path, including arcs and curve extrema.

use msx_ast::{Element, Matrix2D, Paint, PathCommand, Point, Style, Transform};

/// Axis-aligned box, `x`/`y` is the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl BBox {
    pub fn from_corners(x0: f64, y0: f64, x1: f64, y1: f64) -> BBox {
        BBox { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    /// A gradient in bounding-box units needs a box with area.
    pub fn has_area(&self) -> bool {
        self.w > 0.0 && self.h > 0.0 && self.w.is_finite() && self.h.is_finite()
    }

    /// The matrix that maps the unit square onto this box.
    pub fn unit_to_box(&self) -> Matrix2D {
        Matrix2D { a: self.w, b: 0.0, c: 0.0, d: self.h, e: self.x, f: self.y }
    }
}

pub fn invert(m: Matrix2D) -> Option<Matrix2D> {
    let det = m.a * m.d - m.b * m.c;
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    let (a, b, c, d) = (m.d / det, -m.b / det, -m.c / det, m.a / det);
    Some(Matrix2D { a, b, c, d, e: -(a * m.e + c * m.f), f: -(b * m.e + d * m.f) })
}

/// `outer` applied after `inner`, so `mul(outer, inner)(p) == outer(inner(p))`.
pub fn mul(outer: Matrix2D, inner: Matrix2D) -> Matrix2D {
    outer.concat(inner)
}

pub fn apply(m: Matrix2D, x: f64, y: f64) -> (f64, f64) {
    (m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f)
}

/// Running min and max over points.
#[derive(Default)]
struct Extent {
    min: Option<(f64, f64)>,
    max: Option<(f64, f64)>,
}

impl Extent {
    fn add(&mut self, x: f64, y: f64) {
        let (mn, mx) = (self.min.get_or_insert((x, y)), self.max.get_or_insert((x, y)));
        mn.0 = mn.0.min(x);
        mn.1 = mn.1.min(y);
        mx.0 = mx.0.max(x);
        mx.1 = mx.1.max(y);
    }

    fn cubic(&mut self, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
        self.add(p3.0, p3.1);
        for axis in 0..2 {
            let (a0, a1, a2, a3) = if axis == 0 { (p0.0, p1.0, p2.0, p3.0) } else { (p0.1, p1.1, p2.1, p3.1) };
            // Derivative of the cubic is a quadratic: a t^2 + b t + c.
            let a = -a0 + 3.0 * a1 - 3.0 * a2 + a3;
            let b = 2.0 * (a0 - 2.0 * a1 + a2);
            let c = a1 - a0;
            for t in quadratic_roots(a, b, c) {
                if t > 0.0 && t < 1.0 {
                    let mt = 1.0 - t;
                    let (x, y) = (
                        mt * mt * mt * p0.0 + 3.0 * mt * mt * t * p1.0 + 3.0 * mt * t * t * p2.0 + t * t * t * p3.0,
                        mt * mt * mt * p0.1 + 3.0 * mt * mt * t * p1.1 + 3.0 * mt * t * t * p2.1 + t * t * t * p3.1,
                    );
                    self.add(x, y);
                }
            }
        }
    }

    fn quad(&mut self, p0: (f64, f64), c: (f64, f64), p2: (f64, f64)) {
        // Exact: lift to the equivalent cubic.
        let c1 = (p0.0 + 2.0 / 3.0 * (c.0 - p0.0), p0.1 + 2.0 / 3.0 * (c.1 - p0.1));
        let c2 = (p2.0 + 2.0 / 3.0 * (c.0 - p2.0), p2.1 + 2.0 / 3.0 * (c.1 - p2.1));
        self.cubic(p0, c1, c2, p2);
    }
}

fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < 1e-12 {
        return if b.abs() < 1e-12 { vec![] } else { vec![-c / b] };
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return vec![];
    }
    let s = disc.sqrt();
    vec![(-b + s) / (2.0 * a), (-b - s) / (2.0 * a)]
}

/// Convert an SVG elliptical arc to cubic segments `[c1, c2, end]` (each
/// start point is the previous end). Implements the endpoint to center
/// conversion of the SVG spec, split into pieces of at most 90 degrees.
#[allow(clippy::too_many_arguments)]
pub fn arc_to_cubics(x1: f64, y1: f64, rx: f64, ry: f64, x_rot_deg: f64, large: bool, sweep: bool, x2: f64, y2: f64) -> Vec<[(f64, f64); 3]> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if (x1 == x2 && y1 == y2) || rx == 0.0 || ry == 0.0 {
        return vec![[(x1, y1), (x2, y2), (x2, y2)]];
    }
    let phi = x_rot_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (x1p, y1p) = (cp * dx + sp * dy, -sp * dx + cp * dy);

    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut k = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if large == sweep {
        k = -k;
    }
    let (cxp, cyp) = (k * rx * y1p / ry, -k * ry * x1p / rx);
    let (cx, cy) = (cp * cxp - sp * cyp + (x1 + x2) / 2.0, sp * cxp + cp * cyp + (y1 + y2) / 2.0);

    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }

    let n = (dtheta.abs() / std::f64::consts::FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
    let step = dtheta / n as f64;
    let t = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |theta: f64| {
        let (s, c) = theta.sin_cos();
        (cx + rx * c * cp - ry * s * sp, cy + rx * c * sp + ry * s * cp)
    };
    let deriv = |theta: f64| {
        let (s, c) = theta.sin_cos();
        (-rx * s * cp - ry * c * sp, -rx * s * sp + ry * c * cp)
    };

    let mut out = Vec::with_capacity(n);
    let mut a = theta1;
    for i in 0..n {
        let b = a + step;
        let (p0, p3) = (point(a), if i + 1 == n { (x2, y2) } else { point(b) });
        let (d0, d3) = (deriv(a), deriv(b));
        out.push([(p0.0 + t * d0.0, p0.1 + t * d0.1), (p3.0 - t * d3.0, p3.1 - t * d3.1), p3]);
        a = b;
    }
    out
}

/// Exact bounding box of a path, or `None` for an empty one.
/// Bounding box of the geometry of `els` under `m`, ignoring stroke width.
/// Text, images, sdf and splat elements have no simple geometry and are
/// skipped, so `None` also means "nothing measurable".
pub fn elements_bbox(els: &[Element], m: Matrix2D) -> Option<BBox> {
    let mut ext = Extent::default();
    collect_extent(els, m, &mut ext, false);
    let (mn, mx) = (ext.min?, ext.max?);
    Some(BBox::from_corners(mn.0, mn.1, mx.0, mx.1))
}

/// True when drawing `els` reaches outside the viewport `(x0, y0, x1, y1)`, or
/// when that cannot be measured (text, images, sdf, splats and `use` have no
/// simple extent, so the answer is a cautious yes). Half the stroke width
/// counts, so an outline poking over the edge is seen.
pub fn reaches_outside(els: &[Element], (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
    const SLACK: f64 = 1e-3;
    let mut ext = Extent::default();
    if !collect_extent(els, Matrix2D::identity(), &mut ext, true) {
        return true;
    }
    match (ext.min, ext.max) {
        (Some(mn), Some(mx)) => mn.0 < x0 - SLACK || mn.1 < y0 - SLACK || mx.0 > x1 + SLACK || mx.1 > y1 + SLACK,
        _ => false,
    }
}

fn local_matrix(outer: Matrix2D, t: &Option<Transform>) -> Matrix2D {
    match t {
        Some(t) => mul(outer, t.to_matrix()),
        None => outer,
    }
}

/// Half the visible stroke width of `style` in the space `m` maps to, or zero.
fn stroke_pad(style: &Style, m: Matrix2D, on: bool) -> f64 {
    match (&style.stroke, style.stroke_width.unwrap_or(1.0)) {
        (Some(p), w) if on && *p != Paint::None && w > 0.0 => w / 2.0 * m.a.hypot(m.b).max(m.c.hypot(m.d)),
        _ => 0.0,
    }
}

/// Merges one leaf, padded by `pad`, into `ext`.
fn merge_padded(ext: &mut Extent, leaf: Extent, pad: f64) {
    if let (Some(mn), Some(mx)) = (leaf.min, leaf.max) {
        ext.add(mn.0 - pad, mn.1 - pad);
        ext.add(mx.0 + pad, mx.1 + pad);
    }
}

/// Adds the four corners of the box `(x0, y0, x1, y1)` mapped through `m`.
fn add_corners(ext: &mut Extent, m: Matrix2D, (x0, y0, x1, y1): (f64, f64, f64, f64)) {
    for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
        let (px, py) = apply(m, x, y);
        ext.add(px, py);
    }
}

/// An ellipse maps to an ellipse, whose extent along each axis is exact.
fn add_ellipse(ext: &mut Extent, m: Matrix2D, cx: f64, cy: f64, rx: f64, ry: f64) {
    let (px, py) = apply(m, cx, cy);
    let hx = ((m.a * rx).powi(2) + (m.c * ry).powi(2)).sqrt();
    let hy = ((m.b * rx).powi(2) + (m.d * ry).powi(2)).sqrt();
    ext.add(px - hx, py - hy);
    ext.add(px + hx, py + hy);
}

/// Collects the extent of `els`. Returns false when something without a
/// simple extent was met.
fn collect_extent(els: &[Element], outer: Matrix2D, ext: &mut Extent, stroke: bool) -> bool {
    let mut measurable = true;
    for el in els {
        let mut leaf = Extent::default();
        let pad = match el {
            Element::Rect(r) => {
                let m = local_matrix(outer, &r.transform);
                add_corners(&mut leaf, m, (r.x, r.y, r.x + r.width, r.y + r.height));
                stroke_pad(&r.style, m, stroke)
            }
            Element::Circle(c) => {
                let m = local_matrix(outer, &c.transform);
                add_ellipse(&mut leaf, m, c.cx, c.cy, c.r, c.r);
                stroke_pad(&c.style, m, stroke)
            }
            Element::Ellipse(e) => {
                let m = local_matrix(outer, &e.transform);
                add_ellipse(&mut leaf, m, e.cx, e.cy, e.rx, e.ry);
                stroke_pad(&e.style, m, stroke)
            }
            Element::Line(l) => {
                let m = local_matrix(outer, &l.transform);
                for (x, y) in [(l.x1, l.y1), (l.x2, l.y2)] {
                    let (px, py) = apply(m, x, y);
                    leaf.add(px, py);
                }
                stroke_pad(&l.style, m, stroke)
            }
            Element::Polyline(p) | Element::Polygon(p) => {
                let m = local_matrix(outer, &p.transform);
                for pt in &p.points {
                    let (px, py) = apply(m, pt.x, pt.y);
                    leaf.add(px, py);
                }
                stroke_pad(&p.style, m, stroke)
            }
            Element::Path(p) => {
                let m = local_matrix(outer, &p.transform);
                if let Some(b) = path_bbox(&p.commands) {
                    add_corners(&mut leaf, m, (b.x, b.y, b.x + b.w, b.y + b.h));
                }
                stroke_pad(&p.style, m, stroke)
            }
            Element::Group(g) => {
                measurable &= collect_extent(&g.children, local_matrix(outer, &g.transform), ext, stroke);
                continue;
            }
            Element::Layer(l) => {
                measurable &= collect_extent(&l.children, local_matrix(outer, &l.transform), ext, stroke);
                continue;
            }
            _ => {
                measurable = false;
                continue;
            }
        };
        merge_padded(ext, leaf, pad);
    }
    measurable
}

pub fn path_bbox(cmds: &[PathCommand]) -> Option<BBox> {
    let mut ext = Extent::default();
    let (mut cur, mut start) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
    // Last cubic and quadratic control points, for S and T reflection.
    let mut last_c: Option<(f64, f64)> = None;
    let mut last_q: Option<(f64, f64)> = None;
    let abs = |rel: bool, cur: (f64, f64), p: &Point| if rel { (cur.0 + p.x, cur.1 + p.y) } else { (p.x, p.y) };
    let reflect = |ctl: Option<(f64, f64)>, cur: (f64, f64)| ctl.map_or(cur, |c| (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1));

    for cmd in cmds {
        let mut new_c = None;
        let mut new_q = None;
        match cmd {
            PathCommand::MoveTo(p) | PathCommand::RelMoveTo(p) => {
                cur = abs(matches!(cmd, PathCommand::RelMoveTo(_)), cur, p);
                start = cur;
                ext.add(cur.0, cur.1);
            }
            PathCommand::LineTo(p) | PathCommand::RelLineTo(p) => {
                cur = abs(matches!(cmd, PathCommand::RelLineTo(_)), cur, p);
                ext.add(cur.0, cur.1);
            }
            PathCommand::HLineTo(x) => {
                cur.0 = *x;
                ext.add(cur.0, cur.1);
            }
            PathCommand::RelHLineTo(x) => {
                cur.0 += x;
                ext.add(cur.0, cur.1);
            }
            PathCommand::VLineTo(y) => {
                cur.1 = *y;
                ext.add(cur.0, cur.1);
            }
            PathCommand::RelVLineTo(y) => {
                cur.1 += y;
                ext.add(cur.0, cur.1);
            }
            PathCommand::CubicBezier { c1, c2, to } | PathCommand::RelCubicBezier { c1, c2, to } => {
                let rel = matches!(cmd, PathCommand::RelCubicBezier { .. });
                let (a, b, e) = (abs(rel, cur, c1), abs(rel, cur, c2), abs(rel, cur, to));
                ext.cubic(cur, a, b, e);
                new_c = Some(b);
                cur = e;
            }
            PathCommand::SmoothCubic { c2, to } | PathCommand::RelSmoothCubic { c2, to } => {
                let rel = matches!(cmd, PathCommand::RelSmoothCubic { .. });
                let a = reflect(last_c, cur);
                let (b, e) = (abs(rel, cur, c2), abs(rel, cur, to));
                ext.cubic(cur, a, b, e);
                new_c = Some(b);
                cur = e;
            }
            PathCommand::QuadBezier { c, to } | PathCommand::RelQuadBezier { c, to } => {
                let rel = matches!(cmd, PathCommand::RelQuadBezier { .. });
                let (a, e) = (abs(rel, cur, c), abs(rel, cur, to));
                ext.quad(cur, a, e);
                new_q = Some(a);
                cur = e;
            }
            PathCommand::SmoothQuad { to } | PathCommand::RelSmoothQuad { to } => {
                let rel = matches!(cmd, PathCommand::RelSmoothQuad { .. });
                let a = reflect(last_q, cur);
                let e = abs(rel, cur, to);
                ext.quad(cur, a, e);
                new_q = Some(a);
                cur = e;
            }
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } | PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
                let rel = matches!(cmd, PathCommand::RelArc { .. });
                let e = abs(rel, cur, to);
                let mut from = cur;
                for [c1, c2, end] in arc_to_cubics(cur.0, cur.1, *rx, *ry, *x_rotation, *large_arc, *sweep, e.0, e.1) {
                    ext.cubic(from, c1, c2, end);
                    from = end;
                }
                cur = e;
            }
            PathCommand::ClosePath => cur = start,
        }
        last_c = new_c;
        last_q = new_q;
    }
    match (ext.min, ext.max) {
        (Some(mn), Some(mx)) => Some(BBox::from_corners(mn.0, mn.1, mx.0, mx.1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::path::parse_d;

    fn bbox(d: &str) -> BBox {
        path_bbox(&parse_d(d).unwrap()).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn lines_and_relative_moves() {
        let b = bbox("M 10 20 L 50 20 L 50 80 Z");
        assert_eq!((b.x, b.y, b.w, b.h), (10.0, 20.0, 40.0, 60.0));
        let r = bbox("m 5 5 h 10 v 20 l -3 3");
        assert_eq!((r.x, r.y, r.w, r.h), (5.0, 5.0, 10.0, 23.0));
    }

    #[test]
    fn cubic_extrema_are_found_not_just_control_points() {
        // A bump whose control points sit at y = -40 but whose curve peaks at -30.
        let b = bbox("M 0 0 C 0 -40 100 -40 100 0");
        assert!(close(b.y, -30.0), "top should be -30, got {}", b.y);
        assert!(close(b.h, 30.0));
    }

    #[test]
    fn quadratic_extrema() {
        let b = bbox("M 0 0 Q 50 -40 100 0");
        assert!(close(b.y, -20.0), "got {}", b.y);
    }

    #[test]
    fn a_full_circle_made_of_arcs_has_the_exact_box() {
        let b = bbox("M 10 50 A 40 40 0 1 0 90 50 A 40 40 0 1 0 10 50 Z");
        assert!(close(b.x, 10.0) && close(b.w, 80.0), "x {} w {}", b.x, b.w);
        assert!(close(b.y, 10.0) && close(b.h, 80.0), "y {} h {}", b.y, b.h);
    }

    #[test]
    fn a_half_circle_arc() {
        let b = bbox("M 0 0 A 50 50 0 0 1 100 0");
        assert!(close(b.y, -50.0) && close(b.h, 50.0), "y {} h {}", b.y, b.h);
    }

    #[test]
    fn an_arc_with_too_small_radii_is_scaled_up() {
        let b = bbox("M 0 0 A 1 1 0 0 1 100 0");
        assert!(close(b.y, -50.0), "got {}", b.y);
    }

    #[test]
    fn smooth_curves_reflect_the_previous_control_point() {
        let smooth = bbox("M 0 0 C 0 -40 50 -40 50 0 S 100 40 100 0");
        let explicit = bbox("M 0 0 C 0 -40 50 -40 50 0 C 50 40 100 40 100 0");
        assert!(close(smooth.y, explicit.y) && close(smooth.h, explicit.h));
        let tq = bbox("M 0 0 Q 25 -40 50 0 T 100 0");
        let tq_explicit = bbox("M 0 0 Q 25 -40 50 0 Q 75 40 100 0");
        assert!(close(tq.y, tq_explicit.y) && close(tq.h, tq_explicit.h));
    }

    #[test]
    fn empty_path_has_no_box() {
        assert!(path_bbox(&[]).is_none());
    }

    #[test]
    fn matrix_inverse_undoes_the_matrix() {
        let m = Matrix2D { a: 2.0, b: 0.5, c: -0.25, d: 3.0, e: 10.0, f: -4.0 };
        let inv = invert(m).unwrap();
        let (x, y) = apply(m, 3.0, 7.0);
        let (bx, by) = apply(inv, x, y);
        assert!(close(bx, 3.0) && close(by, 7.0));
        assert!(invert(Matrix2D { a: 1.0, b: 2.0, c: 2.0, d: 4.0, e: 0.0, f: 0.0 }).is_none());
    }

    #[test]
    fn mul_applies_the_inner_matrix_first() {
        let outer = Matrix2D::translate(10.0, 0.0);
        let inner = Matrix2D::scale(2.0, 2.0);
        let (x, y) = apply(mul(outer, inner), 1.0, 1.0);
        assert_eq!((x, y), (12.0, 2.0));
    }
}
