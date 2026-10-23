// core/msx-svg-import/src/markers.rs
//! Marker placement: where the vertices of a path are and which way a marker
//! faces at each. The importer turns every vertex into a copy of the marker
//! content, so this module only does the geometry.

use msx_ast::{PathCommand, Point};

use crate::geom::arc_to_cubics;

/// One vertex of a shape and the direction a marker with `orient="auto"`
/// takes there, in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vertex {
    pub x: f64,
    pub y: f64,
    pub angle: f64,
}

/// How a marker is turned at a vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Orient {
    Auto,
    AutoStartReverse,
    /// A fixed angle in radians.
    Angle(f64),
}

/// Parses `orient`. An angle takes `deg`, `grad`, `rad` or `turn`, and a plain
/// number is degrees. Anything unreadable is a fixed angle of zero.
pub(crate) fn parse_orient(v: &str) -> Orient {
    let v = v.trim();
    match v {
        "auto" => return Orient::Auto,
        "auto-start-reverse" => return Orient::AutoStartReverse,
        _ => {}
    }
    let split = v.find(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E').unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let Ok(n) = num.trim().parse::<f64>() else { return Orient::Angle(0.0) };
    let radians = match unit.trim() {
        "" | "deg" => n.to_radians(),
        "grad" => n * std::f64::consts::PI / 200.0,
        "rad" => n,
        "turn" => n * std::f64::consts::TAU,
        _ => return Orient::Angle(0.0),
    };
    Orient::Angle(radians)
}

/// A segment as a cubic. A line has both control points on its ends, so the
/// same tangent rules cover every kind of segment.
#[derive(Debug, Clone, Copy)]
struct Seg {
    from: (f64, f64),
    c1: (f64, f64),
    c2: (f64, f64),
    to: (f64, f64),
    /// Whether a vertex sits at the end. An arc is cut into several cubics but
    /// is one segment, so only its last piece ends in a vertex.
    vertex_after: bool,
}

impl Seg {
    fn line(from: (f64, f64), to: (f64, f64)) -> Seg {
        Seg { from, c1: from, c2: to, to, vertex_after: true }
    }

    fn quad(from: (f64, f64), c: (f64, f64), to: (f64, f64)) -> Seg {
        let third = |a: (f64, f64), b: (f64, f64)| (a.0 + 2.0 / 3.0 * (b.0 - a.0), a.1 + 2.0 / 3.0 * (b.1 - a.1));
        Seg { from, c1: third(from, c), c2: third(to, c), to, vertex_after: true }
    }

    /// Direction the segment leaves its start in, skipping coincident control points.
    fn start_dir(&self) -> Option<(f64, f64)> {
        [self.c1, self.c2, self.to].into_iter().map(|p| (p.0 - self.from.0, p.1 - self.from.1)).find(|d| d.0.hypot(d.1) > 1e-9)
    }

    /// Direction the segment arrives in.
    fn end_dir(&self) -> Option<(f64, f64)> {
        [self.c2, self.c1, self.from].into_iter().map(|p| (self.to.0 - p.0, self.to.1 - p.1)).find(|d| d.0.hypot(d.1) > 1e-9)
    }
}

struct Sub {
    start: (f64, f64),
    segs: Vec<Seg>,
    closed: bool,
}

/// Splits `cmds` into subpaths of absolute segments. A closepath adds a
/// segment back to the start, even a zero length one, so that it has a vertex
/// of its own.
fn subpaths(cmds: &[PathCommand]) -> Vec<Sub> {
    let mut subs: Vec<Sub> = Vec::new();
    let (mut cur, mut start) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
    let mut open = false;
    let mut last_c: Option<(f64, f64)> = None;
    let mut last_q: Option<(f64, f64)> = None;
    let abs = |rel: bool, cur: (f64, f64), p: &Point| if rel { (cur.0 + p.x, cur.1 + p.y) } else { (p.x, p.y) };
    let reflect = |ctl: Option<(f64, f64)>, cur: (f64, f64)| ctl.map_or(cur, |c| (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1));

    for cmd in cmds {
        let mut new_c = None;
        let mut new_q = None;
        let mut segs: Vec<Seg> = Vec::new();
        match cmd {
            PathCommand::MoveTo(p) | PathCommand::RelMoveTo(p) => {
                cur = abs(matches!(cmd, PathCommand::RelMoveTo(_)), cur, p);
                start = cur;
                subs.push(Sub { start, segs: Vec::new(), closed: false });
                open = true;
            }
            PathCommand::LineTo(p) | PathCommand::RelLineTo(p) => {
                let to = abs(matches!(cmd, PathCommand::RelLineTo(_)), cur, p);
                segs.push(Seg::line(cur, to));
            }
            PathCommand::HLineTo(x) => segs.push(Seg::line(cur, (*x, cur.1))),
            PathCommand::RelHLineTo(x) => segs.push(Seg::line(cur, (cur.0 + x, cur.1))),
            PathCommand::VLineTo(y) => segs.push(Seg::line(cur, (cur.0, *y))),
            PathCommand::RelVLineTo(y) => segs.push(Seg::line(cur, (cur.0, cur.1 + y))),
            PathCommand::CubicBezier { c1, c2, to } | PathCommand::RelCubicBezier { c1, c2, to } => {
                let rel = matches!(cmd, PathCommand::RelCubicBezier { .. });
                let (a, b, e) = (abs(rel, cur, c1), abs(rel, cur, c2), abs(rel, cur, to));
                segs.push(Seg { from: cur, c1: a, c2: b, to: e, vertex_after: true });
                new_c = Some(b);
            }
            PathCommand::SmoothCubic { c2, to } | PathCommand::RelSmoothCubic { c2, to } => {
                let rel = matches!(cmd, PathCommand::RelSmoothCubic { .. });
                let a = reflect(last_c, cur);
                let (b, e) = (abs(rel, cur, c2), abs(rel, cur, to));
                segs.push(Seg { from: cur, c1: a, c2: b, to: e, vertex_after: true });
                new_c = Some(b);
            }
            PathCommand::QuadBezier { c, to } | PathCommand::RelQuadBezier { c, to } => {
                let rel = matches!(cmd, PathCommand::RelQuadBezier { .. });
                let (a, e) = (abs(rel, cur, c), abs(rel, cur, to));
                segs.push(Seg::quad(cur, a, e));
                new_q = Some(a);
            }
            PathCommand::SmoothQuad { to } | PathCommand::RelSmoothQuad { to } => {
                let rel = matches!(cmd, PathCommand::RelSmoothQuad { .. });
                let a = reflect(last_q, cur);
                let e = abs(rel, cur, to);
                segs.push(Seg::quad(cur, a, e));
                new_q = Some(a);
            }
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } | PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
                let rel = matches!(cmd, PathCommand::RelArc { .. });
                let e = abs(rel, cur, to);
                let mut from = cur;
                let pieces = arc_to_cubics(cur.0, cur.1, *rx, *ry, *x_rotation, *large_arc, *sweep, e.0, e.1);
                let count = pieces.len();
                for (i, [c1, c2, end]) in pieces.into_iter().enumerate() {
                    segs.push(Seg { from, c1, c2, to: end, vertex_after: i + 1 == count });
                    from = end;
                }
            }
            PathCommand::ClosePath => {
                if open {
                    segs.push(Seg::line(cur, start));
                    if let Some(sub) = subs.last_mut() {
                        sub.segs.append(&mut segs);
                        sub.closed = true;
                    }
                    cur = start;
                    open = false;
                } else {
                    // A closepath with nothing open adds one more vertex at the start.
                    subs.push(Sub { start, segs: vec![Seg::line(start, start)], closed: true });
                }
            }
        }
        if !segs.is_empty() {
            if !open {
                subs.push(Sub { start: cur, segs: Vec::new(), closed: false });
                open = true;
                start = cur;
            }
            cur = segs.last().map_or(cur, |s| s.to);
            if let Some(sub) = subs.last_mut() {
                sub.segs.append(&mut segs);
            }
        }
        last_c = new_c;
        last_q = new_q;
    }
    subs
}

