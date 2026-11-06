// core/msx-svg-import/src/motion.rs
//! Motion paths for the SVG importer.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Animation", for the model and the
//! limits. This file holds only pure functions on paths and numbers, so it has no
//! dependency on the scene types.
//!
//! `animateMotion` moves an element along a path. An MSX track has linear
//! keyframes on translate x, translate y and rotate, so the path is flattened
//! into samples (position and heading) and the samples become the keyframes.
//! Straight lines between close samples follow the path to a fraction of a
//! pixel, and the heading comes from the exact tangent at each sample, so a
//! rotating element turns smoothly on a curve. Where the heading changes at once
//! (a corner, or a jump between subpaths) two samples share one position and the
//! curve pulls them `EPS` apart in time, which makes the turn a short ramp.

use msx_ast::PathCommand;

use crate::anim::Curve;
use crate::markers::{subpaths, Seg};

/// Most samples one path may produce.
const MAX_SAMPLES: usize = 20_000;
/// Most pieces one curved segment is cut into.
const MAX_PIECES: usize = 128;

/// How the element is turned while it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Rotate {
    /// A fixed angle in degrees.
    Fixed(f64),
    /// Along the direction of the path.
    Auto,
    /// Against the direction of the path.
    AutoReverse,
}

impl Rotate {
    /// Whether the element ends up turned at all, so that a rotate channel is needed.
    pub(crate) fn turns(self) -> bool {
        !matches!(self, Rotate::Fixed(a) if a == 0.0)
    }
}

/// Reads `rotate`: a number of degrees, `auto` or `auto-reverse`. Missing is 0.
pub(crate) fn parse_rotate(v: Option<&str>) -> Result<Rotate, String> {
    match v.map(str::trim) {
        None | Some("") => Ok(Rotate::Fixed(0.0)),
        Some("auto") => Ok(Rotate::Auto),
        Some("auto-reverse") => Ok(Rotate::AutoReverse),
        Some(n) => n.parse::<f64>().ok().filter(|a| a.is_finite()).map(Rotate::Fixed).ok_or_else(|| format!("rotate=\"{n}\" is not a number, auto or auto-reverse")),
    }
}

/// A semicolon separated list of numbers, all of them or nothing.
pub(crate) fn parse_list(s: &str) -> Option<Vec<f64>> {
    let items: Vec<&str> = s.split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
    if items.is_empty() {
        return None;
    }
    items.iter().map(|p| p.parse::<f64>().ok().filter(|v| v.is_finite())).collect()
}

/// One point of the flattened path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sample {
    /// Distance travelled along the path, in user units.
    pub s: f64,
    pub x: f64,
    pub y: f64,
    /// Heading in degrees, continuous from sample to sample.
    pub angle: f64,
}

/// A flattened path, ready to become a curve.
#[derive(Debug, Clone)]
pub(crate) struct Flat {
    samples: Vec<Sample>,
    length: f64,
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn point(seg: &Seg, t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * seg.from.0 + b * seg.c1.0 + c * seg.c2.0 + d * seg.to.0, a * seg.from.1 + b * seg.c1.1 + c * seg.c2.1 + d * seg.to.1)
}

/// The direction the segment moves in at `t`, or `None` where it stands still.
fn tangent(seg: &Seg, t: f64) -> Option<(f64, f64)> {
    let u = 1.0 - t;
    let d = |p0: f64, p1: f64, p2: f64, p3: f64| 3.0 * (u * u * (p1 - p0) + 2.0 * u * t * (p2 - p1) + t * t * (p3 - p2));
    let v = (d(seg.from.0, seg.c1.0, seg.c2.0, seg.to.0), d(seg.from.1, seg.c1.1, seg.c2.1, seg.to.1));
    (v.0.hypot(v.1) > 1e-9).then_some(v)
}

