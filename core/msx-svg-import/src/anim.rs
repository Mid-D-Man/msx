// core/msx-svg-import/src/anim.rs
//! SMIL timing and keyframe baking for the SVG importer.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Animation", for the model and the
//! limits. This file holds only pure functions on numbers, so it has no
//! dependency on the scene types.
//!
//! An MSX scene has one timeline (`duration` plus `loop_mode`) and tracks of
//! linear keyframes with four easings. SMIL has a timeline per animation, with
//! `begin`, `dur`, repeats, `fill`, `keyTimes`, `calcMode` and `keySplines`.
//! The two meet by baking: every animation is turned into dense points on the
//! scene timeline, and a track is one linear keyframe per point.

/// A jump between two values is written as a ramp this long, in seconds.
pub(crate) const EPS: f64 = 0.001;
/// Most keyframes one animation may need on the scene timeline.
pub(crate) const MAX_POINTS: usize = 20_000;
/// Longest scene loop the planner builds from the periods of repeating animations.
pub(crate) const MAX_LOOP_SECS: f64 = 60.0;
/// Pieces one `keySplines` segment is cut into.
const SPLINE_STEPS: usize = 12;

fn number(s: &str) -> Option<f64> {
    let v: f64 = s.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

/// A SMIL clock value in seconds: `3s`, `500ms`, `1.5min`, `2h`, `1:30`,
/// `00:01:30.5` or a bare number of seconds.
pub(crate) fn parse_clock(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() > 3 {
            return None;
        }
        let mut total = 0.0;
        for (i, p) in parts.iter().enumerate() {
            let v = if i + 1 == parts.len() { number(p)? } else { f64::from(p.trim().parse::<u32>().ok()?) };
            if v < 0.0 {
                return None;
            }
            total = total * 60.0 + v;
        }
        return Some(total);
    }
    let (digits, scale) = if let Some(r) = s.strip_suffix("ms") {
        (r, 0.001)
    } else if let Some(r) = s.strip_suffix("min") {
        (r, 60.0)
    } else if let Some(r) = s.strip_suffix('h') {
        (r, 3600.0)
    } else if let Some(r) = s.strip_suffix('s') {
        (r, 1.0)
    } else {
        (s, 1.0)
    };
    number(digits).map(|v| v * scale)
}

/// The start of an animation in seconds. A list takes its earliest offset.
/// A start that waits for an event, another animation or never comes is an
/// error, because the scene has no input to wait for.
pub(crate) fn parse_begin(attr: Option<&str>) -> Result<f64, String> {
    let Some(raw) = attr else { return Ok(0.0) };
    let mut best: Option<f64> = None;
    let mut other = false;
    for item in raw.split(';').map(str::trim).filter(|i| !i.is_empty()) {
        match parse_clock(item) {
            Some(v) => best = Some(best.map_or(v, |b| b.min(v))),
            None => other = true,
        }
    }
    match best {
        Some(v) => Ok(v),
        None if other => Err(format!("begin=\"{}\" waits for an event or another animation", raw.trim())),
        None => Ok(0.0),
    }
}

/// When an animation runs, from its timing attributes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Timing {
    /// Start in seconds. May be negative: the animation is already running at 0.
    pub begin: f64,
    /// Length of one iteration in seconds, above zero.
    pub dur: f64,
    /// Total active time in seconds, or `None` when it repeats forever.
    pub active: Option<f64>,
    /// `fill="freeze"`: hold the last value after the end instead of going back.
    pub freeze: bool,
    /// CSS `animation-fill-mode: backwards`: hold the first value during the
    /// delay instead of the value without the animation.
    pub fill_before: bool,
}