/// The angle halfway between the arriving and the leaving direction, going the
/// short way round. With only one of them it is that direction, with neither zero.
fn bisect(dir_in: Option<(f64, f64)>, dir_out: Option<(f64, f64)>) -> f64 {
    let angle = |d: (f64, f64)| d.1.atan2(d.0);
    match (dir_in, dir_out) {
        (Some(a), Some(b)) => {
            let (a1, a2) = (angle(a), angle(b));
            let mut d = a2 - a1;
            while d > std::f64::consts::PI {
                d -= std::f64::consts::TAU;
            }
            while d <= -std::f64::consts::PI {
                d += std::f64::consts::TAU;
            }
            a1 + d / 2.0
        }
        (Some(d), None) | (None, Some(d)) => angle(d),
        (None, None) => 0.0,
    }
}

fn sub_vertices(sub: &Sub) -> Vec<Vertex> {
    let n = sub.segs.len();
    let back = |k: usize| sub.segs[..=k].iter().rev().find_map(Seg::end_dir);
    let forward = |k: usize| sub.segs[k..].iter().find_map(Seg::start_dir);
    // The first vertex faces along the first segment and the last along the
    // last one, also for a closed subpath: nothing is bisected across the
    // closure, which is how resvg draws it. The vertex just before a closepath
    // does bisect with the closing segment.
    let first_out = if n > 0 { forward(0) } else { None };
    let mut out = vec![Vertex { x: sub.start.0, y: sub.start.1, angle: bisect(None, first_out) }];
    for (k, seg) in sub.segs.iter().enumerate().filter(|(_, s)| s.vertex_after) {
        let dir_out = if k + 1 < n { forward(k + 1) } else { None };
        out.push(Vertex { x: seg.to.0, y: seg.to.1, angle: bisect(back(k), dir_out) });
    }
    out
}

