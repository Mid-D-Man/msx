// core/msx-ast/src/path.rs
//! SVG path command types, d-string parsing, and d-string serialization.
//!
//! Binary encode/decode lives in msx-binary, NOT here, to keep msx-ast
//! dependency-free. The `d_raw` field on Path caches the original string
//! for roundtrip fidelity.

use crate::primitives::{fmt_f64, Point};

/// All SVG path commands (absolute and relative).
#[derive(Debug, Clone, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    HLineTo(f64),
    VLineTo(f64),
    CubicBezier  { c1: Point, c2: Point, to: Point },
    SmoothCubic  { c2: Point, to: Point },
    QuadBezier   { c: Point, to: Point },
    SmoothQuad   { to: Point },
    Arc          { rx: f64, ry: f64, x_rotation: f64, large_arc: bool, sweep: bool, to: Point },
    // Relative variants
    RelMoveTo(Point),
    RelLineTo(Point),
    RelHLineTo(f64),
    RelVLineTo(f64),
    RelCubicBezier  { c1: Point, c2: Point, to: Point },
    RelSmoothCubic  { c2: Point, to: Point },
    RelQuadBezier   { c: Point, to: Point },
    RelSmoothQuad   { to: Point },
    RelArc          { rx: f64, ry: f64, x_rotation: f64, large_arc: bool, sweep: bool, to: Point },
    ClosePath,
}

impl PathCommand {
    pub fn to_svg_token(&self) -> String {
        let p = |pt: &Point| format!("{} {}", fmt_f64(pt.x), fmt_f64(pt.y));
        match self {
            PathCommand::MoveTo(pt)  => format!("M {}", p(pt)),
            PathCommand::LineTo(pt)  => format!("L {}", p(pt)),
            PathCommand::HLineTo(x)  => format!("H {}", fmt_f64(*x)),
            PathCommand::VLineTo(y)  => format!("V {}", fmt_f64(*y)),
            PathCommand::CubicBezier  { c1, c2, to } => format!("C {} {} {}", p(c1), p(c2), p(to)),
            PathCommand::SmoothCubic  { c2, to }      => format!("S {} {}", p(c2), p(to)),
            PathCommand::QuadBezier   { c, to }        => format!("Q {} {}", p(c), p(to)),
            PathCommand::SmoothQuad   { to }           => format!("T {}", p(to)),
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } =>
                format!("A {} {} {} {} {} {}", fmt_f64(*rx), fmt_f64(*ry), fmt_f64(*x_rotation), *large_arc as u8, *sweep as u8, p(to)),
            PathCommand::RelMoveTo(pt)  => format!("m {}", p(pt)),
            PathCommand::RelLineTo(pt)  => format!("l {}", p(pt)),
            PathCommand::RelHLineTo(x)  => format!("h {}", fmt_f64(*x)),
            PathCommand::RelVLineTo(y)  => format!("v {}", fmt_f64(*y)),
            PathCommand::RelCubicBezier  { c1, c2, to } => format!("c {} {} {}", p(c1), p(c2), p(to)),
            PathCommand::RelSmoothCubic  { c2, to }      => format!("s {} {}", p(c2), p(to)),
            PathCommand::RelQuadBezier   { c, to }        => format!("q {} {}", p(c), p(to)),
            PathCommand::RelSmoothQuad   { to }           => format!("t {}", p(to)),
            PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } =>
                format!("a {} {} {} {} {} {}", fmt_f64(*rx), fmt_f64(*ry), fmt_f64(*x_rotation), *large_arc as u8, *sweep as u8, p(to)),
            PathCommand::ClosePath => "Z".to_string(),
        }
    }
}

/// Serialize commands to SVG `d` string.
pub fn commands_to_d(cmds: &[PathCommand]) -> String {
    cmds.iter().map(|c| c.to_svg_token()).collect::<Vec<_>>().join(" ")
}

/// Parse SVG `d` string into path commands.
pub fn parse_d(d: &str) -> Result<Vec<PathCommand>, String> {
    let (cmds, err) = parse_d_partial(d);
    match err {
        Some(e) => Err(e),
        None => Ok(cmds),
    }
}

/// Lenient form of [`parse_d`]: returns every command parsed before the
/// first error, plus that error. SVG renders a path up to its first
/// error, so an importer wants the commands that did parse.
pub fn parse_d_partial(d: &str) -> (Vec<PathCommand>, Option<String>) {
    let mut cmds = Vec::new();
    let err = parse_into(d, &mut cmds).err();
    (cmds, err)
}

