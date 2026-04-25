// src/path.rs

use crate::primitives::{fmt_f64, Point};

// ── Binary command tags ───────────────────────────────────────────────────────

pub const CMD_MOVE_TO:       u8 = 0x00;
pub const CMD_LINE_TO:       u8 = 0x01;
pub const CMD_H_LINE_TO:     u8 = 0x02;
pub const CMD_V_LINE_TO:     u8 = 0x03;
pub const CMD_CUBIC:         u8 = 0x04;
pub const CMD_SMOOTH_CUBIC:  u8 = 0x05;
pub const CMD_QUAD:          u8 = 0x06;
pub const CMD_SMOOTH_QUAD:   u8 = 0x07;
pub const CMD_ARC:           u8 = 0x08;
// Relative = absolute + 0x10
pub const CMD_REL_MOVE_TO:      u8 = 0x10;
pub const CMD_REL_LINE_TO:      u8 = 0x11;
pub const CMD_REL_H_LINE_TO:    u8 = 0x12;
pub const CMD_REL_V_LINE_TO:    u8 = 0x13;
pub const CMD_REL_CUBIC:        u8 = 0x14;
pub const CMD_REL_SMOOTH_CUBIC: u8 = 0x15;
pub const CMD_REL_QUAD:         u8 = 0x16;
pub const CMD_REL_SMOOTH_QUAD:  u8 = 0x17;
pub const CMD_REL_ARC:          u8 = 0x18;
pub const CMD_CLOSE:            u8 = 0xFF;

// ── PathCommand enum ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PathCommand {
    // Absolute
    MoveTo(Point),
    LineTo(Point),
    HLineTo(f64),
    VLineTo(f64),
    CubicBezier { c1: Point, c2: Point, to: Point },
    SmoothCubic { c2: Point, to: Point },
    QuadBezier  { c: Point, to: Point },
    SmoothQuad  { to: Point },
    Arc { rx: f64, ry: f64, x_rotation: f64, large_arc: bool, sweep: bool, to: Point },
    // Relative
    RelMoveTo(Point),
    RelLineTo(Point),
    RelHLineTo(f64),
    RelVLineTo(f64),
    RelCubicBezier { c1: Point, c2: Point, to: Point },
    RelSmoothCubic { c2: Point, to: Point },
    RelQuadBezier  { c: Point, to: Point },
    RelSmoothQuad  { to: Point },
    RelArc { rx: f64, ry: f64, x_rotation: f64, large_arc: bool, sweep: bool, to: Point },
    // Close
    ClosePath,
}

