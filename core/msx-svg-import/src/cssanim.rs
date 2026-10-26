// core/msx-svg-import/src/cssanim.rs
//! CSS animation as numbers: the `animation` properties, timing functions,
//! transform lists and the direction of a keyframe curve.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Animation". Like `anim.rs`,
//! this file has no dependency on the scene types. The importer reads the
//! `@keyframes` rules and builds the wrappers, `anim.rs` bakes the timing.

use crate::anim::Curve;

pub(crate) const LINEAR: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
const EASE: [f64; 4] = [0.25, 0.1, 0.25, 1.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// One animation of an element, from the `animation` properties.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnimDecl {
    pub name: String,
    pub duration: f64,
    pub delay: f64,
    /// Easing between two keyframes, as the control points of a cubic Bezier.
    pub ease: [f64; 4],
    /// `None` repeats forever.
    pub count: Option<f64>,
    pub direction: Direction,
    pub forwards: bool,
    pub backwards: bool,
    pub paused: bool,
}

impl AnimDecl {
    fn new() -> AnimDecl {
        AnimDecl { name: String::new(), duration: 0.0, delay: 0.0, ease: EASE, count: Some(1.0), direction: Direction::Normal, forwards: false, backwards: false, paused: false }
    }
}

/// A CSS time: `2s`, `500ms`, `.5s`, or a bare `0`.
pub(crate) fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let (digits, scale) = if let Some(r) = s.strip_suffix("ms") {
        (r, 0.001)
    } else if let Some(r) = s.strip_suffix('s') {
        (r, 1.0)
    } else if s == "0" {
        ("0", 1.0)
    } else {
        return None;
    };
    digits.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v * scale)
}

/// The control points of a CSS easing. Step easings have no equivalent in a
/// track of linear keyframes and are an error.
pub(crate) fn parse_timing_function(s: &str) -> Result<[f64; 4], String> {
    let t = s.trim().to_ascii_lowercase();
    match t.as_str() {
        "linear" => Ok(LINEAR),
        "ease" => Ok(EASE),
        "ease-in" => Ok([0.42, 0.0, 1.0, 1.0]),
        "ease-out" => Ok([0.0, 0.0, 0.58, 1.0]),
        "ease-in-out" => Ok([0.42, 0.0, 0.58, 1.0]),
        "step-start" | "step-end" => Err("step easing has no equivalent in a track of linear keyframes".into()),
        _ if t.starts_with("steps(") => Err("step easing has no equivalent in a track of linear keyframes".into()),
        _ if t.starts_with("cubic-bezier(") && t.ends_with(')') => {
            let inner = &t["cubic-bezier(".len()..t.len() - 1];
            let nums: Vec<f64> = inner.split(',').filter_map(|p| p.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect();
            if nums.len() == 4 && inner.split(',').count() == 4 && (0.0..=1.0).contains(&nums[0]) && (0.0..=1.0).contains(&nums[2]) {
                Ok([nums[0], nums[1], nums[2], nums[3]])
            } else {
                Err(format!("\"{}\" is not a cubic-bezier with x values in 0 to 1", s.trim()))
            }
        }
        _ => Err(format!("timing function \"{}\" is not known", s.trim())),
    }
}

/// Splits on `sep` outside parentheses.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim());
    parts
}

