// core/msx-parser/src/layer.rs
use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{BlendMode, Color, Effect, Layer};

use crate::dix_helpers::{array_len, color_from, enum_discriminant, opt, raw, type_tag};
use crate::element::parse_elements;
use crate::transform::parse_transform;

/// `{ type = "layer", blend_mode<enum> = BlendMode.Multiply (or "multiply"),
///    opacity=0.75, clip=true, effects=[...], elements=[...] }`
pub fn parse_layer(data: &DixData, prefix: &str) -> Result<Layer, String> {
    let id = opt::<String>(data, prefix, "id")?;
    let transform = parse_transform(data, prefix)?;
    let children = parse_elements(data, prefix, "elements")?;

    let mut layer = Layer::new(children);
    layer.id = id;
    layer.transform = transform;
    layer.blend_mode = parse_blend_mode(data, prefix)?.unwrap_or_default();
    layer.opacity = opt::<f64>(data, prefix, "opacity")?.unwrap_or(1.0);
    layer.clip = opt::<bool>(data, prefix, "clip")?.unwrap_or(false);
    layer.effects = parse_effects(data, prefix, "effects")?;

    Ok(layer)
}

fn parse_blend_mode(data: &DixData, prefix: &str) -> Result<Option<BlendMode>, String> {
    if let Some(v) = enum_discriminant(raw(data, prefix, "blend_mode")) {
        return Ok(Some(BlendMode::from_byte(v as u8)));
    }
    Ok(opt::<String>(data, prefix, "blend_mode")?.map(|s| blend_mode_from_str(&s)))
}

fn blend_mode_from_str(s: &str) -> BlendMode {
    match s {
        "multiply"   => BlendMode::Multiply,
        "screen"     => BlendMode::Screen,
        "overlay"    => BlendMode::Overlay,
        "add"        => BlendMode::Add,
        "soft_light" => BlendMode::SoftLight,
        "hard_light" => BlendMode::HardLight,
        "difference" => BlendMode::Difference,
        "exclusion"  => BlendMode::Exclusion,
        "darken"     => BlendMode::Darken,
        "lighten"    => BlendMode::Lighten,
        "subtract"   => BlendMode::Subtract,
        "divide"     => BlendMode::Divide,
        _            => BlendMode::Normal,
    }
}

fn parse_effects(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Effect>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(parse_effect(data, &format!("{}[{}]", path, i))?);
    }
    Ok(out)
}

fn parse_effect(data: &DixData, prefix: &str) -> Result<Effect, String> {
    let kind = type_tag(data, prefix)?;
    match kind.as_str() {
        "blur" => Ok(Effect::Blur {
            radius: opt::<f64>(data, prefix, "radius")?.unwrap_or(4.0),
        }),
        "drop_shadow" => Ok(Effect::DropShadow {
            offset_x: opt::<f64>(data, prefix, "offset_x")?.unwrap_or(0.0),
            offset_y: opt::<f64>(data, prefix, "offset_y")?.unwrap_or(0.0),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(4.0),
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::BLACK),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.5),
        }),
        "inner_shadow" => Ok(Effect::InnerShadow {
            offset_x: opt::<f64>(data, prefix, "offset_x")?.unwrap_or(0.0),
            offset_y: opt::<f64>(data, prefix, "offset_y")?.unwrap_or(0.0),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(4.0),
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::BLACK),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.5),
        }),
        "outer_glow" => Ok(Effect::OuterGlow {
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::WHITE),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(8.0),
            spread: opt::<f64>(data, prefix, "spread")?.unwrap_or(0.0),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.75),
        }),
        "inner_glow" => Ok(Effect::InnerGlow {
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::WHITE),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(8.0),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.75),
        }),
        other => Err(format!("{}: unknown effect type '{}'", prefix, other)),
    }
                 }
