// core/msx-parser/src/gradient.rs
use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{AudioDef, Color, ConicGradient, Def, LinearGradient, RadialGradient, ShaderDef, ShaderUniform, ShaderUniformValue, Stop};

use crate::dix_helpers::{array_len, color_from, opt, parse_media_source, raw, type_tag};

/// Parse the `defs::` (or nested `defs = [...]`) array on the canvas, or on
/// any future scope that wants its own local defs.
pub fn parse_defs(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Def>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(parse_def(data, &format!("{}[{}]", path, i))?);
    }
    Ok(out)
}

fn parse_def(data: &DixData, prefix: &str) -> Result<Def, String> {
    let kind = type_tag(data, prefix)?;
    match kind.as_str() {
        "linear_gradient" => {
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            let x1 = opt::<f64>(data, prefix, "x1")?.unwrap_or(0.0);
            let y1 = opt::<f64>(data, prefix, "y1")?.unwrap_or(0.0);
            let x2 = opt::<f64>(data, prefix, "x2")?.unwrap_or(1.0);
            let y2 = opt::<f64>(data, prefix, "y2")?.unwrap_or(0.0);
            let stops = parse_stops(data, prefix, "stops")?;
            Ok(Def::LinearGradient(LinearGradient::new(id, x1, y1, x2, y2, stops)))
        }
        "radial_gradient" => {
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            let cx = opt::<f64>(data, prefix, "cx")?.unwrap_or(0.5);
            let cy = opt::<f64>(data, prefix, "cy")?.unwrap_or(0.5);
            let r  = opt::<f64>(data, prefix, "r")?.unwrap_or(0.5);
            let fx = opt::<f64>(data, prefix, "fx")?.unwrap_or(cx);
            let fy = opt::<f64>(data, prefix, "fy")?.unwrap_or(cy);
            let stops = parse_stops(data, prefix, "stops")?;
            Ok(Def::RadialGradient(RadialGradient::new(id, cx, cy, r, fx, fy, stops)))
        }
        "conic_gradient" => {
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            let cx    = opt::<f64>(data, prefix, "cx")?.unwrap_or(0.5);
            let cy    = opt::<f64>(data, prefix, "cy")?.unwrap_or(0.5);
            let angle = opt::<f64>(data, prefix, "angle")?.unwrap_or(0.0);
            let stops = parse_stops(data, prefix, "stops")?;
            Ok(Def::ConicGradient(ConicGradient::new(id, cx, cy, angle, stops)))
        }
        "shader" => {
            // FUTURE(raw-section): see the matching marker on `ShaderDef`
            // in core/msx-ast/src/gradient.rs for the migration path once
            // DixScript grows a raw/heredoc text-block section — this arm
            // would gain a branch reading that instead of `source_ref`
            // when an inline source is present.
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            // A path to a .wgsl file, relative to this .msx source — never
            // resolved WGSL text (DixScript has no raw/heredoc string type
            // to embed that inline without base64 or manual escaping).
            // `msx-cli compile` resolves/validates this at compile time.
            let source_ref = opt::<String>(data, prefix, "source_ref")?
                .ok_or_else(|| format!("{}.source_ref: required", prefix))?;
            let entry_point = opt::<String>(data, prefix, "entry_point")?
                .unwrap_or_else(|| "fs_main".to_string());
            let fallback_color = color_from(raw(data, prefix, "fallback_color"))
                .ok_or_else(|| format!("{}.fallback_color: required (every renderer that can't run WGSL paints this instead)", prefix))?;
            let uniforms = parse_shader_uniforms(data, prefix, "uniforms")?;
            Ok(Def::Shader(
                ShaderDef::new(id, source_ref, fallback_color)
                    .with_entry_point(entry_point)
                    .with_uniforms(uniforms),
            ))
        }
        "audio" => {
            let id = opt::<String>(data, prefix, "id")?
                .ok_or_else(|| format!("{}.id: required", prefix))?;
            // Deliberately NO format-sniff validation here, unlike
            // `element.rs`'s `parse_image` — see `AudioDef`'s own doc
            // comment in `media.rs` for why: nothing in this project
            // plays audio yet, so there's no render-time failure mode
            // this would be protecting against the way image format
            // validation protects three different renderers from a
            // garbled blob.
            let source = parse_media_source(data, prefix)?;
            Ok(Def::Audio(AudioDef::new(id, source)))
        }
        other => Err(format!("{}: unknown def type '{}'", prefix, other)),
    }
}

/// `uniforms = [ { name = "speed", type = "float", value = 1.5 },
///               { name = "resolution", type = "vec2", value = [800.0, 600.0] }, ... ]`
/// `type` picks how many components `value` must have — a bare float for
/// `"float"`, or an array of exactly 2/3/4 numbers for `"vec2"/"vec3"/"vec4"`.
fn parse_shader_uniforms(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<ShaderUniform>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let item_path = format!("{}[{}]", path, i);
        let name = opt::<String>(data, &item_path, "name")?
            .ok_or_else(|| format!("{}.name: required", item_path))?;
        let kind = type_tag(data, &item_path)?;
        let value = match kind.as_str() {
            "float" => {
                let v = opt::<f64>(data, &item_path, "value")?
                    .ok_or_else(|| format!("{}.value: required float", item_path))?;
                ShaderUniformValue::Float(v as f32)
            }
            "vec2" | "vec3" | "vec4" => {
                let want = match kind.as_str() { "vec2" => 2, "vec3" => 3, "vec4" => 4, _ => unreachable!() };
                let v = opt::<Vec<f64>>(data, &item_path, "value")?
                    .ok_or_else(|| format!("{}.value: required array of {} numbers", item_path, want))?;
                if v.len() != want {
                    return Err(format!("{}.value: expected {} numbers for {}, got {}", item_path, want, kind, v.len()));
                }
                match want {
                    2 => ShaderUniformValue::Vec2(v[0] as f32, v[1] as f32),
                    3 => ShaderUniformValue::Vec3(v[0] as f32, v[1] as f32, v[2] as f32),
                    _ => ShaderUniformValue::Vec4(v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32),
                }
            }
            other => return Err(format!("{}.type: unknown uniform type '{}' (expected float, vec2, vec3, or vec4)", item_path, other)),
        };
        out.push(ShaderUniform::new(name, value));
    }
    Ok(out)
}

/// `stops = [ { offset = 0.0, color = #f7971e, opacity = 1.0 }, ... ]`
/// Opacity folds into `Stop.color.a` — msx-ast's `Stop` has no separate
/// opacity field, matching `gradient.rs::to_svg()`'s use of `color.opacity()`.
pub fn parse_stops(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Stop>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let item_path = format!("{}[{}]", path, i);
        let offset = opt::<f64>(data, &item_path, "offset")?.unwrap_or(0.0);
        let mut color = color_from(raw(data, &item_path, "color")).unwrap_or(Color::BLACK);
        if let Some(op) = opt::<f64>(data, &item_path, "opacity")? {
            color.a = (op.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        out.push(Stop::new(offset, color));
    }
    Ok(out)
  }