impl PathCommand {
    /// Emit as SVG path `d` token(s).
    pub fn to_svg_token(&self) -> String {
        match self {
            PathCommand::MoveTo(p)  => format!("M {} {}", fmt_f64(p.x), fmt_f64(p.y)),
            PathCommand::LineTo(p)  => format!("L {} {}", fmt_f64(p.x), fmt_f64(p.y)),
            PathCommand::HLineTo(x) => format!("H {}", fmt_f64(*x)),
            PathCommand::VLineTo(y) => format!("V {}", fmt_f64(*y)),
            PathCommand::CubicBezier { c1, c2, to } => format!(
                "C {} {} {} {} {} {}",
                fmt_f64(c1.x), fmt_f64(c1.y), fmt_f64(c2.x), fmt_f64(c2.y),
                fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::SmoothCubic { c2, to } => format!(
                "S {} {} {} {}",
                fmt_f64(c2.x), fmt_f64(c2.y), fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::QuadBezier { c, to } => format!(
                "Q {} {} {} {}",
                fmt_f64(c.x), fmt_f64(c.y), fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::SmoothQuad { to } => format!("T {} {}", fmt_f64(to.x), fmt_f64(to.y)),
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } => format!(
                "A {} {} {} {} {} {} {}",
                fmt_f64(*rx), fmt_f64(*ry), fmt_f64(*x_rotation),
                *large_arc as u8, *sweep as u8,
                fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::RelMoveTo(p)  => format!("m {} {}", fmt_f64(p.x), fmt_f64(p.y)),
            PathCommand::RelLineTo(p)  => format!("l {} {}", fmt_f64(p.x), fmt_f64(p.y)),
            PathCommand::RelHLineTo(x) => format!("h {}", fmt_f64(*x)),
            PathCommand::RelVLineTo(y) => format!("v {}", fmt_f64(*y)),
            PathCommand::RelCubicBezier { c1, c2, to } => format!(
                "c {} {} {} {} {} {}",
                fmt_f64(c1.x), fmt_f64(c1.y), fmt_f64(c2.x), fmt_f64(c2.y),
                fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::RelSmoothCubic { c2, to } => format!(
                "s {} {} {} {}",
                fmt_f64(c2.x), fmt_f64(c2.y), fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::RelQuadBezier { c, to } => format!(
                "q {} {} {} {}",
                fmt_f64(c.x), fmt_f64(c.y), fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::RelSmoothQuad { to } => format!("t {} {}", fmt_f64(to.x), fmt_f64(to.y)),
            PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => format!(
                "a {} {} {} {} {} {} {}",
                fmt_f64(*rx), fmt_f64(*ry), fmt_f64(*x_rotation),
                *large_arc as u8, *sweep as u8,
                fmt_f64(to.x), fmt_f64(to.y)
            ),
            PathCommand::ClosePath => "Z".to_string(),
        }
    }
}

// ── d-string parser ───────────────────────────────────────────────────────────

pub fn parse_d(d: &str) -> Result<Vec<PathCommand>, String> {
    let mut cmds = Vec::new();
    let mut tokens = Tokenizer::new(d);

    while let Some(letter) = tokens.next_letter() {
        match letter {
            'M' => loop {
                let x = tokens.next_f64()?;
                let y = tokens.next_f64()?;
                cmds.push(PathCommand::MoveTo(Point::new(x, y)));
                if !tokens.has_number() { break; }
            },
            'm' => loop {
                let x = tokens.next_f64()?;
                let y = tokens.next_f64()?;
                cmds.push(PathCommand::RelMoveTo(Point::new(x, y)));
                if !tokens.has_number() { break; }
            },
            'L' => loop {
                let x = tokens.next_f64()?;
                let y = tokens.next_f64()?;
                cmds.push(PathCommand::LineTo(Point::new(x, y)));
                if !tokens.has_number() { break; }
            },
            'l' => loop {
                let x = tokens.next_f64()?;
                let y = tokens.next_f64()?;
                cmds.push(PathCommand::RelLineTo(Point::new(x, y)));
                if !tokens.has_number() { break; }
            },
            'H' => loop {
                let x = tokens.next_f64()?;
                cmds.push(PathCommand::HLineTo(x));
                if !tokens.has_number() { break; }
            },
            'h' => loop {
                let x = tokens.next_f64()?;
                cmds.push(PathCommand::RelHLineTo(x));
                if !tokens.has_number() { break; }
            },
            'V' => loop {
                let y = tokens.next_f64()?;
                cmds.push(PathCommand::VLineTo(y));
                if !tokens.has_number() { break; }
            },
            'v' => loop {
                let y = tokens.next_f64()?;
                cmds.push(PathCommand::RelVLineTo(y));
                if !tokens.has_number() { break; }
            },
            'C' => loop {
                let (c1, c2, to) = parse_cubic(&mut tokens)?;
                cmds.push(PathCommand::CubicBezier { c1, c2, to });
                if !tokens.has_number() { break; }
            },
            'c' => loop {
                let (c1, c2, to) = parse_cubic(&mut tokens)?;
                cmds.push(PathCommand::RelCubicBezier { c1, c2, to });
                if !tokens.has_number() { break; }
            },
            'S' => loop {
                let (c2, to) = parse_smooth_cubic(&mut tokens)?;
                cmds.push(PathCommand::SmoothCubic { c2, to });
                if !tokens.has_number() { break; }
            },
            's' => loop {
                let (c2, to) = parse_smooth_cubic(&mut tokens)?;
                cmds.push(PathCommand::RelSmoothCubic { c2, to });
                if !tokens.has_number() { break; }
            },
            'Q' => loop {
                let (c, to) = parse_quad(&mut tokens)?;
                cmds.push(PathCommand::QuadBezier { c, to });
                if !tokens.has_number() { break; }
            },
            'q' => loop {
                let (c, to) = parse_quad(&mut tokens)?;
                cmds.push(PathCommand::RelQuadBezier { c, to });
                if !tokens.has_number() { break; }
            },
            'T' => loop {
                let x = tokens.next_f64()?; let y = tokens.next_f64()?;
                cmds.push(PathCommand::SmoothQuad { to: Point::new(x, y) });
                if !tokens.has_number() { break; }
            },
            't' => loop {
                let x = tokens.next_f64()?; let y = tokens.next_f64()?;
                cmds.push(PathCommand::RelSmoothQuad { to: Point::new(x, y) });
                if !tokens.has_number() { break; }
            },
            'A' => loop {
                let arc = parse_arc(&mut tokens)?;
                cmds.push(arc);
                if !tokens.has_number() { break; }
            },
            'a' => loop {
                let arc = parse_arc_rel(&mut tokens)?;
                cmds.push(arc);
                if !tokens.has_number() { break; }
            },
            'Z' | 'z' => cmds.push(PathCommand::ClosePath),
            other => return Err(format!("unknown path command '{}'", other)),
        }
    }
    Ok(cmds)
}

fn parse_cubic(t: &mut Tokenizer) -> Result<(Point, Point, Point), String> {
    Ok((
        Point::new(t.next_f64()?, t.next_f64()?),
        Point::new(t.next_f64()?, t.next_f64()?),
        Point::new(t.next_f64()?, t.next_f64()?),
    ))
}

fn parse_smooth_cubic(t: &mut Tokenizer) -> Result<(Point, Point), String> {
    Ok((
        Point::new(t.next_f64()?, t.next_f64()?),
        Point::new(t.next_f64()?, t.next_f64()?),
    ))
}

fn parse_quad(t: &mut Tokenizer) -> Result<(Point, Point), String> {
    Ok((
        Point::new(t.next_f64()?, t.next_f64()?),
        Point::new(t.next_f64()?, t.next_f64()?),
    ))
}

fn parse_arc(t: &mut Tokenizer) -> Result<PathCommand, String> {
    let rx = t.next_f64()?;
    let ry = t.next_f64()?;
    let x_rotation = t.next_f64()?;
    let large_arc  = t.next_flag()?;
    let sweep      = t.next_flag()?;
    let to         = Point::new(t.next_f64()?, t.next_f64()?);
    Ok(PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to })
}

fn parse_arc_rel(t: &mut Tokenizer) -> Result<PathCommand, String> {
    let rx = t.next_f64()?;
    let ry = t.next_f64()?;
    let x_rotation = t.next_f64()?;
    let large_arc  = t.next_flag()?;
    let sweep      = t.next_flag()?;
    let to         = Point::new(t.next_f64()?, t.next_f64()?);
    Ok(PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to })
}

// ── Tokenizer ─────────────────────────────────────────────────────────────────

struct Tokenizer<'a> {
    src:    &'a [u8],
    cursor: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(s: &'a str) -> Self {
        Tokenizer { src: s.as_bytes(), cursor: 0 }
    }

    fn skip_ws_comma(&mut self) {
        while self.cursor < self.src.len() {
            match self.src[self.cursor] {
                b' ' | b'\t' | b'\r' | b'\n' | b',' => self.cursor += 1,
                _ => break,
            }
        }
    }

    fn next_letter(&mut self) -> Option<char> {
        self.skip_ws_comma();
        if self.cursor >= self.src.len() { return None; }
        let b = self.src[self.cursor] as char;
        if b.is_ascii_alphabetic() {
            self.cursor += 1;
            Some(b)
        } else {
            None
        }
    }

    fn next_f64(&mut self) -> Result<f64, String> {
        self.skip_ws_comma();
        let start = self.cursor;
        if self.cursor < self.src.len() && (self.src[self.cursor] == b'-' || self.src[self.cursor] == b'+') {
            self.cursor += 1;
        }
        while self.cursor < self.src.len() && (self.src[self.cursor].is_ascii_digit() || self.src[self.cursor] == b'.') {
            self.cursor += 1;
        }
        // scientific notation
        if self.cursor < self.src.len() && (self.src[self.cursor] == b'e' || self.src[self.cursor] == b'E') {
            self.cursor += 1;
            if self.cursor < self.src.len() && (self.src[self.cursor] == b'-' || self.src[self.cursor] == b'+') {
                self.cursor += 1;
            }
            while self.cursor < self.src.len() && self.src[self.cursor].is_ascii_digit() {
                self.cursor += 1;
            }
        }
        let slice = std::str::from_utf8(&self.src[start..self.cursor])
            .map_err(|e| e.to_string())?;
        slice.parse::<f64>().map_err(|_| format!("expected number, got '{}'", slice))
    }

    fn next_flag(&mut self) -> Result<bool, String> {
        self.skip_ws_comma();
        if self.cursor >= self.src.len() {
            return Err("expected flag (0 or 1) but reached end of string".to_string());
        }
        match self.src[self.cursor] {
            b'0' => { self.cursor += 1; Ok(false) }
            b'1' => { self.cursor += 1; Ok(true)  }
            other => Err(format!("expected flag 0 or 1, got '{}'", other as char)),
        }
    }

    fn has_number(&mut self) -> bool {
        self.skip_ws_comma();
        if self.cursor >= self.src.len() { return false; }
        let b = self.src[self.cursor];
        b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.'
    }
}

// ── d-string serialiser ───────────────────────────────────────────────────────

pub fn commands_to_d(cmds: &[PathCommand]) -> String {
    cmds.iter()
        .map(|c| c.to_svg_token())
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Binary encoder / decoder ──────────────────────────────────────────────────

pub fn encode_commands(cmds: &[PathCommand], out: &mut Vec<u8>) {
    for cmd in cmds {
        encode_command(cmd, out);
    }
}

fn encode_command(cmd: &PathCommand, out: &mut Vec<u8>) {
    let write_f32 = |v: f64, o: &mut Vec<u8>| o.extend_from_slice(&(v as f32).to_le_bytes());
    let write_pt  = |p: Point, o: &mut Vec<u8>| { write_f32(p.x, o); write_f32(p.y, o); };
    let arc_flags = |large: bool, sweep: bool| (large as u8) | ((sweep as u8) << 1);

    match cmd {
        PathCommand::MoveTo(p)           => { out.push(CMD_MOVE_TO);   write_pt(*p, out); }
        PathCommand::LineTo(p)           => { out.push(CMD_LINE_TO);   write_pt(*p, out); }
        PathCommand::HLineTo(x)          => { out.push(CMD_H_LINE_TO); write_f32(*x, out); }
        PathCommand::VLineTo(y)          => { out.push(CMD_V_LINE_TO); write_f32(*y, out); }
        PathCommand::CubicBezier { c1, c2, to } => {
            out.push(CMD_CUBIC);
            write_pt(*c1, out); write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::SmoothCubic { c2, to } => {
            out.push(CMD_SMOOTH_CUBIC);
            write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::QuadBezier { c, to } => {
            out.push(CMD_QUAD);
            write_pt(*c, out); write_pt(*to, out);
        }
        PathCommand::SmoothQuad { to }   => { out.push(CMD_SMOOTH_QUAD); write_pt(*to, out); }
        PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } => {
            out.push(CMD_ARC);
            write_f32(*rx, out); write_f32(*ry, out); write_f32(*x_rotation, out);
            out.push(arc_flags(*large_arc, *sweep));
            write_pt(*to, out);
        }
        PathCommand::RelMoveTo(p)            => { out.push(CMD_REL_MOVE_TO);   write_pt(*p, out); }
        PathCommand::RelLineTo(p)            => { out.push(CMD_REL_LINE_TO);   write_pt(*p, out); }
        PathCommand::RelHLineTo(x)           => { out.push(CMD_REL_H_LINE_TO); write_f32(*x, out); }
        PathCommand::RelVLineTo(y)           => { out.push(CMD_REL_V_LINE_TO); write_f32(*y, out); }
        PathCommand::RelCubicBezier { c1, c2, to } => {
            out.push(CMD_REL_CUBIC);
            write_pt(*c1, out); write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::RelSmoothCubic { c2, to } => {
            out.push(CMD_REL_SMOOTH_CUBIC);
            write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::RelQuadBezier { c, to } => {
            out.push(CMD_REL_QUAD);
            write_pt(*c, out); write_pt(*to, out);
        }
        PathCommand::RelSmoothQuad { to }    => { out.push(CMD_REL_SMOOTH_QUAD); write_pt(*to, out); }
        PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
            out.push(CMD_REL_ARC);
            write_f32(*rx, out); write_f32(*ry, out); write_f32(*x_rotation, out);
            out.push(arc_flags(*large_arc, *sweep));
            write_pt(*to, out);
        }
        PathCommand::ClosePath => { out.push(CMD_CLOSE); }
    }
}

pub fn decode_commands(data: &[u8]) -> Result<Vec<PathCommand>, std::io::Error> {
    let mut cmds   = Vec::new();
    let mut cursor = 0usize;

    let read_f32 = |c: &mut usize, d: &[u8]| -> Result<f64, std::io::Error> {
        if *c + 4 > d.len() { return Err(eof()); }
        let v = f32::from_le_bytes(d[*c..*c+4].try_into().unwrap()) as f64;
        *c += 4;
        Ok(v)
    };
    let read_pt = |c: &mut usize, d: &[u8]| -> Result<Point, std::io::Error> {
        let x = read_f32(c, d)?;
        let y = read_f32(c, d)?;
        Ok(Point::new(x, y))
    };

    while cursor < data.len() {
        let tag = data[cursor];
        cursor += 1;

        let cmd = match tag {
            CMD_MOVE_TO  => PathCommand::MoveTo(read_pt(&mut cursor, data)?),
            CMD_LINE_TO  => PathCommand::LineTo(read_pt(&mut cursor, data)?),
            CMD_H_LINE_TO => { let x = read_f32(&mut cursor, data)?; PathCommand::HLineTo(x) }
            CMD_V_LINE_TO => { let y = read_f32(&mut cursor, data)?; PathCommand::VLineTo(y) }
            CMD_CUBIC    => {
                let c1 = read_pt(&mut cursor, data)?;
                let c2 = read_pt(&mut cursor, data)?;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::CubicBezier { c1, c2, to }
            }
            CMD_SMOOTH_CUBIC => {
                let c2 = read_pt(&mut cursor, data)?;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::SmoothCubic { c2, to }
            }
            CMD_QUAD => {
                let c  = read_pt(&mut cursor, data)?;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::QuadBezier { c, to }
            }
            CMD_SMOOTH_QUAD => { let to = read_pt(&mut cursor, data)?; PathCommand::SmoothQuad { to } }
            CMD_ARC => {
                let rx = read_f32(&mut cursor, data)?;
                let ry = read_f32(&mut cursor, data)?;
                let xr = read_f32(&mut cursor, data)?;
                if cursor >= data.len() { return Err(eof()); }
                let flags = data[cursor]; cursor += 1;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::Arc {
                    rx, ry, x_rotation: xr,
                    large_arc: flags & 1 != 0,
                    sweep: flags & 2 != 0,
                    to,
                }
            }
            CMD_REL_MOVE_TO  => PathCommand::RelMoveTo(read_pt(&mut cursor, data)?),
            CMD_REL_LINE_TO  => PathCommand::RelLineTo(read_pt(&mut cursor, data)?),
            CMD_REL_H_LINE_TO => { let x = read_f32(&mut cursor, data)?; PathCommand::RelHLineTo(x) }
            CMD_REL_V_LINE_TO => { let y = read_f32(&mut cursor, data)?; PathCommand::RelVLineTo(y) }
            CMD_REL_CUBIC    => {
                let c1 = read_pt(&mut cursor, data)?;
                let c2 = read_pt(&mut cursor, data)?;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::RelCubicBezier { c1, c2, to }
            }
            CMD_REL_SMOOTH_CUBIC => {
                let c2 = read_pt(&mut cursor, data)?;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::RelSmoothCubic { c2, to }
            }
            CMD_REL_QUAD => {
                let c  = read_pt(&mut cursor, data)?;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::RelQuadBezier { c, to }
            }
            CMD_REL_SMOOTH_QUAD => { let to = read_pt(&mut cursor, data)?; PathCommand::RelSmoothQuad { to } }
            CMD_REL_ARC => {
                let rx = read_f32(&mut cursor, data)?;
                let ry = read_f32(&mut cursor, data)?;
                let xr = read_f32(&mut cursor, data)?;
                if cursor >= data.len() { return Err(eof()); }
                let flags = data[cursor]; cursor += 1;
                let to = read_pt(&mut cursor, data)?;
                PathCommand::RelArc {
                    rx, ry, x_rotation: xr,
                    large_arc: flags & 1 != 0,
                    sweep: flags & 2 != 0,
                    to,
                }
            }
            CMD_CLOSE => PathCommand::ClosePath,
            other => return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown path command tag 0x{:02x}", other),
            )),
        };
        cmds.push(cmd);
    }
    Ok(cmds)
}

fn eof() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "path command data truncated")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(cmds: &[PathCommand]) {
        let d    = commands_to_d(cmds);
        let back = parse_d(&d).expect("parse failed");
        assert_eq!(back.len(), cmds.len(), "command count mismatch for: {}", d);
    }

    #[test]
    fn parse_simple_triangle() {
        let cmds = parse_d("M 50 450 L 250 50 L 450 450 Z").unwrap();
        assert_eq!(cmds.len(), 4);
        assert!(matches!(cmds[0], PathCommand::MoveTo(_)));
        assert!(matches!(cmds[3], PathCommand::ClosePath));
    }

    #[test]
    fn parse_arc() {
        let cmds = parse_d("M 120 380 A 130 130 0 0 1 380 380").unwrap();
        assert_eq!(cmds.len(), 2);
        if let PathCommand::Arc { rx, ry, large_arc, sweep, .. } = &cmds[1] {
            assert!((rx - 130.0).abs() < 1e-4);
            assert!((ry - 130.0).abs() < 1e-4);
            assert!(!large_arc);
            assert!(*sweep);
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn d_string_roundtrip() {
        let cmds = vec![
            PathCommand::MoveTo(Point::new(10.0, 20.0)),
            PathCommand::CubicBezier {
                c1: Point::new(30.0, 40.0),
                c2: Point::new(50.0, 60.0),
                to: Point::new(70.0, 80.0),
            },
            PathCommand::ClosePath,
        ];
        rt(&cmds);
    }

    #[test]
    fn binary_roundtrip() {
        let cmds = vec![
            PathCommand::MoveTo(Point::new(0.0, 0.0)),
            PathCommand::LineTo(Point::new(100.0, 0.0)),
            PathCommand::VLineTo(100.0),
            PathCommand::ClosePath,
        ];
        let mut buf = Vec::new();
        encode_commands(&cmds, &mut buf);
        let back = decode_commands(&buf).unwrap();
        assert_eq!(cmds, back);
    }

    #[test]
    fn relative_commands_preserved() {
        let d = "m 10 20 l 5 5 h 10 v -5 z";
        let cmds = parse_d(d).unwrap();
        assert!(matches!(cmds[0], PathCommand::RelMoveTo(_)));
        assert!(matches!(cmds[1], PathCommand::RelLineTo(_)));
        assert!(matches!(cmds[2], PathCommand::RelHLineTo(_)));
        assert!(matches!(cmds[3], PathCommand::RelVLineTo(_)));
        assert!(matches!(cmds[4], PathCommand::ClosePath));
    }
}