impl Timing {
    /// Reads `begin`, `dur`, `repeatCount`, `repeatDur`, `end` and `fill`.
    /// Attributes that are read but cannot be honored add a line to `notes`.
    pub(crate) fn parse<'a>(get: impl Fn(&str) -> Option<&'a str>, notes: &mut Vec<String>) -> Result<Timing, String> {
        let begin = parse_begin(get("begin"))?;
        let dur = match get("dur").map(str::trim) {
            None | Some("indefinite") | Some("media") => return Err("it has no finite dur".into()),
            Some(v) => parse_clock(v).filter(|d| *d > 0.0).ok_or_else(|| format!("dur=\"{v}\" is not a positive time"))?,
        };
        if dur < EPS * 4.0 {
            return Err(format!("dur=\"{dur}s\" is too short"));
        }
        // None: not given. Some(None): forever. Some(Some(x)): x seconds.
        let by_count = match get("repeatCount").map(str::trim) {
            None => None,
            Some("indefinite") => Some(None),
            Some(v) => Some(Some(number(v).filter(|c| *c > 0.0).ok_or_else(|| format!("repeatCount=\"{v}\" is not a positive number"))? * dur)),
        };
        let by_dur = match get("repeatDur").map(str::trim) {
            None => None,
            Some("indefinite") => Some(None),
            Some(v) => Some(Some(parse_clock(v).filter(|d| *d > 0.0).ok_or_else(|| format!("repeatDur=\"{v}\" is not a positive time"))?)),
        };
        let mut active = match (by_count, by_dur) {
            (None, None) => Some(dur),
            (Some(c), None) => c,
            (None, Some(d)) => d,
            (Some(c), Some(d)) => match (c, d) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) | (None, Some(a)) => Some(a),
                (None, None) => None,
            },
        };
        if let Some(raw) = get("end").map(str::trim).filter(|e| !e.is_empty() && *e != "indefinite") {
            match parse_begin(Some(raw)) {
                Ok(end) if end <= begin => return Err(format!("end=\"{raw}\" is not after its begin")),
                Ok(end) => active = Some(active.map_or(end - begin, |a| a.min(end - begin))),
                Err(_) => notes.push(format!("end=\"{raw}\" waits for an event or another animation and was ignored")),
            }
        }
        Ok(Timing { begin, dur, active, freeze: get("fill").is_some_and(|f| f.trim() == "freeze"), fill_before: false })
    }

    /// Where the animation stops on the scene timeline: infinite when it repeats forever.
    pub(crate) fn end(&self) -> f64 {
        self.active.map_or(f64::INFINITY, |a| self.begin + a)
    }
}

/// How the value moves between two key times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Calc {
    Linear,
    Discrete,
    Paced,
    Spline,
}

/// The values of one animation over one iteration, with their key times.
#[derive(Debug, Clone)]
pub(crate) struct Curve {
    times: Vec<f64>,
    values: Vec<Vec<f64>>,
    calc: Calc,
    splines: Vec<[f64; 4]>,
}

fn lerp(a: &[f64], b: &[f64], t: f64) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x + (y - x) * t).collect()
}

fn list(s: &str) -> impl Iterator<Item = &str> {
    s.split(';').map(str::trim).filter(|p| !p.is_empty())
}

/// Solves the cubic Bezier of `keySplines` for the progress that goes with the time fraction `x`.
fn bezier(c: [f64; 4], x: f64) -> f64 {
    let at = |s: f64, p1: f64, p2: f64| 3.0 * (1.0 - s).powi(2) * s * p1 + 3.0 * (1.0 - s) * s * s * p2 + s.powi(3);
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if at(mid, c[0], c[2]) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at((lo + hi) / 2.0, c[1], c[3])
}