/// The outline of a rectangle as path commands, the way SVG 2 defines it for
/// markers: corners for a plain one, and an arc at each corner for a rounded one.
pub(crate) fn rect_commands(x: f64, y: f64, w: f64, h: f64, rx: Option<f64>, ry: Option<f64>) -> Vec<PathCommand> {
    let (rx, ry) = (rx.unwrap_or(0.0), ry.unwrap_or(0.0));
    let p = |x: f64, y: f64| Point::new(x, y);
    if rx <= 0.0 || ry <= 0.0 {
        return vec![
            PathCommand::MoveTo(p(x, y)),
            PathCommand::LineTo(p(x + w, y)),
            PathCommand::LineTo(p(x + w, y + h)),
            PathCommand::LineTo(p(x, y + h)),
            PathCommand::ClosePath,
        ];
    }
    let arc = |to: Point| PathCommand::Arc { rx, ry, x_rotation: 0.0, large_arc: false, sweep: true, to };
    vec![
        PathCommand::MoveTo(p(x + rx, y)),
        PathCommand::LineTo(p(x + w - rx, y)),
        arc(p(x + w, y + ry)),
        PathCommand::LineTo(p(x + w, y + h - ry)),
        arc(p(x + w - rx, y + h)),
        PathCommand::LineTo(p(x + rx, y + h)),
        arc(p(x, y + h - ry)),
        PathCommand::LineTo(p(x, y + ry)),
        arc(p(x + rx, y)),
        PathCommand::ClosePath,
    ]
}