/// How many pieces a segment is cut into. A straight line is one.
fn pieces(seg: &Seg) -> usize {
    if seg.c1 == seg.from && seg.c2 == seg.to {
        return 1;
    }
    let hull = dist(seg.from, seg.c1) + dist(seg.c1, seg.c2) + dist(seg.c2, seg.to);
    ((hull.sqrt() * 1.5).ceil() as usize).clamp(4, MAX_PIECES)
}

fn degrees(d: (f64, f64)) -> f64 {
    d.1.atan2(d.0).to_degrees()
}

struct Raw {
    s: f64,
    p: (f64, f64),
    angle: Option<f64>,
}

/// Adds a sample unless it repeats the last one. A second sample at the same
/// place with another heading is a corner and is kept.
fn push(raw: &mut Vec<Raw>, s: f64, p: (f64, f64), angle: Option<f64>) {
    if let Some(last) = raw.last_mut() {
        if (last.s - s).abs() < 1e-9 && dist(last.p, p) < 1e-9 {
            match (last.angle, angle) {
                (_, None) => return,
                (None, Some(_)) => {
                    last.angle = angle;
                    return;
                }
                (Some(a), Some(b)) if (a - b).abs() < 1e-6 => return,
                _ => {}
            }
        }
    }
    raw.push(Raw { s, p, angle });
}

/// Cuts the path into samples. The path has to have some length.
pub(crate) fn flatten(cmds: &[PathCommand], rotate: Rotate) -> Result<Flat, String> {
    let mut raw: Vec<Raw> = Vec::new();
    let mut s = 0.0;
    for sub in subpaths(cmds) {
        push(&mut raw, s, sub.start, sub.segs.iter().find_map(Seg::start_dir).map(degrees));
        for seg in &sub.segs {
            if let Some(d) = seg.start_dir() {
                push(&mut raw, s, seg.from, Some(degrees(d)));
            }
            let n = pieces(seg);
            let mut prev = seg.from;
            for k in 1..=n {
                let t = k as f64 / n as f64;
                let p = if k == n { seg.to } else { point(seg, t) };
                s += dist(prev, p);
                prev = p;
                let dir = if k == n { seg.end_dir() } else { tangent(seg, t) };
                push(&mut raw, s, p, dir.map(degrees));
            }
            if raw.len() > MAX_SAMPLES {
                return Err(format!("its path needs more than {MAX_SAMPLES} samples"));
            }
        }
    }
    if !s.is_finite() || raw.iter().any(|r| !r.p.0.is_finite() || !r.p.1.is_finite()) {
        return Err("its path has coordinates that are too large".into());
    }
    if s < 1e-9 {
        return Err("its path has no length".into());
    }

    // A sample without a direction (a stop on the path) keeps the one before it;
    // leading ones take the first direction there is.
    let first = raw.iter().find_map(|r| r.angle).unwrap_or(0.0);
    let mut carry = first;
    let mut samples = Vec::with_capacity(raw.len());
    let mut prev: Option<f64> = None;
    for r in &raw {
        let mut a = r.angle.unwrap_or(carry);
        carry = a;
        if let Some(p) = prev {
            // Turn the short way round, so the heading never jumps by a whole turn.
            a += 360.0 * ((p - a) / 360.0).round();
        }
        prev = Some(a);
        let angle = match rotate {
            Rotate::Fixed(v) => v,
            Rotate::Auto => a,
            Rotate::AutoReverse => a + 180.0,
        };
        samples.push(Sample { s: r.s, x: r.p.0, y: r.p.1, angle });
    }
    Ok(Flat { samples, length: s })
}

impl Flat {
    /// Total length of the flattened path.
    #[cfg(test)]
    pub(crate) fn length(&self) -> f64 {
        self.length
    }

    fn frac(&self, i: usize) -> f64 {
        self.samples[i].s / self.length
    }

    fn mix(a: &Sample, b: &Sample, f: f64) -> Sample {
        Sample { s: a.s + (b.s - a.s) * f, x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f, angle: a.angle + (b.angle - a.angle) * f }
    }

