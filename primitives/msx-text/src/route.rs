// primitives/msx-text/src/route.rs
//! The path a text runs along: the first subpath of a set of path commands as
//! a polyline, with the distance from its start at every point.

use msx_ast::PathCommand;

/// A polyline with the distance along it at each point.
pub(crate) struct Route {
    pts: Vec<(f64, f64)>,
    dist: Vec<f64>,
}

impl Route {
    /// Flattens the first subpath. `None` when it has no length.
    pub(crate) fn new(cmds: &[PathCommand]) -> Option<Route> {
        let pts = flatten_first_subpath(cmds);
        if pts.len() < 2 {
            return None;
        }
        let mut dist = Vec::with_capacity(pts.len());
        let mut total = 0.0;
        dist.push(0.0);
        for w in pts.windows(2) {
            total += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            dist.push(total);
        }
        (total > 0.0).then_some(Route { pts, dist })
    }

    pub(crate) fn length(&self) -> f64 {
        self.dist[self.dist.len() - 1]
    }

    /// The point and the unit direction at distance `s`, which is clamped to
    /// the route.
    pub(crate) fn at(&self, s: f64) -> ((f64, f64), (f64, f64)) {
        let s = s.clamp(0.0, self.length());
        let last = self.pts.len() - 2;
        let i = match self.dist.binary_search_by(|d| d.total_cmp(&s)) {
            Ok(i) => i.min(last),
            Err(i) => i.saturating_sub(1).min(last),
        };
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let seg = self.dist[i + 1] - self.dist[i];
        let t = (s - self.dist[i]) / seg;
        ((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), ((b.0 - a.0) / seg, (b.1 - a.1) / seg))
    }
}

/// Length of the first subpath of `cmds`, zero when it has none.
pub fn path_length(cmds: &[PathCommand]) -> f64 {
    Route::new(cmds).map_or(0.0, |r| r.length())
}

/// The first subpath of `cmds` as points joined by straight lines.
pub fn path_points(cmds: &[PathCommand]) -> Vec<(f64, f64)> {
    flatten_first_subpath(cmds)
}

/// The point at distance `s` along the first subpath of `cmds`, clamped to its
/// ends. `None` when it has no length.
pub fn path_point_at(cmds: &[PathCommand], s: f64) -> Option<(f64, f64)> {
    Route::new(cmds).map(|r| r.at(s).0)
}

fn flatten_first_subpath(cmds: &[PathCommand]) -> Vec<(f64, f64)> {
    use PathCommand::*;
    let mut pts: Vec<(f64, f64)> = Vec::new();
    let (mut cur, mut start) = ((0.0, 0.0), (0.0, 0.0));
    // The second control point of the last curve, for the smooth commands.
    let mut last_cubic: Option<(f64, f64)> = None;
    let mut last_quad: Option<(f64, f64)> = None;
    let mut open = false;

    fn push(pts: &mut Vec<(f64, f64)>, p: (f64, f64)) {
        if pts.last().is_none_or(|q| (q.0 - p.0).hypot(q.1 - p.1) > 1e-9) {
            pts.push(p);
        }
    }

    for c in cmds {
        let (mut next_cubic, mut next_quad) = (None, None);
        match c {
            MoveTo(p) | RelMoveTo(p) => {
                if open {
                    break;
                }
                let rel = matches!(c, RelMoveTo(_));
                cur = if rel { (cur.0 + p.x, cur.1 + p.y) } else { (p.x, p.y) };
                start = cur;
                push(&mut pts, cur);
                open = true;
            }
            LineTo(p) | RelLineTo(p) => {
                cur = if matches!(c, RelLineTo(_)) { (cur.0 + p.x, cur.1 + p.y) } else { (p.x, p.y) };
                push(&mut pts, cur);
            }
            HLineTo(x) | RelHLineTo(x) => {
                cur.0 = if matches!(c, RelHLineTo(_)) { cur.0 + x } else { *x };
                push(&mut pts, cur);
            }
            VLineTo(y) | RelVLineTo(y) => {
                cur.1 = if matches!(c, RelVLineTo(_)) { cur.1 + y } else { *y };
                push(&mut pts, cur);
            }
            CubicBezier { c1, c2, to } | RelCubicBezier { c1, c2, to } => {
                let o = if matches!(c, RelCubicBezier { .. }) { cur } else { (0.0, 0.0) };
                let (p1, p2, p3) = ((o.0 + c1.x, o.1 + c1.y), (o.0 + c2.x, o.1 + c2.y), (o.0 + to.x, o.1 + to.y));
                cubic(&mut pts, cur, p1, p2, p3);
                next_cubic = Some(p2);
                cur = p3;
            }
            SmoothCubic { c2, to } | RelSmoothCubic { c2, to } => {
                let o = if matches!(c, RelSmoothCubic { .. }) { cur } else { (0.0, 0.0) };
                let p1 = last_cubic.map_or(cur, |q| (2.0 * cur.0 - q.0, 2.0 * cur.1 - q.1));
                let (p2, p3) = ((o.0 + c2.x, o.1 + c2.y), (o.0 + to.x, o.1 + to.y));
                cubic(&mut pts, cur, p1, p2, p3);
                next_cubic = Some(p2);
                cur = p3;
            }
            QuadBezier { c: q, to } | RelQuadBezier { c: q, to } => {
                let o = if matches!(c, RelQuadBezier { .. }) { cur } else { (0.0, 0.0) };
                let (p1, p2) = ((o.0 + q.x, o.1 + q.y), (o.0 + to.x, o.1 + to.y));
                quad(&mut pts, cur, p1, p2);
                next_quad = Some(p1);
                cur = p2;
            }
            SmoothQuad { to } | RelSmoothQuad { to } => {
                let o = if matches!(c, RelSmoothQuad { .. }) { cur } else { (0.0, 0.0) };
                let p1 = last_quad.map_or(cur, |q| (2.0 * cur.0 - q.0, 2.0 * cur.1 - q.1));
                let p2 = (o.0 + to.x, o.1 + to.y);
                quad(&mut pts, cur, p1, p2);
                next_quad = Some(p1);
                cur = p2;
            }
            Arc { rx, ry, x_rotation, large_arc, sweep, to } | RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
                let o = if matches!(c, RelArc { .. }) { cur } else { (0.0, 0.0) };
                let end = (o.0 + to.x, o.1 + to.y);
                arc(&mut pts, cur, (*rx, *ry), *x_rotation, (*large_arc, *sweep), end);
                cur = end;
            }
            ClosePath => {
                if open {
                    push(&mut pts, start);
                    break;
                }
            }
        }
        last_cubic = next_cubic;
        last_quad = next_quad;
    }
    pts
}