/// The outline of an ellipse as path commands: four arcs from the rightmost
/// point, clockwise.
pub(crate) fn ellipse_commands(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathCommand> {
    let p = |x: f64, y: f64| Point::new(x, y);
    let arc = |to: Point| PathCommand::Arc { rx, ry, x_rotation: 0.0, large_arc: false, sweep: true, to };
    vec![
        PathCommand::MoveTo(p(cx + rx, cy)),
        arc(p(cx, cy + ry)),
        arc(p(cx - rx, cy)),
        arc(p(cx, cy - ry)),
        arc(p(cx + rx, cy)),
        PathCommand::ClosePath,
    ]
}

/// Every vertex of the path in order, with its marker direction.
pub(crate) fn vertices(cmds: &[PathCommand]) -> Vec<Vertex> {
    subpaths(cmds).iter().flat_map(sub_vertices).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::path::parse_d;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn v(d: &str) -> Vec<Vertex> {
        vertices(&parse_d(d).unwrap())
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_line_has_two_vertices_facing_along_it() {
        let vs = v("M0 0 L10 10");
        assert_eq!(vs.len(), 2);
        assert!(near(vs[0].angle, FRAC_PI_4) && near(vs[1].angle, FRAC_PI_4));
        assert_eq!((vs[1].x, vs[1].y), (10.0, 10.0));
    }

    #[test]
    fn a_corner_faces_between_the_two_directions() {
        // Right, then down: the corner bisects 0 and 90 degrees.
        let vs = v("M0 0 L10 0 L10 10");
        assert_eq!(vs.len(), 3);
        assert!(near(vs[0].angle, 0.0));
        assert!(near(vs[1].angle, FRAC_PI_4));
        assert!(near(vs[2].angle, FRAC_PI_2));
    }

    #[test]
    fn the_bisector_takes_the_short_way_round_the_half_turn() {
        // Left, then up and back left of straight: 170 degrees then 190 degrees must bisect to 180, not 0.
        let a = bisect(Some((-1.0, 0.17)), Some((-1.0, -0.17)));
        assert!((a.abs() - PI).abs() < 0.01 || near(a.abs(), PI), "{a}");
    }

    #[test]
    fn relative_and_axis_commands_give_the_same_vertices() {
        let abs = v("M10 10 L20 10 V30 H5");
        let rel = v("m10 10 l10 0 v20 h-15");
        assert_eq!(abs, rel);
        assert_eq!(abs.len(), 4);
    }

    #[test]
    fn a_curve_leaves_and_arrives_along_its_tangents() {
        let vs = v("M0 0 C0 10 10 20 20 20");
        assert!(near(vs[0].angle, FRAC_PI_2), "leaves along the first control arm: {}", vs[0].angle);
        assert!(near(vs[1].angle, 0.0), "arrives along the last control arm: {}", vs[1].angle);
    }

    #[test]
    fn coincident_control_points_fall_back_to_the_next_one() {
        let vs = v("M0 0 C0 0 10 0 10 10");
        assert!(near(vs[0].angle, 0.0), "{}", vs[0].angle);
        let end = v("M0 0 C0 10 10 10 10 10");
        assert!(near(end[1].angle, 0.0), "{}", end[1].angle);
    }

    #[test]
    fn a_quadratic_matches_its_cubic_equivalent() {
        let q = v("M0 0 Q10 0 10 10");
        assert!(near(q[0].angle, 0.0) && near(q[1].angle, FRAC_PI_2));
    }

    #[test]
    fn an_arc_adds_a_vertex_at_its_end_only() {
        let vs = v("M0 0 A10 10 0 0 1 20 0");
        assert_eq!(vs.len(), 2, "the pieces of the arc are not vertices");
        assert!((vs[1].x - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_closed_path_has_a_closing_vertex_and_does_not_bisect_across_the_closure() {
        // A square drawn clockwise: M, three corners, then Z back to the start.
        let vs = v("M0 0 L10 0 L10 10 L0 10 Z");
        assert_eq!(vs.len(), 5, "the start, three corners and the closepath vertex");
        assert!(near(vs[0].angle, 0.0), "the start faces along the first segment: {}", vs[0].angle);
        assert!(near(vs[4].angle, -FRAC_PI_2), "the closing vertex faces along the closing segment: {}", vs[4].angle);
        assert!(near(vs[3].angle, bisect(Some((-1.0, 0.0)), Some((0.0, -1.0)))), "the corner before Z bisects with the closing segment");
        assert_eq!((vs[4].x, vs[4].y), (0.0, 0.0));
    }

    #[test]
    fn a_back_and_forth_closed_line_faces_out_then_back() {
        // M L Z: the start faces right, the corner bisects to down, the closing vertex faces left.
        let vs = v("M30 100 L170 100 Z");
        assert_eq!(vs.len(), 3);
        assert!(near(vs[0].angle, 0.0) && near(vs[1].angle, FRAC_PI_2) && near(vs[2].angle.abs(), PI), "{vs:?}");
    }

    #[test]
    fn a_rect_is_four_corners_and_a_closing_vertex() {
        let vs = vertices(&rect_commands(20.0, 20.0, 160.0, 160.0, None, None));
        let at: Vec<(f64, f64)> = vs.iter().map(|v| (v.x, v.y)).collect();
        assert_eq!(at, vec![(20.0, 20.0), (180.0, 20.0), (180.0, 180.0), (20.0, 180.0), (20.0, 20.0)]);
    }

    #[test]
    fn a_rounded_rect_has_a_vertex_where_each_corner_arc_starts_and_ends() {
        let vs = vertices(&rect_commands(0.0, 0.0, 100.0, 80.0, Some(10.0), Some(10.0)));
        let at: Vec<(f64, f64)> = vs.iter().map(|v| (v.x.round(), v.y.round())).collect();
        assert_eq!(at, vec![(10.0, 0.0), (90.0, 0.0), (100.0, 10.0), (100.0, 70.0), (90.0, 80.0), (10.0, 80.0), (0.0, 70.0), (0.0, 10.0), (10.0, 0.0), (10.0, 0.0)]);
    }

    #[test]
    fn a_circle_has_a_vertex_at_each_quarter() {
        let vs = vertices(&ellipse_commands(100.0, 100.0, 80.0, 80.0));
        let at: Vec<(f64, f64)> = vs.iter().map(|v| (v.x.round(), v.y.round())).collect();
        assert_eq!(at, vec![(180.0, 100.0), (100.0, 180.0), (20.0, 100.0), (100.0, 20.0), (180.0, 100.0), (180.0, 100.0)]);
    }

    #[test]
    fn an_open_subpath_does_not_bisect_across_a_gap() {
        let vs = v("M0 0 L10 0 M20 20 L30 20");
        assert_eq!(vs.len(), 4);
        assert!(near(vs[1].angle, 0.0) && near(vs[2].angle, 0.0));
    }

    #[test]
    fn a_lone_moveto_is_one_vertex_without_a_direction() {
        let vs = v("M5 6");
        assert_eq!(vs, vec![Vertex { x: 5.0, y: 6.0, angle: 0.0 }]);
    }

    #[test]
    fn segments_after_a_closepath_start_a_new_subpath_at_the_old_start() {
        let vs = v("M0 0 L10 0 L10 10 Z L5 5");
        // First subpath: 4 vertices (M, L, L, Z). Second: the start again, then L.
        assert_eq!(vs.len(), 6);
        assert_eq!((vs[4].x, vs[4].y), (0.0, 0.0));
        assert_eq!((vs[5].x, vs[5].y), (5.0, 5.0));
    }

    #[test]
    fn orient_reads_every_unit() {
        assert_eq!(parse_orient("auto"), Orient::Auto);
        assert_eq!(parse_orient(" auto-start-reverse "), Orient::AutoStartReverse);
        let angle = |s: &str| match parse_orient(s) {
            Orient::Angle(a) => a,
            other => panic!("{other:?}"),
        };
        assert!(near(angle("90"), FRAC_PI_2));
        assert!(near(angle("90deg"), FRAC_PI_2));
        assert!(near(angle("100grad"), FRAC_PI_2));
        assert!(near(angle("1.5rad"), 1.5));
        assert!(near(angle("0.25turn"), FRAC_PI_2));
        assert!(near(angle("-45"), -FRAC_PI_4));
        assert!(near(angle("nonsense"), 0.0));
    }
}
