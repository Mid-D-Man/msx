// core/msx-binary/src/effect_codec.rs
//! Binary encode/decode for Effect (post-processing pipeline nodes).
//!
//! Pattern taken from Grease Pencil's VFX pipeline: each effect type is a
//! discrete tagged variant carrying just its own parameters, applied in
//! sequence by the renderer's ping-pong FBO chain.

use std::io;
use msx_ast::Effect;

use crate::decoder::{read_color, read_f32, read_u8};
use crate::encoder::{write_color, write_f32, write_u8};
use crate::tags::*;

pub fn write_effect(out: &mut Vec<u8>, effect: &Effect) {
    match effect {
        Effect::Blur { radius } => {
            write_u8(out, EFFECT_TAG_BLUR);
            write_f32(out, *radius);
        }
        Effect::DropShadow { offset_x, offset_y, blur_radius, color, opacity } => {
            write_u8(out, EFFECT_TAG_DROP_SHADOW);
            write_f32(out, *offset_x);
            write_f32(out, *offset_y);
            write_f32(out, *blur_radius);
            write_color(out, *color);
            write_f32(out, *opacity);
        }
        Effect::InnerShadow { offset_x, offset_y, blur_radius, color, opacity } => {
            write_u8(out, EFFECT_TAG_INNER_SHADOW);
            write_f32(out, *offset_x);
            write_f32(out, *offset_y);
            write_f32(out, *blur_radius);
            write_color(out, *color);
            write_f32(out, *opacity);
        }
        Effect::OuterGlow { color, blur_radius, spread, opacity } => {
            write_u8(out, EFFECT_TAG_OUTER_GLOW);
            write_color(out, *color);
            write_f32(out, *blur_radius);
            write_f32(out, *spread);
            write_f32(out, *opacity);
        }
        Effect::InnerGlow { color, blur_radius, opacity } => {
            write_u8(out, EFFECT_TAG_INNER_GLOW);
            write_color(out, *color);
            write_f32(out, *blur_radius);
            write_f32(out, *opacity);
        }
    }
}

pub fn read_effect(data: &[u8], cursor: &mut usize) -> io::Result<Effect> {
    let tag = read_u8(data, cursor)?;
    match tag {
        EFFECT_TAG_BLUR => {
            let radius = read_f32(data, cursor)?;
            Ok(Effect::Blur { radius })
        }
        EFFECT_TAG_DROP_SHADOW => {
            let offset_x    = read_f32(data, cursor)?;
            let offset_y    = read_f32(data, cursor)?;
            let blur_radius = read_f32(data, cursor)?;
            let color       = read_color(data, cursor)?;
            let opacity     = read_f32(data, cursor)?;
            Ok(Effect::DropShadow { offset_x, offset_y, blur_radius, color, opacity })
        }
        EFFECT_TAG_INNER_SHADOW => {
            let offset_x    = read_f32(data, cursor)?;
            let offset_y    = read_f32(data, cursor)?;
            let blur_radius = read_f32(data, cursor)?;
            let color       = read_color(data, cursor)?;
            let opacity     = read_f32(data, cursor)?;
            Ok(Effect::InnerShadow { offset_x, offset_y, blur_radius, color, opacity })
        }
        EFFECT_TAG_OUTER_GLOW => {
            let color       = read_color(data, cursor)?;
            let blur_radius = read_f32(data, cursor)?;
            let spread      = read_f32(data, cursor)?;
            let opacity     = read_f32(data, cursor)?;
            Ok(Effect::OuterGlow { color, blur_radius, spread, opacity })
        }
        EFFECT_TAG_INNER_GLOW => {
            let color       = read_color(data, cursor)?;
            let blur_radius = read_f32(data, cursor)?;
            let opacity     = read_f32(data, cursor)?;
            Ok(Effect::InnerGlow { color, blur_radius, opacity })
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown effect tag 0x{:02x}", other),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::Color;

    #[test]
    fn blur_roundtrip() {
        let e = Effect::Blur { radius: 12.5 };
        let mut buf = Vec::new();
        write_effect(&mut buf, &e);
        let mut cursor = 0;
        let back = read_effect(&buf, &mut cursor).unwrap();
        if let Effect::Blur { radius } = back {
            assert!((radius - 12.5).abs() < 1e-3);
        } else {
            panic!("expected Blur");
        }
    }

    #[test]
    fn drop_shadow_roundtrip() {
        let e = Effect::DropShadow {
            offset_x: 4.0, offset_y: 6.0, blur_radius: 8.0,
            color: Color::rgba(0, 0, 0, 180), opacity: 0.5,
        };
        let mut buf = Vec::new();
        write_effect(&mut buf, &e);
        let mut cursor = 0;
        let back = read_effect(&buf, &mut cursor).unwrap();
        if let Effect::DropShadow { offset_x, offset_y, blur_radius, color, opacity } = back {
            assert!((offset_x - 4.0).abs() < 1e-3);
            assert!((offset_y - 6.0).abs() < 1e-3);
            assert!((blur_radius - 8.0).abs() < 1e-3);
            assert_eq!(color, Color::rgba(0, 0, 0, 180));
            assert!((opacity - 0.5).abs() < 1e-3);
        } else {
            panic!("expected DropShadow");
        }
    }

    #[test]
    fn outer_glow_roundtrip() {
        let e = Effect::OuterGlow {
            color: Color::rgb(255, 200, 100), blur_radius: 16.0, spread: 4.0, opacity: 0.75,
        };
        let mut buf = Vec::new();
        write_effect(&mut buf, &e);
        let mut cursor = 0;
        let back = read_effect(&buf, &mut cursor).unwrap();
        if let Effect::OuterGlow { color, blur_radius, spread, opacity } = back {
            assert_eq!(color, Color::rgb(255, 200, 100));
            assert!((blur_radius - 16.0).abs() < 1e-3);
            assert!((spread - 4.0).abs() < 1e-3);
            assert!((opacity - 0.75).abs() < 1e-3);
        } else {
            panic!("expected OuterGlow");
        }
    }
}