/// Splits on whitespace outside parentheses.
fn tokens(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, None::<usize>);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(st) = start.take() {
                parts.push(&s[st..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        parts.push(&s[st..]);
    }
    parts
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s.trim() {
        "normal" => Some(Direction::Normal),
        "reverse" => Some(Direction::Reverse),
        "alternate" => Some(Direction::Alternate),
        "alternate-reverse" => Some(Direction::AlternateReverse),
        _ => None,
    }
}

fn parse_count(s: &str) -> Result<Option<f64>, String> {
    match s.trim() {
        "infinite" => Ok(None),
        v => v.parse::<f64>().ok().filter(|c| c.is_finite() && *c >= 0.0).map(Some).ok_or_else(|| format!("animation-iteration-count \"{v}\" is not a number")),
    }
}

fn shorthand(item: &str) -> Result<AnimDecl, String> {
    let mut d = AnimDecl::new();
    let mut times = 0;
    let mut name: Option<&str> = None;
    for tok in tokens(item) {
        if let Some(t) = parse_time(tok) {
            match times {
                0 => d.duration = t,
                1 => d.delay = t,
                _ => return Err("the animation shorthand has more than two times".into()),
            }
            times += 1;
            continue;
        }
        let lower = tok.to_ascii_lowercase();
        match lower.as_str() {
            "infinite" => d.count = None,
            "forwards" => d.forwards = true,
            "backwards" => d.backwards = true,
            "both" => (d.forwards, d.backwards) = (true, true),
            "none" => {}
            "running" => d.paused = false,
            "paused" => d.paused = true,
            "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end" => d.ease = parse_timing_function(&lower)?,
            _ if lower.starts_with("cubic-bezier(") || lower.starts_with("steps(") => d.ease = parse_timing_function(&lower)?,
            _ if parse_direction(&lower).is_some() => d.direction = parse_direction(&lower).unwrap_or(Direction::Normal),
            _ if tok.parse::<f64>().is_ok() => d.count = parse_count(tok)?,
            _ => {
                if name.is_none() {
                    name = Some(tok);
                }
            }
        }
    }
    d.name = name.unwrap_or("none").to_string();
    Ok(d)
}

fn longhand(d: &mut AnimDecl, prop: &str, value: &str) -> Result<(), String> {
    let v = value.trim();
    match prop {
        "animation-duration" => d.duration = parse_time(v).filter(|t| *t >= 0.0).ok_or_else(|| format!("animation-duration \"{v}\" is not a time"))?,
        "animation-delay" => d.delay = parse_time(v).ok_or_else(|| format!("animation-delay \"{v}\" is not a time"))?,
        "animation-timing-function" => d.ease = parse_timing_function(v)?,
        "animation-iteration-count" => d.count = parse_count(v)?,
        "animation-direction" => d.direction = parse_direction(v).ok_or_else(|| format!("animation-direction \"{v}\" is not known"))?,
        "animation-fill-mode" => match v {
            "none" => (d.forwards, d.backwards) = (false, false),
            "forwards" => (d.forwards, d.backwards) = (true, false),
            "backwards" => (d.forwards, d.backwards) = (false, true),
            "both" => (d.forwards, d.backwards) = (true, true),
            _ => return Err(format!("animation-fill-mode \"{v}\" is not known")),
        },
        "animation-play-state" => d.paused = v == "paused",
        _ => {}
    }
    Ok(())
}

/// The animations of an element, from `animation` and its longhands. Longhands
/// refine the shorthand, and each comma list cycles to the number of names.
/// An entry that cannot be read is an error and the rest still count.
pub(crate) fn parse_animations<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Vec<Result<AnimDecl, String>> {
    let mut decls: Vec<Result<AnimDecl, String>> = match get("animation").map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => split_top(s, ',').into_iter().map(shorthand).collect(),
        None => Vec::new(),
    };
    if let Some(names) = get("animation-name").map(str::trim).filter(|s| !s.is_empty()) {
        let names = split_top(names, ',');
        while decls.len() < names.len() {
            decls.push(Ok(AnimDecl::new()));
        }
        for (i, d) in decls.iter_mut().enumerate() {
            if let Ok(d) = d {
                d.name = names[i % names.len()].to_string();
            }
        }
    }
    for prop in ["animation-duration", "animation-delay", "animation-timing-function", "animation-iteration-count", "animation-direction", "animation-fill-mode", "animation-play-state"] {
        let Some(value) = get(prop).map(str::trim).filter(|s| !s.is_empty()) else { continue };
        let list = split_top(value, ',');
        for (i, d) in decls.iter_mut().enumerate() {
            if let Ok(inner) = d {
                if let Err(why) = longhand(inner, prop, list[i % list.len()]) {
                    *d = Err(why);
                }
            }
        }
    }
    decls.into_iter().filter(|d| !matches!(d, Ok(a) if a.name.is_empty() || a.name == "none")).collect()
}