impl Curve {
    /// Builds the curve from the already parsed `values` (each entry has the
    /// same number of components) and the raw `keyTimes`, `calcMode` and
    /// `keySplines` attributes.
    pub(crate) fn new(values: Vec<Vec<f64>>, key_times: Option<&str>, calc: Option<&str>, key_splines: Option<&str>) -> Result<Curve, String> {
        let n = values.len();
        if n == 0 {
            return Err("it has no values".into());
        }
        let width = values[0].len();
        if width == 0 || values.iter().any(|v| v.len() != width) {
            return Err("its values do not all have the same number of components".into());
        }
        let calc = match calc.map(str::trim) {
            None | Some("") | Some("linear") => Calc::Linear,
            Some("discrete") => Calc::Discrete,
            Some("paced") => Calc::Paced,
            Some("spline") => Calc::Spline,
            Some(other) => return Err(format!("calcMode=\"{other}\" is not known")),
        };
        let given = match key_times.map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(s) => Some(list(s).map(number).collect::<Option<Vec<f64>>>().ok_or("keyTimes is not a list of numbers")?),
        };
        if let (Some(g), true) = (&given, n > 1 && calc != Calc::Paced) {
            if g.len() != n {
                return Err(format!("keyTimes has {} entries for {n} values", g.len()));
            }
            if g[0] != 0.0 || (calc != Calc::Discrete && g[n - 1] != 1.0) || g.windows(2).any(|w| w[1] < w[0]) || g.iter().any(|t| !(0.0..=1.0).contains(t)) {
                return Err("keyTimes must rise from 0 to 1".into());
            }
        }
        let times = if n == 1 {
            vec![0.0]
        } else {
            match (calc, given) {
                (Calc::Paced, _) => {
                    let steps: Vec<f64> = values.windows(2).map(|w| w[0].iter().zip(&w[1]).map(|(a, b)| (b - a).powi(2)).sum::<f64>().sqrt()).collect();
                    let total: f64 = steps.iter().sum();
                    if total <= 0.0 {
                        (0..n).map(|i| i as f64 / (n - 1) as f64).collect()
                    } else {
                        let mut acc = 0.0;
                        let mut t = vec![0.0];
                        for s in &steps {
                            acc += s;
                            t.push(acc / total);
                        }
                        t[n - 1] = 1.0;
                        t
                    }
                }
                (_, Some(g)) => g,
                (Calc::Discrete, None) => (0..n).map(|i| i as f64 / n as f64).collect(),
                (_, None) => (0..n).map(|i| i as f64 / (n - 1) as f64).collect(),
            }
        };
        let mut splines = Vec::new();
        if calc == Calc::Spline && n > 1 {
            let raw = key_splines.map(str::trim).filter(|s| !s.is_empty()).ok_or("calcMode=\"spline\" needs keySplines")?;
            for item in list(raw) {
                let nums: Vec<f64> = item.split(|c: char| c.is_whitespace() || c == ',').filter(|p| !p.is_empty()).filter_map(number).collect();
                if nums.len() != 4 || !(0.0..=1.0).contains(&nums[0]) || !(0.0..=1.0).contains(&nums[2]) {
                    return Err(format!("keySplines entry \"{item}\" is not four numbers with x values in 0 to 1"));
                }
                splines.push([nums[0], nums[1], nums[2], nums[3]]);
            }
            if splines.len() != n - 1 {
                return Err(format!("keySplines has {} entries for {n} values", splines.len()));
            }
        }
        Ok(Curve { times, values, calc, splines })
    }

    /// One iteration as a polyline: progress `0..=1` and the value there.
    /// Linear between the points reproduces the animation.
    fn polyline(&self, dur: f64) -> Vec<(f64, Vec<f64>)> {
        let n = self.values.len();
        let mut out = Vec::new();
        if n == 1 {
            out.push((0.0, self.values[0].clone()));
            out.push((1.0, self.values[0].clone()));
            return out;
        }
        match self.calc {
            Calc::Linear | Calc::Paced => {
                for i in 0..n {
                    out.push((self.times[i], self.values[i].clone()));
                }
            }
            Calc::Spline => {
                for i in 0..n - 1 {
                    let (t0, t1) = (self.times[i], self.times[i + 1]);
                    for s in 0..SPLINE_STEPS {
                        let f = s as f64 / SPLINE_STEPS as f64;
                        out.push((t0 + (t1 - t0) * f, lerp(&self.values[i], &self.values[i + 1], bezier(self.splines[i], f))));
                    }
                }
                out.push((1.0, self.values[n - 1].clone()));
            }
            Calc::Discrete => {
                let eps = EPS / dur;
                for i in 0..n {
                    let start = self.times[i];
                    let stop = if i + 1 < n { self.times[i + 1] } else { 1.0 };
                    out.push((start, self.values[i].clone()));
                    let end = (stop - eps).max(start);
                    if end > start {
                        out.push((end, self.values[i].clone()));
                    }
                }
                if out.last().is_some_and(|p| p.0 < 1.0) {
                    out.push((1.0, self.values[n - 1].clone()));
                }
            }
        }
        out
    }
}

fn eval(poly: &[(f64, Vec<f64>)], u: f64) -> Vec<f64> {
    if u <= poly[0].0 {
        return poly[0].1.clone();
    }
    for w in poly.windows(2) {
        if u <= w[1].0 {
            let span = w[1].0 - w[0].0;
            return if span <= 0.0 { w[1].1.clone() } else { lerp(&w[0].1, &w[1].1, (u - w[0].0) / span) };
        }
    }
    poly[poly.len() - 1].1.clone()
}

