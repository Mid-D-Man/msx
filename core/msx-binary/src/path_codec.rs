// core/msx-binary/src/path_codec.rs
//! Binary encode/decode for PathCommand sequences.
//! Moved out of msx-ast to keep that crate dependency-free of io types.

use std::io;
use msx_ast::{PathCommand, Point};

use crate::decoder::{read_f32, read_u8};
use crate::encoder::{write_f32, write_u8};
use crate::tags::*;

pub fn encode_commands(cmds: &[PathCommand], out: &mut Vec<u8>) {
    for cmd in cmds {
        encode_command(cmd, out);
    }
}

fn write_pt(p: Point, out: &mut Vec<u8>) {
    write_f32(out, p.x);
    write_f32(out, p.y);
}

fn arc_flags(large: bool, sweep: bool) -> u8 {
    (large as u8) | ((sweep as u8) << 1)
}

fn encode_command(cmd: &PathCommand, out: &mut Vec<u8>) {
    match cmd {
        PathCommand::MoveTo(p)           => { write_u8(out, CMD_MOVE_TO);   write_pt(*p, out); }
        PathCommand::LineTo(p)           => { write_u8(out, CMD_LINE_TO);   write_pt(*p, out); }
        PathCommand::HLineTo(x)          => { write_u8(out, CMD_H_LINE_TO); write_f32(out, *x); }
        PathCommand::VLineTo(y)          => { write_u8(out, CMD_V_LINE_TO); write_f32(out, *y); }
        PathCommand::CubicBezier { c1, c2, to } => {
            write_u8(out, CMD_CUBIC);
            write_pt(*c1, out); write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::SmoothCubic { c2, to } => {
            write_u8(out, CMD_SMOOTH_CUBIC);
            write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::QuadBezier { c, to } => {
            write_u8(out, CMD_QUAD);
            write_pt(*c, out); write_pt(*to, out);
        }
        PathCommand::SmoothQuad { to } => { write_u8(out, CMD_SMOOTH_QUAD); write_pt(*to, out); }
        PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } => {
            write_u8(out, CMD_ARC);
            write_f32(out, *rx); write_f32(out, *ry); write_f32(out, *x_rotation);
            write_u8(out, arc_flags(*large_arc, *sweep));
            write_pt(*to, out);
        }
        PathCommand::RelMoveTo(p)            => { write_u8(out, CMD_REL_MOVE_TO);   write_pt(*p, out); }
        PathCommand::RelLineTo(p)            => { write_u8(out, CMD_REL_LINE_TO);   write_pt(*p, out); }
        PathCommand::RelHLineTo(x)           => { write_u8(out, CMD_REL_H_LINE_TO); write_f32(out, *x); }
        PathCommand::RelVLineTo(y)           => { write_u8(out, CMD_REL_V_LINE_TO); write_f32(out, *y); }
        PathCommand::RelCubicBezier { c1, c2, to } => {
            write_u8(out, CMD_REL_CUBIC);
            write_pt(*c1, out); write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::RelSmoothCubic { c2, to } => {
            write_u8(out, CMD_REL_SMOOTH_CUBIC);
            write_pt(*c2, out); write_pt(*to, out);
        }
        PathCommand::RelQuadBezier { c, to } => {
            write_u8(out, CMD_REL_QUAD);
            write_pt(*c, out); write_pt(*to, out);
        }
        PathCommand::RelSmoothQuad { to } => { write_u8(out, CMD_REL_SMOOTH_QUAD); write_pt(*to, out); }
        PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
            write_u8(out, CMD_REL_ARC);
            write_f32(out, *rx); write_f32(out, *ry); write_f32(out, *x_rotation);
            write_u8(out, arc_flags(*large_arc, *sweep));
            write_pt(*to, out);
        }
        PathCommand::ClosePath => { write_u8(out, CMD_CLOSE); }
    }
}

pub fn decode_commands(data: &[u8]) -> io::Result<Vec<PathCommand>> {
    let mut cmds   = Vec::new();
    let mut cursor = 0usize;

    while cursor < data.len() {
        let tag = read_u8(data, &mut cursor)?;
        cmds.push(decode_command(tag, data, &mut cursor)?);
    }
    Ok(cmds)
}

fn read_pt(data: &[u8], cursor: &mut usize) -> io::Result<Point> {
    let x = read_f32(data, cursor)?;
    let y = read_f32(data, cursor)?;
    Ok(Point::new(x, y))
}

fn decode_command(tag: u8, data: &[u8], cursor: &mut usize) -> io::Result<PathCommand> {
    Ok(match tag {
        CMD_MOVE_TO   => PathCommand::MoveTo(read_pt(data, cursor)?),
        CMD_LINE_TO   => PathCommand::LineTo(read_pt(data, cursor)?),
        CMD_H_LINE_TO => PathCommand::HLineTo(read_f32(data, cursor)?),
        CMD_V_LINE_TO => PathCommand::VLineTo(read_f32(data, cursor)?),
        CMD_CUBIC => {
            let c1 = read_pt(data, cursor)?;
            let c2 = read_pt(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::CubicBezier { c1, c2, to }
        }
        CMD_SMOOTH_CUBIC => {
            let c2 = read_pt(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::SmoothCubic { c2, to }
        }
        CMD_QUAD => {
            let c  = read_pt(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::QuadBezier { c, to }
        }
        CMD_SMOOTH_QUAD => PathCommand::SmoothQuad { to: read_pt(data, cursor)? },
        CMD_ARC => {
            let rx = read_f32(data, cursor)?;
            let ry = read_f32(data, cursor)?;
            let xr = read_f32(data, cursor)?;
            let flags = read_u8(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::Arc {
                rx, ry, x_rotation: xr,
                large_arc: flags & 1 != 0,
                sweep: flags & 2 != 0,
                to,
            }
        }
        CMD_REL_MOVE_TO   => PathCommand::RelMoveTo(read_pt(data, cursor)?),
        CMD_REL_LINE_TO   => PathCommand::RelLineTo(read_pt(data, cursor)?),
        CMD_REL_H_LINE_TO => PathCommand::RelHLineTo(read_f32(data, cursor)?),
        CMD_REL_V_LINE_TO => PathCommand::RelVLineTo(read_f32(data, cursor)?),
        CMD_REL_CUBIC => {
            let c1 = read_pt(data, cursor)?;
            let c2 = read_pt(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::RelCubicBezier { c1, c2, to }
        }
        CMD_REL_SMOOTH_CUBIC => {
            let c2 = read_pt(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::RelSmoothCubic { c2, to }
        }
        CMD_REL_QUAD => {
            let c  = read_pt(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::RelQuadBezier { c, to }
        }
        CMD_REL_SMOOTH_QUAD => PathCommand::RelSmoothQuad { to: read_pt(data, cursor)? },
        CMD_REL_ARC => {
            let rx = read_f32(data, cursor)?;
            let ry = read_f32(data, cursor)?;
            let xr = read_f32(data, cursor)?;
            let flags = read_u8(data, cursor)?;
            let to = read_pt(data, cursor)?;
            PathCommand::RelArc {
                rx, ry, x_rotation: xr,
                large_arc: flags & 1 != 0,
                sweep: flags & 2 != 0,
                to,
            }
        }
        CMD_CLOSE => PathCommand::ClosePath,
        other => return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown path command tag 0x{:02x}", other),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn arc_roundtrip() {
        let cmds = vec![PathCommand::Arc {
            rx: 130.0, ry: 130.0, x_rotation: 0.0,
            large_arc: false, sweep: true,
            to: Point::new(380.0, 380.0),
        }];
        let mut buf = Vec::new();
        encode_commands(&cmds, &mut buf);
        let back = decode_commands(&buf).unwrap();
        assert_eq!(cmds, back);
    }
                                         }