    /// The state on leaving the point at fraction `p` of the length going
    /// forward. At a corner that is the sample after the turn.
    fn leaving(&self, p: f64) -> Sample {
        let n = self.samples.len();
        let i = (0..n).rev().find(|&i| self.frac(i) <= p + 1e-12).unwrap_or(0);
        if i + 1 >= n || (self.frac(i) - p).abs() < 1e-12 {
            return self.samples[i];
        }
        let (fa, fb) = (self.frac(i), self.frac(i + 1));
        Flat::mix(&self.samples[i], &self.samples[i + 1], if fb > fa { (p - fa) / (fb - fa) } else { 0.0 })
    }

    /// The state on arriving at the point at fraction `p`. At a corner that is
    /// the sample before the turn.
    fn arriving(&self, p: f64) -> Sample {
        let n = self.samples.len();
        let i = (0..n).find(|&i| self.frac(i) >= p - 1e-12).unwrap_or(n - 1);
        if i == 0 || (self.frac(i) - p).abs() < 1e-12 {
            return self.samples[i];
        }
        let (fa, fb) = (self.frac(i - 1), self.frac(i));
        Flat::mix(&self.samples[i - 1], &self.samples[i], if fb > fa { (p - fa) / (fb - fa) } else { 1.0 })
    }

    /// The fraction of the length at each of `points`, which must lie on the
    /// path in order. Used for `values`, where each value is a point on it.
    pub(crate) fn fractions_of(points: &[(f64, f64)]) -> Vec<f64> {
        let mut acc = 0.0;
        let mut out = vec![0.0];
        for w in points.windows(2) {
            acc += dist(w[0], w[1]);
            out.push(acc);
        }
        let total = acc.max(1e-300);
        out.iter().map(|d| d / total).collect()
    }
}

fn add(times: &mut Vec<f64>, rows: &mut Vec<Vec<f64>>, t: f64, smp: Sample, angle: bool) {
    let row: Vec<f64> = if angle { vec![smp.x, smp.y, smp.angle] } else { vec![smp.x, smp.y] };
    if let (Some(lt), Some(lr)) = (times.last(), rows.last()) {
        if (t - lt).abs() < 1e-12 && lr.iter().zip(&row).all(|(a, b)| (a - b).abs() < 1e-9) {
            return;
        }
    }
    times.push(t);
    rows.push(row);
}