/// Points on the scene timeline `0..=total` that, joined by straight lines and
/// held beyond the ends, give the value of the animation at every moment.
///
/// `base` is the value without the animation (before it begins, and after it
/// ends unless it freezes). An animation that repeats forever has no start
/// that matters on a looping scene, so its `begin` only shifts the phase, and
/// the loop closes without a seam.
pub(crate) fn bake(curve: &Curve, timing: &Timing, base: &[f64], total: f64) -> Result<Vec<(f64, Vec<f64>)>, String> {
    let dur = timing.dur;
    let poly = curve.polyline(dur);
    let periodic = timing.active.is_none();
    let first = if periodic { timing.begin.rem_euclid(dur) - dur } else { timing.begin };
    let stop = timing.end().min(total);

    let iterations = ((stop - first.max(0.0)) / dur).ceil().max(0.0);
    if iterations * poly.len() as f64 > MAX_POINTS as f64 {
        return Err(format!("it needs more than {MAX_POINTS} keyframes on a {total}s timeline"));
    }

    let mut pts: Vec<(f64, Vec<f64>)> = Vec::new();
    if !periodic && first > EPS {
        let before = if timing.fill_before { poly[0].1.clone() } else { base.to_vec() };
        pts.push((0.0, before.clone()));
        pts.push((first - EPS, before));
    }
    let mut a = first;
    while a < stop - 1e-12 {
        let b = a + dur;
        let (lo, hi) = (a.max(0.0), b.min(stop));
        if hi > lo + 1e-12 {
            let (ulo, uhi) = ((lo - a) / dur, (hi - a) / dur);
            // Another iteration follows: end this one a hair early so the next
            // starts at exactly `b` and the jump back is a short ramp.
            let continues = b < stop - 1e-9;
            // The early end carries the value the animation has at that moment, so
            // the stretch stays on its line.
            let uend = if continues { uhi - EPS / dur } else { uhi };
            pts.push((lo, eval(&poly, ulo)));
            for (u, v) in &poly {
                if *u > ulo + 1e-12 && *u < uend - 1e-12 {
                    pts.push((a + u * dur, v.clone()));
                }
            }
            pts.push((if continues { hi - EPS } else { hi }, eval(&poly, uend)));
        }
        a = b;
    }
    if !periodic && !timing.freeze && timing.end() + EPS <= total + 1e-9 && !pts.is_empty() {
        pts.push((timing.end() + EPS, base.to_vec()));
    }

    let mut out: Vec<(f64, Vec<f64>)> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last().is_none_or(|l| p.0 > l.0 + 1e-12) {
            out.push(p);
        }
    }
    Ok(out)
}

/// What the planner needs to know about one animation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Span {
    pub period: f64,
    pub indefinite: bool,
    /// For a finite animation, where it stops.
    pub end: f64,
    /// A finite animation that goes back to the value without it after its
    /// end. The scene has to last a moment longer for the last frame to show that.
    pub removes: bool,
}

