// core/msx-parser/src/animation.rs
//! Parses the top-level `animations::`/`animations = [...]` array, plus the
//! sibling scalar keys `duration` and `loop_mode` — all three sit directly
//! under `@DATA(...)` alongside `elements::`/`defs::`, not inside a
//! separate `@ANIMATIONS` section. DixScript only has one top-level data
//! section per file (`@DATA`); everything else is just another key in it,
//! same as `defs`/`elements` already are — see `parse_scene_from_data`.

use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{AnimatedProperty, AnimationTrack, Easing, Keyframe, LoopMode};

use crate::dix_helpers::{array_len, opt};

/// `duration = 1.5` — optional, defaults to `0.0` (meaning "infer from the
/// latest keyframe", same as `Scene::new`'s default — see
/// `Scene::effective_duration`).
pub fn parse_duration(data: &DixData) -> Result<f64, String> {
    Ok(opt::<f64>(data, "", "duration")?.unwrap_or(0.0))
}

/// `loop_mode = "loop"` — optional, defaults to `LoopMode::Once`. Unknown
/// strings also fall back to `Once` (`LoopMode::parse` is infallible by
/// design), so a typo here degrades to "plays once" rather than a hard
/// parse error.
pub fn parse_loop_mode(data: &DixData) -> Result<LoopMode, String> {
    Ok(opt::<String>(data, "", "loop_mode")?
        .map(|s| LoopMode::parse(&s))
        .unwrap_or_default())
}

/// `animations::` (or `animations = [...]`) — array of
/// `{ target_id, property, keyframes: [...] }`.
pub fn parse_animations(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<AnimationTrack>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(parse_animation_track(data, &format!("{}[{}]", path, i))?);
    }
    Ok(out)
}

fn parse_animation_track(data: &DixData, prefix: &str) -> Result<AnimationTrack, String> {
    let target_id = opt::<String>(data, prefix, "target_id")?
        .ok_or_else(|| format!("{}.target_id: required", prefix))?;

    let property_str = opt::<String>(data, prefix, "property")?
        .ok_or_else(|| format!("{}.property: required", prefix))?;
    let property = AnimatedProperty::parse(&property_str).ok_or_else(|| {
        format!(
            "{}.property: unknown property '{}' (expected one of: translate_x, translate_y, scale_x, scale_y, rotate, opacity)",
            prefix, property_str
        )
    })?;

    let keyframes = parse_keyframes(data, prefix, "keyframes")?;
    if keyframes.is_empty() {
        return Err(format!("{}.keyframes: at least one keyframe is required", prefix));
    }

    Ok(AnimationTrack::new(target_id, property, keyframes))
}

/// `keyframes = [ { time = 0.0, value = 0.0, easing = "ease_in_out" }, ... ]`
/// `easing` is optional per-keyframe and defaults to `Linear`
/// (`Easing::parse` is infallible, same fallback shape as `LoopMode::parse`
/// above).
fn parse_keyframes(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Keyframe>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let item_path = format!("{}[{}]", path, i);
        let time = opt::<f64>(data, &item_path, "time")?
            .ok_or_else(|| format!("{}.time: required", item_path))?;
        let value = opt::<f64>(data, &item_path, "value")?
            .ok_or_else(|| format!("{}.value: required", item_path))?;
        let easing = opt::<String>(data, &item_path, "easing")?
            .map(|s| Easing::parse(&s))
            .unwrap_or(Easing::Linear);
        out.push(Keyframe::new(time, value, easing));
    }
    Ok(out)
                        }