/// The curve of one iteration: a row of x, y (and the heading) at each sample.
///
/// `keys` is `(keyTimes, keyPoints)`: at each key time the element has gone
/// the given fraction of the path, and between two key times it moves at a
/// constant speed, forwards or backwards. Without it the speed is constant
/// over the whole path.
pub(crate) fn curve(flat: &Flat, keys: Option<(&[f64], &[f64])>, angle: bool, dur: f64) -> Result<Curve, String> {
    let (kt, kp): (Vec<f64>, Vec<f64>) = match keys {
        Some((t, p)) => (t.to_vec(), p.to_vec()),
        None => (vec![0.0, 1.0], vec![0.0, 1.0]),
    };
    if kt.len() != kp.len() || kt.len() < 2 {
        return Err(format!("keyPoints has {} entries for {} keyTimes", kp.len(), kt.len()));
    }
    if kt[0] != 0.0 || kt[kt.len() - 1] != 1.0 || kt.windows(2).any(|w| w[1] < w[0]) || kt.iter().any(|t| !(0.0..=1.0).contains(t)) {
        return Err("keyTimes must rise from 0 to 1".into());
    }
    if kp.iter().any(|p| !(0.0..=1.0).contains(p)) {
        return Err("keyPoints must lie in 0 to 1".into());
    }
    let (mut times, mut rows) = (Vec::new(), Vec::new());
    for j in 0..kt.len() - 1 {
        let (t0, t1, p0, p1) = (kt[j], kt[j + 1], kp[j], kp[j + 1]);
        let forward = p1 >= p0;
        // Going forward the element enters the segment after a corner, going back the one before it.
        let start = if forward { flat.leaving(p0) } else { flat.arriving(p0) };
        add(&mut times, &mut rows, t0, start, angle);
        let (lo, hi) = (p0.min(p1), p0.max(p1));
        if hi > lo + 1e-12 {
            let inside: Vec<(f64, &Sample)> = flat.samples.iter().enumerate().map(|(i, smp)| (flat.frac(i), smp)).filter(|(f, _)| *f > lo + 1e-12 && *f < hi - 1e-12).collect();
            let crossed: Box<dyn Iterator<Item = &(f64, &Sample)>> = if forward { Box::new(inside.iter()) } else { Box::new(inside.iter().rev()) };
            for (f, smp) in crossed {
                add(&mut times, &mut rows, t0 + (f - p0) / (p1 - p0) * (t1 - t0), **smp, angle);
            }
        }
        let end = if hi <= lo + 1e-12 {
            start
        } else if forward {
            flat.arriving(p1)
        } else {
            flat.leaving(p1)
        };
        add(&mut times, &mut rows, t1, end, angle);
    }
    Curve::from_points(times, rows, dur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::path::parse_d;

    /// The value of a polyline at `t`, straight between points.
    fn eval(pts: &[(f64, Vec<f64>)], t: f64) -> Vec<f64> {
        let i = pts.iter().position(|p| p.0 >= t).unwrap_or(pts.len() - 1);
        if i == 0 || (pts[i].0 - t).abs() < 1e-12 {
            return pts[i].1.clone();
        }
        let (a, b) = (&pts[i - 1], &pts[i]);
        let f = (t - a.0) / (b.0 - a.0);
        a.1.iter().zip(&b.1).map(|(x, y)| x + (y - x) * f).collect()
    }

    fn flat(d: &str, rotate: Rotate) -> Flat {
        flatten(&parse_d(d).expect("a path"), rotate).expect("a flat path")
    }

    #[test]
    fn rotate_values() {
        assert_eq!(parse_rotate(None), Ok(Rotate::Fixed(0.0)));
        assert_eq!(parse_rotate(Some(" auto ")), Ok(Rotate::Auto));
        assert_eq!(parse_rotate(Some("auto-reverse")), Ok(Rotate::AutoReverse));
        assert_eq!(parse_rotate(Some("-30.5")), Ok(Rotate::Fixed(-30.5)));
        assert!(parse_rotate(Some("sideways")).is_err());
        assert!(!Rotate::Fixed(0.0).turns() && Rotate::Fixed(1.0).turns() && Rotate::Auto.turns());
    }

    #[test]
    fn lists() {
        assert_eq!(parse_list("0; .5 ;1"), Some(vec![0.0, 0.5, 1.0]));
        assert_eq!(parse_list("0;x"), None);
        assert_eq!(parse_list(" ; "), None);
    }

    #[test]
    fn a_straight_path_has_two_samples_and_its_length() {
        let f = flat("M 0 0 L 30 40", Rotate::Auto);
        assert!((f.length() - 50.0).abs() < 1e-9);
        assert_eq!(f.samples.len(), 2);
        assert!((f.samples[0].angle - 53.130_102).abs() < 1e-5);
    }

    #[test]
    fn a_circle_is_flattened_to_its_circumference_and_the_heading_turns_smoothly() {
        let f = flat("M 50 0 A 50 50 0 1 1 -50 0 A 50 50 0 1 1 50 0 Z", Rotate::Auto);
        let tau_r = std::f64::consts::TAU * 50.0;
        assert!((f.length() - tau_r).abs() < tau_r * 1e-3, "{}", f.length());
        // Continuous: no step of the heading is bigger than a few degrees, and a lap is 360.
        for w in f.samples.windows(2) {
            assert!((w[1].angle - w[0].angle).abs() < 15.0, "{w:?}");
        }
        let turned = f.samples[f.samples.len() - 1].angle - f.samples[0].angle;
        assert!((turned - 360.0).abs() < 1.0, "{turned}");
    }

    #[test]
    fn a_corner_has_two_samples_at_one_place_with_different_headings() {
        let f = flat("M 0 0 L 10 0 L 10 10", Rotate::Auto);
        let corner: Vec<&Sample> = f.samples.iter().filter(|s| (s.x, s.y) == (10.0, 0.0)).collect();
        assert_eq!(corner.len(), 2, "{:?}", f.samples);
        assert!((corner[0].angle - 0.0).abs() < 1e-9 && (corner[1].angle - 90.0).abs() < 1e-9);
    }

    #[test]
    fn the_heading_turns_the_short_way_round() {
        // East, then north-west past the 180 degree line: -135 would be 225 without the unwrapping.
        let f = flat("M 0 0 L 10 0 L 0 10 L -10 0 L 0 -10", Rotate::Auto);
        for w in f.samples.windows(2) {
            assert!((w[1].angle - w[0].angle).abs() <= 180.0 + 1e-9, "{w:?}");
        }
        assert!(f.samples[f.samples.len() - 1].angle > 180.0, "a full left turn keeps counting");
    }

    #[test]
    fn auto_reverse_and_fixed_angles() {
        assert!((flat("M 0 0 L 10 0", Rotate::AutoReverse).samples[0].angle - 180.0).abs() < 1e-9);
        assert!(flat("M 0 0 L 10 0 L 10 10", Rotate::Fixed(30.0)).samples.iter().all(|s| s.angle == 30.0));
    }

    #[test]
    fn a_path_without_length_is_an_error() {
        assert!(flatten(&parse_d("M 5 5").unwrap(), Rotate::Auto).is_err());
        assert!(flatten(&parse_d("M 5 5 L 5 5").unwrap(), Rotate::Auto).is_err());
    }

    #[test]
    fn a_path_with_huge_coordinates_is_an_error() {
        let huge = parse_d("M 0 0 L 1e308 1e308 L -1e308 -1e308").unwrap();
        assert!(flatten(&huge, Rotate::Auto).is_err());
    }

    #[test]
    fn a_second_subpath_is_a_jump() {
        let f = flat("M 0 0 L 10 0 M 50 50 L 60 50", Rotate::Fixed(0.0));
        assert!((f.length() - 20.0).abs() < 1e-9, "the jump adds no length");
        let c = curve(&f, None, false, 2.0).unwrap();
        let pts = c.polyline_for_test(2.0);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(pts.iter().any(|(t, v)| near(*t, 0.5) && near(v[0], 10.0) && near(v[1], 0.0)), "{pts:?}");
        assert!(pts.iter().any(|(t, v)| near(v[0], 50.0) && *t < 0.51 && *t > 0.5 && near(v[1], 50.0)), "{pts:?}");
    }

    #[test]
    fn constant_speed_over_the_whole_path() {
        let f = flat("M 0 0 L 10 0 L 10 30", Rotate::Fixed(0.0));
        let c = curve(&f, None, false, 4.0).unwrap();
        let pts = c.polyline_for_test(4.0);
        // 40 long: the corner at 10 is a quarter of the time.
        assert!(pts.iter().any(|(t, v)| (t - 0.25).abs() < 1e-6 && (v[0] - 10.0).abs() < 1e-9 && v[1].abs() < 1e-9), "{pts:?}");
        let last = pts.last().unwrap();
        assert!((last.0 - 1.0).abs() < 1e-9 && (last.1[1] - 30.0).abs() < 1e-9);
    }

    #[test]
    fn key_points_and_key_times_set_the_progress() {
        let f = flat("M 0 0 L 100 0", Rotate::Fixed(0.0));
        let c = curve(&f, Some((&[0.0, 0.25, 1.0], &[0.0, 0.8, 1.0])), false, 1.0).unwrap();
        let pts = c.polyline_for_test(1.0);
        let at = |t: f64| {
            let i = pts.iter().position(|p| p.0 >= t).unwrap();
            if (pts[i].0 - t).abs() < 1e-9 || i == 0 {
                pts[i].1[0]
            } else {
                let (a, b) = (&pts[i - 1], &pts[i]);
                a.1[0] + (b.1[0] - a.1[0]) * (t - a.0) / (b.0 - a.0)
            }
        };
        assert!((at(0.25) - 80.0).abs() < 1e-6 && (at(0.125) - 40.0).abs() < 1e-6 && (at(1.0) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn bad_keys_are_errors() {
        let f = flat("M 0 0 L 100 0", Rotate::Fixed(0.0));
        assert!(curve(&f, Some((&[0.0, 1.0], &[0.0])), false, 1.0).is_err());
        assert!(curve(&f, Some((&[0.0, 0.5], &[0.0, 1.0])), false, 1.0).is_err());
        assert!(curve(&f, Some((&[0.0, 1.0], &[0.0, 1.5])), false, 1.0).is_err());
    }

    #[test]
    fn key_points_may_go_back_along_the_path() {
        // The web platform test for keyPoints: 1 to 0 over the same path.
        let f = flat("M -200 50 h 250", Rotate::Fixed(0.0));
        let c = curve(&f, Some((&[0.0, 1.0], &[1.0, 0.0])), false, 5.0).unwrap();
        let pts = c.polyline_for_test(5.0);
        assert!((pts[0].1[0] - 50.0).abs() < 1e-9 && (pts[0].1[1] - 50.0).abs() < 1e-9, "{pts:?}");
        let last = pts.last().unwrap();
        assert!((last.1[0] + 200.0).abs() < 1e-9, "{pts:?}");
        assert!(pts.windows(2).all(|w| w[1].1[0] <= w[0].1[0] + 1e-9), "monotone back: {pts:?}");
    }

    #[test]
    fn a_ping_pong_visits_the_corner_with_the_heading_of_the_side_it_comes_from() {
        let f = flat("M 0 0 L 10 0 L 10 10", Rotate::Auto);
        let c = curve(&f, Some((&[0.0, 0.5, 1.0], &[0.0, 1.0, 0.0])), true, 4.0).unwrap();
        let pts = c.polyline_for_test(4.0);
        // Out: east then south. Back: the south leg is walked up with the heading of the south leg
        // (the heading follows the path, not the direction of travel), then the east leg.
        let heading_at = |t: f64| eval(&pts, t)[2];
        assert!(heading_at(0.1).abs() < 1e-6 && (heading_at(0.4) - 90.0).abs() < 1e-6);
        assert!((heading_at(0.6) - 90.0).abs() < 1e-6 && heading_at(0.9).abs() < 1e-6, "{} {}", heading_at(0.6), heading_at(0.9));
    }

    #[test]
    fn a_hold_between_key_times() {
        let f = flat("M 0 0 L 100 0", Rotate::Fixed(0.0));
        let c = curve(&f, Some((&[0.0, 0.5, 1.0], &[0.0, 0.5, 0.5])), false, 1.0).unwrap();
        let pts = c.polyline_for_test(1.0);
        assert!((pts.last().unwrap().1[0] - 50.0).abs() < 1e-9, "{pts:?}");
    }

    #[test]
    fn fractions_of_points() {
        let f = Flat::fractions_of(&[(0.0, 0.0), (10.0, 0.0), (10.0, 30.0)]);
        assert_eq!(f.len(), 3);
        assert!((f[1] - 0.25).abs() < 1e-12 && (f[2] - 1.0).abs() < 1e-12);
    }
}