fn parse_into(d: &str, cmds: &mut Vec<PathCommand>) -> Result<(), String> {
    let mut tok = Tokenizer::new(d);

    while let Some(letter) = tok.next_letter() {
        match letter {
            // Extra coordinate pairs after a moveto are implicit linetos.
            'M' => {
                let (x, y) = tok.xy()?;
                cmds.push(PathCommand::MoveTo(Point::new(x, y)));
                while tok.has_number() { let (x, y) = tok.xy()?; cmds.push(PathCommand::LineTo(Point::new(x, y))); }
            }
            'm' => {
                let (x, y) = tok.xy()?;
                cmds.push(PathCommand::RelMoveTo(Point::new(x, y)));
                while tok.has_number() { let (x, y) = tok.xy()?; cmds.push(PathCommand::RelLineTo(Point::new(x, y))); }
            }
            'L' => loop { let (x,y) = tok.xy()?; cmds.push(PathCommand::LineTo(Point::new(x,y))); if !tok.has_number() { break; } },
            'l' => loop { let (x,y) = tok.xy()?; cmds.push(PathCommand::RelLineTo(Point::new(x,y))); if !tok.has_number() { break; } },
            'H' => loop { cmds.push(PathCommand::HLineTo(tok.f()?)); if !tok.has_number() { break; } },
            'h' => loop { cmds.push(PathCommand::RelHLineTo(tok.f()?)); if !tok.has_number() { break; } },
            'V' => loop { cmds.push(PathCommand::VLineTo(tok.f()?)); if !tok.has_number() { break; } },
            'v' => loop { cmds.push(PathCommand::RelVLineTo(tok.f()?)); if !tok.has_number() { break; } },
            'C' => loop {
                let (c1,c2,to) = (Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?));
                cmds.push(PathCommand::CubicBezier { c1, c2, to }); if !tok.has_number() { break; }
            },
            'c' => loop {
                let (c1,c2,to) = (Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?));
                cmds.push(PathCommand::RelCubicBezier { c1, c2, to }); if !tok.has_number() { break; }
            },
            'S' => loop { let (c2,to) = (Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?)); cmds.push(PathCommand::SmoothCubic { c2, to }); if !tok.has_number() { break; } },
            's' => loop { let (c2,to) = (Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?)); cmds.push(PathCommand::RelSmoothCubic { c2, to }); if !tok.has_number() { break; } },
            'Q' => loop { let (c,to) = (Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?)); cmds.push(PathCommand::QuadBezier { c, to }); if !tok.has_number() { break; } },
            'q' => loop { let (c,to) = (Point::new(tok.f()?,tok.f()?), Point::new(tok.f()?,tok.f()?)); cmds.push(PathCommand::RelQuadBezier { c, to }); if !tok.has_number() { break; } },
            'T' => loop { let (x,y) = tok.xy()?; cmds.push(PathCommand::SmoothQuad { to: Point::new(x,y) }); if !tok.has_number() { break; } },
            't' => loop { let (x,y) = tok.xy()?; cmds.push(PathCommand::RelSmoothQuad { to: Point::new(x,y) }); if !tok.has_number() { break; } },
            'A' => loop { cmds.push(parse_arc(&mut tok, false)?); if !tok.has_number() { break; } },
            'a' => loop { cmds.push(parse_arc(&mut tok, true)?);  if !tok.has_number() { break; } },
            'Z' | 'z' => cmds.push(PathCommand::ClosePath),
            other => return Err(format!("unknown path command '{}'", other)),
        }
    }
    Ok(())
}

fn parse_arc(tok: &mut Tokenizer, relative: bool) -> Result<PathCommand, String> {
    let (rx, ry, xr) = (tok.f()?, tok.f()?, tok.f()?);
    let large_arc    = tok.flag()?;
    let sweep        = tok.flag()?;
    let to           = Point::new(tok.f()?, tok.f()?);
    if relative {
        Ok(PathCommand::RelArc { rx, ry, x_rotation: xr, large_arc, sweep, to })
    } else {
        Ok(PathCommand::Arc { rx, ry, x_rotation: xr, large_arc, sweep, to })
    }
}

struct Tokenizer<'a> { src: &'a [u8], cur: usize }

impl<'a> Tokenizer<'a> {
    fn new(s: &'a str) -> Self { Tokenizer { src: s.as_bytes(), cur: 0 } }

    fn skip(&mut self) {
        while self.cur < self.src.len() && matches!(self.src[self.cur], b' ' | b'\t' | b'\r' | b'\n' | b',') { self.cur += 1; }
    }

    fn next_letter(&mut self) -> Option<char> {
        self.skip();
        if self.cur >= self.src.len() { return None; }
        let b = self.src[self.cur] as char;
        if b.is_ascii_alphabetic() { self.cur += 1; Some(b) } else { None }
    }

    /// One number: optional sign, digits with at most one decimal point,
    /// optional exponent. A second `.` starts the next number, so `0.5.5`
    /// is two numbers, as the SVG grammar requires.
    fn f(&mut self) -> Result<f64, String> {
        self.skip();
        let start = self.cur;
        let at = |i: usize| self.src.get(i).copied();
        let mut i = self.cur;
        if matches!(at(i), Some(b'-') | Some(b'+')) { i += 1; }
        let mut digits = 0;
        while matches!(at(i), Some(b'0'..=b'9')) { i += 1; digits += 1; }
        if at(i) == Some(b'.') {
            i += 1;
            while matches!(at(i), Some(b'0'..=b'9')) { i += 1; digits += 1; }
        }
        if digits == 0 { return Err(format!("expected number at byte {}", start)); }
        if matches!(at(i), Some(b'e') | Some(b'E')) {
            let mut j = i + 1;
            if matches!(at(j), Some(b'-') | Some(b'+')) { j += 1; }
            if matches!(at(j), Some(b'0'..=b'9')) {
                while matches!(at(j), Some(b'0'..=b'9')) { j += 1; }
                i = j;
            }
        }
        self.cur = i;
        std::str::from_utf8(&self.src[start..i]).ok()
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| format!("expected number at byte {}", start))
    }