/// One function of a CSS `transform` list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Tf {
    Translate(f64, f64),
    Scale(f64, f64),
    Rotate(f64),
}

impl Tf {
    fn same_kind(&self, other: &Tf) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    fn identity(&self) -> Tf {
        match self {
            Tf::Translate(..) => Tf::Translate(0.0, 0.0),
            Tf::Scale(..) => Tf::Scale(1.0, 1.0),
            Tf::Rotate(_) => Tf::Rotate(0.0),
        }
    }

    /// The numbers a track interpolates.
    pub(crate) fn comps(&self) -> Vec<f64> {
        match *self {
            Tf::Translate(x, y) | Tf::Scale(x, y) => vec![x, y],
            Tf::Rotate(a) => vec![a],
        }
    }
}

fn length_px(s: &str) -> Result<f64, String> {
    let t = s.trim();
    let digits = t.strip_suffix("px").unwrap_or(t);
    digits.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("\"{t}\" is not a length in px"))
}

fn angle_deg(s: &str) -> Result<f64, String> {
    let t = s.trim().to_ascii_lowercase();
    let (digits, scale) = if let Some(r) = t.strip_suffix("deg") {
        (r, 1.0)
    } else if let Some(r) = t.strip_suffix("grad") {
        (r, 0.9)
    } else if let Some(r) = t.strip_suffix("rad") {
        (r, 180.0 / std::f64::consts::PI)
    } else if let Some(r) = t.strip_suffix("turn") {
        (r, 360.0)
    } else {
        (t.as_str(), 1.0)
    };
    digits.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v * scale).ok_or_else(|| format!("\"{}\" is not an angle", s.trim()))
}

fn plain_number(s: &str) -> Result<f64, String> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("\"{}\" is not a number", s.trim()))
}

/// A CSS `transform` as a list of functions. `none` and an empty value are an
/// empty list. Skews, matrices and anything three dimensional have no
/// animation channel and are an error.
pub(crate) fn parse_transform_list(s: &str) -> Result<Vec<Tf>, String> {
    let mut rest = s.trim();
    let mut out = Vec::new();
    if rest.is_empty() || rest.eq_ignore_ascii_case("none") {
        return Ok(out);
    }
    while !rest.is_empty() {
        let open = rest.find('(').ok_or_else(|| format!("\"{rest}\" is not a transform list"))?;
        let close = rest.find(')').ok_or("a transform function is not closed")?;
        if close < open {
            return Err("a transform function is not closed".into());
        }
        let written = rest[..open].trim();
        let name = written.to_ascii_lowercase();
        let args: Vec<&str> = rest[open + 1..close].split(|c: char| c == ',' || c.is_whitespace()).filter(|a| !a.is_empty()).collect();
        let arg = |i: usize| args.get(i).copied().ok_or_else(|| format!("{name}() is missing an argument"));
        let f = match name.as_str() {
            "translate" => Tf::Translate(length_px(arg(0)?)?, if args.len() > 1 { length_px(arg(1)?)? } else { 0.0 }),
            "translatex" => Tf::Translate(length_px(arg(0)?)?, 0.0),
            "translatey" => Tf::Translate(0.0, length_px(arg(0)?)?),
            "scale" => {
                let x = plain_number(arg(0)?)?;
                Tf::Scale(x, if args.len() > 1 { plain_number(arg(1)?)? } else { x })
            }
            "scalex" => Tf::Scale(plain_number(arg(0)?)?, 1.0),
            "scaley" => Tf::Scale(1.0, plain_number(arg(0)?)?),
            "rotate" | "rotatez" => Tf::Rotate(angle_deg(arg(0)?)?),
            _ => return Err(format!("{written}() has no animation channel")),
        };
        out.push(f);
        rest = rest[close + 1..].trim_start();
    }
    Ok(out)
}