/// Segments for a curve whose control polygon is `len` long.
fn steps(len: f64) -> usize {
    ((len / 2.0).ceil() as usize).clamp(4, 256)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn cubic(pts: &mut Vec<(f64, f64)>, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
    let n = steps(dist(p0, p1) + dist(p1, p2) + dist(p2, p3));
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        let p = (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1);
        if pts.last().is_none_or(|q| dist(*q, p) > 1e-9) {
            pts.push(p);
        }
    }
}

fn quad(pts: &mut Vec<(f64, f64)>, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) {
    let n = steps(dist(p0, p1) + dist(p1, p2));
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        let (a, b, c) = (u * u, 2.0 * u * t, t * t);
        let p = (a * p0.0 + b * p1.0 + c * p2.0, a * p0.1 + b * p1.1 + c * p2.1);
        if pts.last().is_none_or(|q| dist(*q, p) > 1e-9) {
            pts.push(p);
        }
    }
}

/// An elliptical arc from `p0` to `p1`, by the centre form of the SVG
/// implementation notes. A zero radius is a straight line.
fn arc(pts: &mut Vec<(f64, f64)>, p0: (f64, f64), radii: (f64, f64), rotation: f64, flags: (bool, bool), p1: (f64, f64)) {
    let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
    if (p0 == p1) || rx == 0.0 || ry == 0.0 {
        if pts.last().is_none_or(|q| dist(*q, p1) > 1e-9) {
            pts.push(p1);
        }
        return;
    }
    let (large, sweep) = flags;
    let (sin_phi, cos_phi) = rotation.to_radians().sin_cos();
    let (dx, dy) = ((p0.0 - p1.0) / 2.0, (p0.1 - p1.1) / 2.0);
    let (x1, y1) = (cos_phi * dx + sin_phi * dy, -sin_phi * dx + cos_phi * dy);
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if large == sweep {
        k = -k;
    }
    let (cxp, cyp) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (cos_phi * cxp - sin_phi * cyp + (p0.0 + p1.0) / 2.0, sin_phi * cxp + cos_phi * cyp + (p0.1 + p1.1) / 2.0);
    let theta1 = ((y1 - cyp) / ry).atan2((x1 - cxp) / rx);
    let theta2 = ((-y1 - cyp) / ry).atan2((-x1 - cxp) / rx);
    let mut delta = theta2 - theta1;
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    let n = steps(delta.abs() * rx.max(ry)).max(8);
    for i in 1..=n {
        let th = theta1 + delta * i as f64 / n as f64;
        let (s, c) = th.sin_cos();
        let p = (cos_phi * rx * c - sin_phi * ry * s + cx, sin_phi * rx * c + cos_phi * ry * s + cy);
        if pts.last().is_none_or(|q| dist(*q, p) > 1e-9) {
            pts.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::path::parse_d;

    fn route(d: &str) -> Route {
        Route::new(&parse_d(d).unwrap()).expect("the path has a length")
    }

    #[test]
    fn a_line_has_its_length_and_direction() {
        let r = route("M 10 20 L 40 60");
        assert!((r.length() - 50.0).abs() < 1e-9);
        let (p, d) = r.at(25.0);
        assert!((p.0 - 25.0).abs() < 1e-9 && (p.1 - 40.0).abs() < 1e-9);
        assert!((d.0 - 0.6).abs() < 1e-9 && (d.1 - 0.8).abs() < 1e-9);
    }

    #[test]
    fn relative_and_absolute_commands_agree() {
        let a = route("M 10 10 L 50 10 L 50 40 H 20 V 80");
        let b = route("m 10 10 l 40 0 l 0 30 h -30 v 40");
        assert!((a.length() - b.length()).abs() < 1e-9);
        assert!((a.length() - (40.0 + 30.0 + 30.0 + 40.0)).abs() < 1e-9);
    }

    #[test]
    fn the_distance_clamps_to_the_ends() {
        let r = route("M 0 0 L 10 0");
        assert_eq!(r.at(-5.0).0, (0.0, 0.0));
        assert_eq!(r.at(99.0).0, (10.0, 0.0));
    }

    #[test]
    fn a_closed_path_includes_the_closing_edge() {
        assert!((route("M 0 0 L 10 0 L 10 10 Z").length() - (10.0 + 10.0 + 200f64.sqrt())).abs() < 1e-9);
    }

    #[test]
    fn a_circle_made_of_arcs_has_the_circumference() {
        let r = route("M 100 50 A 50 50 0 1 1 0 50 A 50 50 0 1 1 100 50");
        let circumference = std::f64::consts::TAU * 50.0;
        assert!((r.length() - circumference).abs() < 0.5, "{} vs {circumference}", r.length());
    }

    #[test]
    fn a_cubic_matches_a_known_length() {
        // A cubic that is a straight line with evenly spaced controls.
        assert!((route("M 0 0 C 10 0 20 0 30 0").length() - 30.0).abs() < 1e-6);
    }

    #[test]
    fn a_quarter_circle_cubic_is_about_a_quarter_of_the_circumference() {
        let r = route("M 50 0 C 77.614 0 100 22.386 100 50");
        assert!((r.length() - std::f64::consts::FRAC_PI_2 * 50.0).abs() < 0.1, "{}", r.length());
    }

    #[test]
    fn smooth_commands_reflect_the_last_control_point() {
        // The smooth cubic repeats the first curve mirrored, so the lengths match.
        let r = route("M 0 0 C 0 20 20 40 40 40 S 80 20 80 0");
        let first = route("M 0 0 C 0 20 20 40 40 40").length();
        assert!((r.length() - 2.0 * first).abs() < 0.2, "{} vs {}", r.length(), 2.0 * first);
        let q = route("M 0 0 Q 10 20 20 0 T 40 0");
        assert!((q.length() - 2.0 * route("M 0 0 Q 10 20 20 0").length()).abs() < 0.1);
    }

    #[test]
    fn only_the_first_subpath_is_followed() {
        let r = route("M 0 0 L 10 0 M 100 100 L 200 100");
        assert!((r.length() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn points_and_point_at_follow_the_same_route() {
        let cmds = parse_d("M 0 0 L 30 40").unwrap();
        assert_eq!(path_points(&cmds), vec![(0.0, 0.0), (30.0, 40.0)]);
        let (x, y) = path_point_at(&cmds, 25.0).unwrap();
        assert!((x - 15.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9);
        assert_eq!(path_point_at(&cmds, 999.0), Some((30.0, 40.0)));
        assert_eq!(path_point_at(&[], 1.0), None);
    }

    #[test]
    fn no_length_is_no_route() {
        assert!(Route::new(&parse_d("M 5 5").unwrap()).is_none());
        assert!(Route::new(&parse_d("M 5 5 L 5 5").unwrap()).is_none());
        assert_eq!(path_length(&[]), 0.0);
    }

    #[test]
    fn a_direction_follows_a_curve() {
        let r = route("M 0 0 C 0 50 50 100 100 100");
        let (_, start) = r.at(0.0);
        let (_, end) = r.at(r.length());
        assert!(start.1 > 0.99, "leaves downwards: {start:?}");
        assert!(end.0 > 0.99, "arrives to the right: {end:?}");
    }
}