/// The scene timeline that holds every animation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    pub duration: f64,
    pub looping: bool,
    pub notes: Vec<String>,
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Picks `duration` and the loop mode. Without a repeating animation the scene
/// plays once and lasts until the last one ends. With one, the scene loops at
/// the least common multiple of the periods (to the millisecond), and finite
/// animations play again on every loop.
pub(crate) fn plan(spans: &[Span]) -> Plan {
    let mut notes = Vec::new();
    if spans.is_empty() {
        return Plan { duration: 0.0, looping: false, notes };
    }
    // A play-once scene lasts a moment longer for the return to the base value.
    // A looping one is back at the start anyway.
    let finite_end = |room: bool| spans.iter().filter(|s| !s.indefinite).map(|s| if room && s.removes { s.end + EPS } else { s.end }).fold(0.0, f64::max);
    let periods: Vec<u64> = spans.iter().filter(|s| s.indefinite).map(|s| ((s.period * 1000.0).round() as u64).max(1)).collect();
    if periods.is_empty() {
        return Plan { duration: finite_end(true), looping: false, notes };
    }
    let cap = (MAX_LOOP_SECS * 1000.0) as u64;
    let mut lcm: u64 = 1;
    let mut over = false;
    for p in &periods {
        lcm = lcm / gcd(lcm, *p) * p;
        if lcm > cap {
            over = true;
            break;
        }
    }
    let mut ms = if over {
        let longest = periods.iter().copied().max().unwrap_or(1);
        notes.push(format!("the repeating animations do not share a loop shorter than {MAX_LOOP_SECS}s; the scene loops at {}s and the others drift", longest as f64 / 1000.0));
        longest
    } else {
        lcm
    };
    let finite_end = finite_end(false);
    let need = (finite_end * 1000.0).ceil() as u64;
    if need > ms {
        let rounded = need.div_ceil(ms) * ms;
        if rounded <= cap {
            ms = rounded;
        } else {
            notes.push(format!("an animation that runs until {finite_end}s is longer than the loop and is cut at {}s", ms as f64 / 1000.0));
        }
    }
    Plan { duration: ms as f64 / 1000.0, looping: true, notes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn timing(pairs: &[(&str, &str)]) -> Result<Timing, String> {
        let mut notes = Vec::new();
        Timing::parse(|k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| *v), &mut notes)
    }

    fn lin(vals: &[f64]) -> Curve {
        Curve::new(vals.iter().map(|v| vec![*v]).collect(), None, None, None).unwrap()
    }

    #[test]
    fn clock_values() {
        assert_eq!(parse_clock("3s"), Some(3.0));
        assert_eq!(parse_clock(" 500ms "), Some(0.5));
        assert_eq!(parse_clock("1.5min"), Some(90.0));
        assert_eq!(parse_clock("2h"), Some(7200.0));
        assert_eq!(parse_clock("2"), Some(2.0));
        assert_eq!(parse_clock(".25s"), Some(0.25));
        assert_eq!(parse_clock("1:30"), Some(90.0));
        assert_eq!(parse_clock("01:01:30.5"), Some(3690.5));
        assert_eq!(parse_clock("-1s"), Some(-1.0));
        assert_eq!(parse_clock("click"), None);
        assert_eq!(parse_clock("indefinite"), None);
        assert_eq!(parse_clock("inf"), None);
        assert_eq!(parse_clock("1:2:3:4"), None);
        assert_eq!(parse_clock(""), None);
    }

    #[test]
    fn begin_lists() {
        assert_eq!(parse_begin(None), Ok(0.0));
        assert_eq!(parse_begin(Some("0.5s")), Ok(0.5));
        assert_eq!(parse_begin(Some("2s; 1s")), Ok(1.0));
        assert_eq!(parse_begin(Some("1s;click")), Ok(1.0));
        assert!(parse_begin(Some("click")).is_err());
        assert!(parse_begin(Some("a.end+1s")).is_err());
        assert!(parse_begin(Some("indefinite")).is_err());
        assert_eq!(parse_begin(Some("-2s")), Ok(-2.0));
    }

    #[test]
    fn timing_defaults_and_repeats() {
        let t = timing(&[("dur", "2s")]).unwrap();
        assert_eq!((t.begin, t.dur, t.active, t.freeze), (0.0, 2.0, Some(2.0), false));
        let t = timing(&[("dur", "2s"), ("repeatCount", "3")]).unwrap();
        assert_eq!(t.active, Some(6.0));
        let t = timing(&[("dur", "2s"), ("repeatCount", "indefinite")]).unwrap();
        assert_eq!(t.active, None);
        let t = timing(&[("dur", "2s"), ("repeatDur", "5s")]).unwrap();
        assert_eq!(t.active, Some(5.0));
        let t = timing(&[("dur", "2s"), ("repeatCount", "10"), ("repeatDur", "5s")]).unwrap();
        assert_eq!(t.active, Some(5.0));
        let t = timing(&[("dur", "2s"), ("repeatCount", "indefinite"), ("repeatDur", "5s")]).unwrap();
        assert_eq!(t.active, Some(5.0));
        let t = timing(&[("dur", "2s"), ("repeatDur", "indefinite")]).unwrap();
        assert_eq!(t.active, None);
        let t = timing(&[("dur", "2s"), ("repeatCount", "indefinite"), ("end", "7s"), ("begin", "1s")]).unwrap();
        assert_eq!(t.active, Some(6.0));
        let t = timing(&[("dur", "1s"), ("fill", "freeze")]).unwrap();
        assert!(t.freeze);
    }

    #[test]
    fn timing_errors() {
        assert!(timing(&[]).is_err());
        assert!(timing(&[("dur", "indefinite")]).is_err());
        assert!(timing(&[("dur", "0s")]).is_err());
        assert!(timing(&[("dur", "1ms")]).is_err());
        assert!(timing(&[("dur", "1s"), ("repeatCount", "0")]).is_err());
        assert!(timing(&[("dur", "1s"), ("begin", "click")]).is_err());
        assert!(timing(&[("dur", "1s"), ("begin", "2s"), ("end", "1s")]).is_err());
    }

    #[test]
    fn unresolvable_end_is_noted_not_fatal() {
        let mut notes = Vec::new();
        let pairs = [("dur", "1s"), ("end", "other.end")];
        let t = Timing::parse(|k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| *v), &mut notes).unwrap();
        assert_eq!(t.active, Some(1.0));
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn curve_validation() {
        let v = vec![vec![0.0], vec![1.0], vec![2.0]];
        assert!(Curve::new(v.clone(), Some("0;0.5;1"), None, None).is_ok());
        assert!(Curve::new(v.clone(), Some("0;0.5;1;"), None, None).is_ok());
        assert!(Curve::new(v.clone(), Some("0;1"), None, None).is_err());
        assert!(Curve::new(v.clone(), Some("0.1;0.5;1"), None, None).is_err());
        assert!(Curve::new(v.clone(), Some("0;0.5;0.9"), None, None).is_err());
        assert!(Curve::new(v.clone(), Some("0;0.7;0.5"), None, None).is_err());
        assert!(Curve::new(v.clone(), Some("0;0.5;0.9"), Some("discrete"), None).is_ok());
        assert!(Curve::new(v.clone(), None, Some("bogus"), None).is_err());
        assert!(Curve::new(v.clone(), None, Some("spline"), None).is_err());
        assert!(Curve::new(v.clone(), None, Some("spline"), Some("0.5 0 0.5 1")).is_err());
        assert!(Curve::new(v.clone(), None, Some("spline"), Some("0.5 0 0.5 1; 0.5 0 0.5 1")).is_ok());
        assert!(Curve::new(v.clone(), None, Some("spline"), Some("1.5 0 0.5 1; 0.5 0 0.5 1")).is_err());
        assert!(Curve::new(vec![vec![0.0], vec![1.0, 2.0]], None, None, None).is_err());
        assert!(Curve::new(vec![], None, None, None).is_err());
    }

    #[test]
    fn paced_times_follow_distance() {
        let c = Curve::new(vec![vec![0.0], vec![1.0], vec![4.0]], None, Some("paced"), None).unwrap();
        assert!(close(c.times[1], 0.25) && close(c.times[2], 1.0));
        let flat = Curve::new(vec![vec![2.0], vec![2.0], vec![2.0]], None, Some("paced"), None).unwrap();
        assert!(close(flat.times[1], 0.5));
    }

    #[test]
    fn bezier_endpoints_and_linear_case() {
        for x in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!((bezier([0.0, 0.0, 1.0, 1.0], x) - x).abs() < 1e-6);
        }
        let ease = [0.42, 0.0, 0.58, 1.0];
        assert!(bezier(ease, 0.0).abs() < 1e-6 && (bezier(ease, 1.0) - 1.0).abs() < 1e-6);
        assert!((bezier(ease, 0.5) - 0.5).abs() < 1e-6);
        assert!(bezier(ease, 0.2) < 0.2);
    }

    #[test]
    fn spinner_one_iteration_is_two_points() {
        let t = timing(&[("dur", "2s"), ("repeatCount", "indefinite")]).unwrap();
        let pts = bake(&lin(&[0.0, 360.0]), &t, &[0.0], 2.0).unwrap();
        assert_eq!(pts.len(), 2);
        assert!(close(pts[0].0, 0.0) && close(pts[0].1[0], 0.0));
        assert!(close(pts[1].0, 2.0) && close(pts[1].1[0], 360.0));
    }

    #[test]
    fn repeats_unroll_with_a_short_ramp_at_the_seam() {
        let t = timing(&[("dur", "1s"), ("repeatCount", "2")]).unwrap();
        let pts = bake(&lin(&[0.0, 360.0]), &t, &[0.0], 2.0).unwrap();
        let times: Vec<f64> = pts.iter().map(|p| p.0).collect();
        assert_eq!(pts.len(), 4);
        assert!(close(times[1], 1.0 - EPS) && close(times[2], 1.0) && close(times[3], 2.0));
        assert!(close(pts[1].1[0], 360.0 * (1.0 - EPS)) && close(pts[2].1[0], 0.0));
    }

    #[test]
    fn delayed_finite_animation_holds_the_base_first_and_goes_back_after() {
        let t = timing(&[("dur", "2s"), ("begin", "1s")]).unwrap();
        let pts = bake(&lin(&[10.0, 20.0]), &t, &[5.0], 5.0).unwrap();
        let want = [(0.0, 5.0), (1.0 - EPS, 5.0), (1.0, 10.0), (3.0, 20.0), (3.0 + EPS, 5.0)];
        assert_eq!(pts.len(), want.len());
        for (p, w) in pts.iter().zip(want) {
            assert!(close(p.0, w.0) && close(p.1[0], w.1), "{p:?} vs {w:?}");
        }
    }

    #[test]
    fn a_backwards_fill_holds_the_first_value_during_the_delay() {
        let mut t = timing(&[("dur", "2s"), ("begin", "1s")]).unwrap();
        t.fill_before = true;
        let pts = bake(&lin(&[10.0, 20.0]), &t, &[5.0], 5.0).unwrap();
        assert!(close(pts[0].1[0], 10.0) && close(pts[1].1[0], 10.0), "{pts:?}");
        t.fill_before = false;
        let pts = bake(&lin(&[10.0, 20.0]), &t, &[5.0], 5.0).unwrap();
        assert!(close(pts[0].1[0], 5.0));
    }

    #[test]
    fn freeze_keeps_the_last_value() {
        let t = timing(&[("dur", "2s"), ("fill", "freeze")]).unwrap();
        let pts = bake(&lin(&[10.0, 20.0]), &t, &[5.0], 5.0).unwrap();
        assert_eq!(pts.len(), 2);
        assert!(close(pts[1].1[0], 20.0));
    }

    #[test]
    fn repeating_animation_with_a_delay_becomes_a_phase_shift() {
        let t = timing(&[("dur", "2s"), ("begin", "0.5s"), ("repeatCount", "indefinite")]).unwrap();
        let pts = bake(&lin(&[0.0, 360.0]), &t, &[0.0], 2.0).unwrap();
        // At 0 the animation is 1.5s into its cycle (270), reaches 360 at 0.5s,
        // starts again at 0 and is back at 270 when the loop closes.
        assert!(close(pts[0].0, 0.0) && close(pts[0].1[0], 270.0));
        let last = pts.last().unwrap();
        assert!(close(last.0, 2.0) && close(last.1[0], 270.0));
        assert!(pts.iter().any(|p| close(p.0, 0.5) && close(p.1[0], 0.0)));
        assert!(pts.iter().any(|p| close(p.0, 0.5 - EPS) && close(p.1[0], 360.0 * (1.0 - EPS / 2.0))));
    }

    #[test]
    fn negative_begin_starts_part_way_in() {
        let t = timing(&[("dur", "4s"), ("begin", "-1s")]).unwrap();
        let pts = bake(&lin(&[0.0, 8.0]), &t, &[0.0], 3.0).unwrap();
        assert!(close(pts[0].0, 0.0) && close(pts[0].1[0], 2.0));
        let last = pts.last().unwrap();
        assert!(close(last.0, 3.0) && close(last.1[0], 8.0));
    }

    #[test]
    fn discrete_makes_steps() {
        let c = Curve::new(vec![vec![1.0], vec![2.0], vec![3.0]], None, Some("discrete"), None).unwrap();
        let t = timing(&[("dur", "3s"), ("fill", "freeze")]).unwrap();
        let pts = bake(&c, &t, &[0.0], 3.0).unwrap();
        let at = |time: f64| {
            let poly: Vec<(f64, Vec<f64>)> = pts.clone();
            eval(&poly, time)[0]
        };
        assert!(close(at(0.5), 1.0) && close(at(1.5), 2.0) && close(at(2.5), 3.0));
        assert!(close(at(0.9985), 1.0));
        assert!(close(at(1.002), 2.0));
    }

    #[test]
    fn spline_follows_the_ease_curve() {
        let c = Curve::new(vec![vec![0.0], vec![100.0]], None, Some("spline"), Some("0.42 0 0.58 1")).unwrap();
        let t = timing(&[("dur", "1s"), ("fill", "freeze")]).unwrap();
        let pts = bake(&c, &t, &[0.0], 1.0).unwrap();
        assert_eq!(pts.len(), SPLINE_STEPS + 1);
        let mid = eval(&pts, 0.5)[0];
        assert!((mid - 50.0).abs() < 0.5);
        assert!(eval(&pts, 0.1)[0] < 10.0);
        assert!(eval(&pts, 0.9)[0] > 90.0);
        assert!(pts.windows(2).all(|w| w[1].1[0] >= w[0].1[0]));
    }

    #[test]
    fn components_move_together() {
        let c = Curve::new(vec![vec![0.0, 10.0], vec![100.0, 20.0]], None, None, None).unwrap();
        let t = timing(&[("dur", "1s"), ("fill", "freeze")]).unwrap();
        let pts = bake(&c, &t, &[0.0, 0.0], 1.0).unwrap();
        assert_eq!(pts[1].1, vec![100.0, 20.0]);
    }

    #[test]
    fn too_many_keyframes_is_an_error() {
        let t = timing(&[("dur", "0.01s"), ("repeatCount", "indefinite")]).unwrap();
        assert!(bake(&lin(&[0.0, 1.0, 0.0, 1.0]), &t, &[0.0], 60.0).is_err());
    }

    #[test]
    fn times_strictly_rise() {
        let t = timing(&[("dur", "0.5s"), ("repeatCount", "indefinite")]).unwrap();
        let pts = bake(&lin(&[0.0, 1.0, 0.0]), &t, &[0.0], 3.0).unwrap();
        assert!(pts.windows(2).all(|w| w[1].0 > w[0].0));
    }

    #[test]
    fn plan_without_repeats_plays_once() {
        let p = plan(&[Span { period: 1.0, indefinite: false, end: 3.0, removes: false }, Span { period: 1.0, indefinite: false, end: 2.0, removes: false }]);
        assert_eq!(p, Plan { duration: 3.0, looping: false, notes: vec![] });
        assert_eq!(plan(&[]).duration, 0.0);
    }

    #[test]
    fn plan_leaves_a_moment_for_the_return_to_the_base_value() {
        let p = plan(&[Span { period: 1.0, indefinite: false, end: 3.0, removes: true }, Span { period: 1.0, indefinite: false, end: 2.0, removes: false }]);
        assert!(close(p.duration, 3.0 + EPS) && !p.looping);
        let frozen = plan(&[Span { period: 1.0, indefinite: false, end: 3.0, removes: false }]);
        assert!(close(frozen.duration, 3.0));
    }

    #[test]
    fn a_finite_animation_without_fill_ends_on_the_base_value_on_the_last_frame() {
        let t = timing(&[("dur", "2s"), ("begin", "1s")]).unwrap();
        let total = plan(&[Span { period: 2.0, indefinite: false, end: t.end(), removes: true }]).duration;
        let pts = bake(&lin(&[10.0, 20.0]), &t, &[5.0], total).unwrap();
        let last = pts.last().unwrap();
        assert!(close(last.0, 3.0 + EPS) && close(last.1[0], 5.0), "{last:?}");
    }

    #[test]
    fn plan_loops_at_the_common_period() {
        let p = plan(&[Span { period: 1.0, indefinite: true, end: f64::INFINITY, removes: false }, Span { period: 1.5, indefinite: true, end: f64::INFINITY, removes: false }]);
        assert!(p.looping && close(p.duration, 3.0) && p.notes.is_empty());
        let one = plan(&[Span { period: 1.2, indefinite: true, end: f64::INFINITY, removes: false }]);
        assert!(close(one.duration, 1.2));
    }

    #[test]
    fn a_looping_scene_does_not_grow_for_the_return_to_the_base_value() {
        let p = plan(&[Span { period: 2.0, indefinite: true, end: f64::INFINITY, removes: false }, Span { period: 2.0, indefinite: false, end: 2.0, removes: true }]);
        assert!(p.looping && close(p.duration, 2.0), "{p:?}");
    }

    #[test]
    fn plan_stretches_the_loop_over_a_long_finite_animation() {
        let p = plan(&[Span { period: 2.0, indefinite: true, end: f64::INFINITY, removes: false }, Span { period: 5.0, indefinite: false, end: 5.0, removes: false }]);
        assert!(p.looping && close(p.duration, 6.0));
    }

    #[test]
    fn plan_caps_unrelated_periods() {
        let p = plan(&[Span { period: 1.013, indefinite: true, end: f64::INFINITY, removes: false }, Span { period: 1.017, indefinite: true, end: f64::INFINITY, removes: false }, Span { period: 1.019, indefinite: true, end: f64::INFINITY, removes: false }]);
        assert!(p.looping && close(p.duration, 1.019));
        assert_eq!(p.notes.len(), 1);
    }
}