/// Gives every list of the keyframes the same functions in the same order,
/// padding a `none` with the identity of the others. Lists with different
/// functions would need matrix interpolation, which a track cannot do.
pub(crate) fn align(lists: &[Vec<Tf>]) -> Result<Vec<Vec<Tf>>, String> {
    let Some(shape) = lists.iter().find(|l| !l.is_empty()) else { return Ok(vec![Vec::new(); lists.len()]) };
    for l in lists.iter().filter(|l| !l.is_empty()) {
        if l.len() != shape.len() || l.iter().zip(shape.iter()).any(|(a, b)| !a.same_kind(b)) {
            return Err("its transform lists use different functions, which would need matrix interpolation".into());
        }
    }
    Ok(lists.iter().map(|l| if l.is_empty() { shape.iter().map(Tf::identity).collect() } else { l.clone() }).collect())
}

/// The keyframes of one property: offsets in `0..=1` from 0 to 1, a value
/// at each, and the easing of each interval.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stops {
    pub offsets: Vec<f64>,
    pub values: Vec<Vec<f64>>,
    pub eases: Vec<[f64; 4]>,
}

fn mirror(e: [f64; 4]) -> [f64; 4] {
    [1.0 - e[2], 1.0 - e[3], 1.0 - e[0], 1.0 - e[1]]
}

impl Stops {
    /// The same keyframes played backwards. The easing of each interval is
    /// reversed with it.
    fn reversed(&self) -> Stops {
        let n = self.offsets.len();
        Stops {
            offsets: (0..n).map(|k| 1.0 - self.offsets[n - 1 - k]).collect(),
            values: self.values.iter().rev().cloned().collect(),
            eases: (0..n.saturating_sub(1)).map(|k| mirror(self.eases[n - 2 - k])).collect(),
        }
    }

    /// The keyframes of one iteration for a direction, and how many times
    /// longer than the animation duration that iteration is: an alternating
    /// one is a forward and a backward play joined into a single cycle.
    pub(crate) fn orient(&self, dir: Direction) -> (Stops, f64) {
        let join = |a: &Stops, b: &Stops| {
            let n = a.offsets.len();
            let mut offsets: Vec<f64> = a.offsets.iter().map(|o| o / 2.0).collect();
            offsets.extend(b.offsets.iter().skip(1).map(|o| (1.0 + o) / 2.0));
            let mut values = a.values.clone();
            values.extend(b.values.iter().skip(1).cloned());
            let mut eases = a.eases.clone();
            eases.extend(b.eases.iter().copied());
            debug_assert_eq!(offsets.len(), 2 * n - 1);
            Stops { offsets, values, eases }
        };
        match dir {
            Direction::Normal => (self.clone(), 1.0),
            Direction::Reverse => (self.reversed(), 1.0),
            Direction::Alternate => (join(self, &self.reversed()), 2.0),
            Direction::AlternateReverse => (join(&self.reversed(), self), 2.0),
        }
    }

    /// A curve for `anim::bake`. Straight easings stay linear, the others
    /// become `keySplines`.
    pub(crate) fn curve(&self) -> Result<Curve, String> {
        let times = self.offsets.iter().map(|o| o.to_string()).collect::<Vec<_>>().join(";");
        if self.eases.iter().all(|e| *e == LINEAR) {
            return Curve::new(self.values.clone(), Some(&times), Some("linear"), None);
        }
        let splines = self.eases.iter().map(|e| format!("{} {} {} {}", e[0], e[1], e[2], e[3])).collect::<Vec<_>>().join(";");
        Curve::new(self.values.clone(), Some(&times), Some("spline"), Some(&splines))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{bake, Timing};

    fn decls(pairs: &[(&str, &str)]) -> Vec<Result<AnimDecl, String>> {
        parse_animations(|k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| *v))
    }