    fn xy(&mut self) -> Result<(f64, f64), String> { Ok((self.f()?, self.f()?)) }

    fn flag(&mut self) -> Result<bool, String> {
        self.skip();
        match self.src.get(self.cur) {
            Some(b'0') => { self.cur += 1; Ok(false) }
            Some(b'1') => { self.cur += 1; Ok(true)  }
            _ => Err("expected flag 0 or 1".into()),
        }
    }

    fn has_number(&mut self) -> bool {
        self.skip();
        self.cur < self.src.len() && matches!(self.src[self.cur], b'0'..=b'9' | b'-' | b'+' | b'.')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle() {
        let cmds = parse_d("M 50 450 L 250 50 L 450 450 Z").unwrap();
        assert_eq!(cmds.len(), 4);
        assert!(matches!(cmds[0], PathCommand::MoveTo(_)));
        assert!(matches!(cmds[3], PathCommand::ClosePath));
    }

    #[test]
    fn arc_flags() {
        let cmds = parse_d("M 120 380 A 130 130 0 0 1 380 380").unwrap();
        if let PathCommand::Arc { rx, large_arc, sweep, .. } = &cmds[1] {
            assert!((rx - 130.0).abs() < 1e-4);
            assert!(!large_arc);
            assert!(*sweep);
        }
    }

    #[test]
    fn second_decimal_point_starts_a_new_number() {
        let cmds = parse_d("M0.5.5 L.5.25").unwrap();
        assert_eq!(cmds, vec![
            PathCommand::MoveTo(Point::new(0.5, 0.5)),
            PathCommand::LineTo(Point::new(0.5, 0.25)),
        ]);
    }

    #[test]
    fn signs_and_exponents_separate_numbers() {
        assert_eq!(parse_d("M1-1").unwrap(), vec![PathCommand::MoveTo(Point::new(1.0, -1.0))]);
        assert_eq!(parse_d("M1e2 3E-1").unwrap(), vec![PathCommand::MoveTo(Point::new(100.0, 0.3))]);
        assert_eq!(parse_d("M+5 +6").unwrap(), vec![PathCommand::MoveTo(Point::new(5.0, 6.0))]);
    }

    #[test]
    fn extra_pairs_after_a_moveto_are_implicit_linetos() {
        let abs = parse_d("M 10 10 20 20 30 30").unwrap();
        assert_eq!(abs, vec![
            PathCommand::MoveTo(Point::new(10.0, 10.0)),
            PathCommand::LineTo(Point::new(20.0, 20.0)),
            PathCommand::LineTo(Point::new(30.0, 30.0)),
        ]);
        let rel = parse_d("m 1 1 2 2").unwrap();
        assert_eq!(rel, vec![
            PathCommand::RelMoveTo(Point::new(1.0, 1.0)),
            PathCommand::RelLineTo(Point::new(2.0, 2.0)),
        ]);
    }

    #[test]
    fn compact_arc_flags_are_single_characters() {
        let cmds = parse_d("M0 0A2 2 0 012 3").unwrap();
        assert_eq!(cmds[1], PathCommand::Arc { rx: 2.0, ry: 2.0, x_rotation: 0.0, large_arc: false, sweep: true, to: Point::new(2.0, 3.0) });
    }

    #[test]
    fn partial_parse_keeps_the_commands_before_the_error() {
        let (cmds, err) = parse_d_partial("M 0 0 L 10 10 L 5");
        assert_eq!(cmds.len(), 2);
        assert!(err.is_some());
        assert!(parse_d("M 0 0 L 10 10 L 5").is_err());

        let (cmds, err) = parse_d_partial("M 0 0 x");
        assert_eq!(cmds.len(), 1);
        assert!(err.unwrap().contains("unknown path command"));

        let (cmds, err) = parse_d_partial("M 0 0 L 1 1");
        assert_eq!(cmds.len(), 2);
        assert!(err.is_none());
    }

    #[test]
    fn d_string_roundtrip() {
        let cmds = vec![
            PathCommand::MoveTo(Point::new(10.0, 20.0)),
            PathCommand::CubicBezier { c1: Point::new(30.0,40.0), c2: Point::new(50.0,60.0), to: Point::new(70.0,80.0) },
            PathCommand::ClosePath,
        ];
        let d    = commands_to_d(&cmds);
        let back = parse_d(&d).unwrap();
        assert_eq!(cmds, back);
    }

    #[test]
    fn relative_preserved() {
        let cmds = parse_d("m 10 20 l 5 5 h 10 v -5 z").unwrap();
        assert!(matches!(cmds[0], PathCommand::RelMoveTo(_)));
        assert!(matches!(cmds[4], PathCommand::ClosePath));
    }
                  }