    fn one(pairs: &[(&str, &str)]) -> AnimDecl {
        let mut d = decls(pairs);
        assert_eq!(d.len(), 1, "{d:?}");
        d.remove(0).expect("one readable animation")
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("2s"), Some(2.0));
        assert_eq!(parse_time(" 500ms "), Some(0.5));
        assert_eq!(parse_time(".25s"), Some(0.25));
        assert_eq!(parse_time("-1s"), Some(-1.0));
        assert_eq!(parse_time("0"), Some(0.0));
        assert_eq!(parse_time("2"), None);
        assert_eq!(parse_time("fast"), None);
    }

    #[test]
    fn timing_functions() {
        assert_eq!(parse_timing_function("linear"), Ok(LINEAR));
        assert_eq!(parse_timing_function("EASE"), Ok(EASE));
        assert_eq!(parse_timing_function("cubic-bezier(0.1, 0.7, 1, 0.1)"), Ok([0.1, 0.7, 1.0, 0.1]));
        assert_eq!(parse_timing_function("cubic-bezier(.4,-1,.6,2)"), Ok([0.4, -1.0, 0.6, 2.0]));
        assert!(parse_timing_function("cubic-bezier(2, 0, 1, 1)").is_err());
        assert!(parse_timing_function("cubic-bezier(0, 0, 1)").is_err());
        assert!(parse_timing_function("steps(4, end)").is_err());
        assert!(parse_timing_function("step-start").is_err());
        assert!(parse_timing_function("wobble").is_err());
    }

    #[test]
    fn shorthand_in_any_order() {
        let d = one(&[("animation", "spin 2s linear infinite")]);
        assert_eq!((d.name.as_str(), d.duration, d.delay, d.ease, d.count), ("spin", 2.0, 0.0, LINEAR, None));
        let d = one(&[("animation", "1.5s ease-in-out 0.5s 3 alternate both fade")]);
        assert_eq!((d.name.as_str(), d.duration, d.delay, d.count, d.direction, d.forwards, d.backwards), ("fade", 1.5, 0.5, Some(3.0), Direction::Alternate, true, true));
        assert_eq!(d.ease, [0.42, 0.0, 0.58, 1.0]);
        let d = one(&[("animation", "bounce 800ms cubic-bezier(0.5, 0, 0.5, 1) reverse forwards")]);
        assert_eq!((d.duration, d.ease, d.direction, d.forwards, d.backwards), (0.8, [0.5, 0.0, 0.5, 1.0], Direction::Reverse, true, false));
        let d = one(&[("animation", "spin 1s paused")]);
        assert!(d.paused);
        assert_eq!(one(&[("animation", "spin 1s")]).count, Some(1.0));
        assert_eq!(one(&[("animation", "spin 1s")]).ease, EASE, "the default easing is ease");
    }

    #[test]
    fn several_animations_in_one_shorthand() {
        let d = decls(&[("animation", "spin 2s linear infinite, fade 1s ease-in 0.5s")]);
        assert_eq!(d.len(), 2);
        let (a, b) = (d[0].clone().unwrap(), d[1].clone().unwrap());
        assert_eq!((a.name.as_str(), a.duration, a.count), ("spin", 2.0, None));
        assert_eq!((b.name.as_str(), b.duration, b.delay, b.count), ("fade", 1.0, 0.5, Some(1.0)));
        let d = decls(&[("animation", "a 1s cubic-bezier(0, 0, 1, 1), b 2s")]);
        assert_eq!(d.len(), 2, "a comma inside cubic-bezier does not split");
    }

    #[test]
    fn longhands() {
        let d = one(&[
            ("animation-name", "wiggle"),
            ("animation-duration", "250ms"),
            ("animation-delay", "-100ms"),
            ("animation-timing-function", "ease-out"),
            ("animation-iteration-count", "infinite"),
            ("animation-direction", "alternate-reverse"),
            ("animation-fill-mode", "backwards"),
        ]);
        assert_eq!(d.name, "wiggle");
        assert!(close(d.duration, 0.25) && close(d.delay, -0.1));
        assert_eq!((d.count, d.direction, d.forwards, d.backwards), (None, Direction::AlternateReverse, false, true));
        assert_eq!(d.ease, [0.0, 0.0, 0.58, 1.0]);
    }

    #[test]
    fn longhands_refine_the_shorthand_and_lists_cycle() {
        let d = decls(&[("animation", "a 1s, b 1s"), ("animation-duration", "3s"), ("animation-name", "x, y, z")]);
        let names: Vec<String> = d.iter().map(|d| d.clone().unwrap().name).collect();
        assert_eq!(names, vec!["x", "y", "z"]);
        assert!(d.iter().all(|d| d.clone().unwrap().duration == 3.0));
    }

    #[test]
    fn none_and_missing_animations_are_dropped_and_bad_ones_are_errors() {
        assert!(decls(&[]).is_empty());
        assert!(decls(&[("animation", "none")]).is_empty());
        assert!(decls(&[("animation-name", "none")]).is_empty());
        let d = decls(&[("animation", "spin 1s steps(4)")]);
        assert!(d.len() == 1 && d[0].is_err());
        let d = decls(&[("animation-name", "a"), ("animation-duration", "soon")]);
        assert!(d[0].is_err());
    }

    #[test]
    fn transform_lists() {
        assert_eq!(parse_transform_list("none"), Ok(vec![]));
        assert_eq!(parse_transform_list(""), Ok(vec![]));
        assert_eq!(parse_transform_list("rotate(90deg)"), Ok(vec![Tf::Rotate(90.0)]));
        assert_eq!(parse_transform_list("translate(10px, 20px) rotate(0.5turn) scale(2)"), Ok(vec![Tf::Translate(10.0, 20.0), Tf::Rotate(180.0), Tf::Scale(2.0, 2.0)]));
        assert_eq!(parse_transform_list("translateX(5px) translateY(-3px) scaleX(2) scaleY(.5)"), Ok(vec![Tf::Translate(5.0, 0.0), Tf::Translate(0.0, -3.0), Tf::Scale(2.0, 1.0), Tf::Scale(1.0, 0.5)]));
        let Ok(l) = parse_transform_list("rotate(3.14159265rad)") else { panic!() };
        assert!(matches!(l[0], Tf::Rotate(a) if (a - 180.0).abs() < 1e-3));
        assert_eq!(parse_transform_list("rotate(100grad)"), Ok(vec![Tf::Rotate(90.0)]));
        assert_eq!(parse_transform_list("TRANSLATE(1px)"), Ok(vec![Tf::Translate(1.0, 0.0)]));
        for bad in ["skewX(10deg)", "matrix(1,0,0,1,0,0)", "rotate3d(1,0,0,45deg)", "translate(10%, 0)", "translate(1em)", "rotate(", "scale(a)", "perspective(5px)"] {
            assert!(parse_transform_list(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn aligning_transform_lists() {
        let a = vec![Tf::Translate(10.0, 0.0), Tf::Rotate(90.0)];
        let l = align(&[vec![], a.clone(), vec![Tf::Translate(0.0, 5.0), Tf::Rotate(0.0)]]).unwrap();
        assert_eq!(l[0], vec![Tf::Translate(0.0, 0.0), Tf::Rotate(0.0)]);
        assert_eq!(l[1], a);
        assert_eq!(align(&[vec![], vec![]]).unwrap(), vec![vec![], vec![]]);
        assert!(align(&[vec![Tf::Rotate(1.0)], vec![Tf::Scale(1.0, 1.0)]]).is_err());
        assert!(align(&[vec![Tf::Rotate(1.0)], vec![Tf::Rotate(1.0), Tf::Rotate(2.0)]]).is_err());
    }

    fn stops(values: &[f64], eases: &[[f64; 4]]) -> Stops {
        let n = values.len();
        Stops { offsets: (0..n).map(|i| i as f64 / (n - 1) as f64).collect(), values: values.iter().map(|v| vec![*v]).collect(), eases: eases.to_vec() }
    }

    fn sample(s: &Stops, dir: Direction, dur: f64, count: Option<f64>, total: f64, times: &[f64]) -> Vec<f64> {
        let (o, factor) = s.orient(dir);
        let curve = o.curve().unwrap();
        let t = Timing { begin: 0.0, dur: dur * factor, active: count.map(|c| c * dur), freeze: true, fill_before: false };
        let pts = bake(&curve, &t, &[0.0], total).unwrap();
        times
            .iter()
            .map(|x| {
                let mut v = pts[0].1[0];
                for w in pts.windows(2) {
                    if *x >= w[0].0 && *x <= w[1].0 {
                        v = w[0].1[0] + (w[1].1[0] - w[0].1[0]) * (x - w[0].0) / (w[1].0 - w[0].0);
                    }
                }
                if *x >= pts[pts.len() - 1].0 {
                    v = pts[pts.len() - 1].1[0];
                }
                v
            })
            .collect()
    }

    #[test]
    fn reverse_plays_the_keyframes_backwards() {
        let s = stops(&[0.0, 100.0], &[LINEAR]);
        let v = sample(&s, Direction::Reverse, 1.0, Some(1.0), 1.0, &[0.0, 0.25, 1.0]);
        assert!(close(v[0], 100.0) && close(v[1], 75.0) && close(v[2], 0.0), "{v:?}");
    }

    #[test]
    fn alternate_goes_there_and_back() {
        let s = stops(&[0.0, 100.0], &[LINEAR]);
        let (o, factor) = s.orient(Direction::Alternate);
        assert_eq!(factor, 2.0);
        assert_eq!(o.offsets, vec![0.0, 0.5, 1.0]);
        assert_eq!(o.values, vec![vec![0.0], vec![100.0], vec![0.0]]);
        let v = sample(&s, Direction::Alternate, 1.0, None, 2.0, &[0.0, 0.5, 1.0, 1.5, 2.0]);
        assert!(close(v[0], 0.0) && close(v[1], 50.0) && close(v[2], 100.0) && close(v[3], 50.0) && close(v[4], 0.0), "{v:?}");
        let r = sample(&s, Direction::AlternateReverse, 1.0, None, 2.0, &[0.0, 1.0, 2.0]);
        assert!(close(r[0], 100.0) && close(r[1], 0.0) && close(r[2], 100.0), "{r:?}");
    }

    #[test]
    fn a_finite_alternation_stops_on_the_right_end() {
        let s = stops(&[0.0, 100.0], &[LINEAR]);
        let three = sample(&s, Direction::Alternate, 1.0, Some(3.0), 4.0, &[2.5, 3.0, 4.0]);
        assert!(close(three[0], 50.0) && close(three[1], 100.0) && close(three[2], 100.0), "{three:?}");
        let two = sample(&s, Direction::Alternate, 1.0, Some(2.0), 4.0, &[1.5, 2.0, 4.0]);
        assert!(close(two[0], 50.0) && close(two[1], 0.0) && close(two[2], 0.0), "{two:?}");
    }

    #[test]
    fn easing_is_reversed_with_the_direction() {
        let ease_in = [0.42, 0.0, 1.0, 1.0];
        let s = stops(&[0.0, 100.0], &[ease_in]);
        let fwd = sample(&s, Direction::Normal, 1.0, Some(1.0), 1.0, &[0.25]);
        let rev = sample(&s, Direction::Reverse, 1.0, Some(1.0), 1.0, &[0.75]);
        assert!(fwd[0] < 25.0, "ease-in starts slow: {fwd:?}");
        // Backwards, the animation runs the same curve mirrored in time: 100 - f(1 - u)
        // with the reversed easing, so after 3/4 of the time it is at the value forward play has at 1/4.
        assert!((rev[0] - fwd[0]).abs() < 0.5, "{rev:?} vs {fwd:?}");
        let m = mirror(ease_in);
        assert!(m.iter().zip([0.0, 0.0, 0.58, 1.0]).all(|(a, b)| (a - b).abs() < 1e-12), "{m:?}");
    }

    #[test]
    fn unequal_easings_per_interval_survive_alternation() {
        let s = stops(&[0.0, 10.0, 0.0], &[[0.42, 0.0, 1.0, 1.0], LINEAR]);
        let (o, _) = s.orient(Direction::Alternate);
        assert_eq!(o.eases.len(), o.offsets.len() - 1);
        assert!(o.curve().is_ok());
    }

    #[test]
    fn linear_stops_make_a_linear_curve_and_others_a_spline() {
        assert!(stops(&[0.0, 1.0], &[LINEAR]).curve().is_ok());
        assert!(stops(&[0.0, 1.0], &[EASE]).curve().is_ok());
    }
}
