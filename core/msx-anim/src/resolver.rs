// core/msx-anim/src/resolver.rs
//! The resolver: `resolve_at_time(scene, t) -> Scene`.
//!
//! Pure data transform, no rendering knowledge. Groups tracks by
//! `target_id`, walks the element tree once, and for every element whose
//! `id` has at least one track, composes the evaluated delta onto that
//! element's existing static state.

use std::collections::HashMap;

use msx_ast::{AnimationTrack, Color, Element, Paint, Scene, Style};

use crate::delta::AnimatedDelta;

/// Resolves an animated `Scene` at time `t` (seconds, in the scene's own
/// timeline — not yet loop-adjusted) into a fully static `Scene`.
///
/// `t` is first passed through `scene.loop_mode.resolve_time(t, duration)`,
/// so callers can hand in raw elapsed playback time and don't need to
/// pre-clamp/wrap it themselves. A scene with no tracks is returned
/// unchanged (cloned) — the common case for every pre-Phase-1 `.msx` file.
pub fn resolve_at_time(scene: &Scene, t: f64) -> Scene {
    if scene.animations.is_empty() {
        return scene.clone();
    }

    let local_t = scene.loop_mode.resolve_time(t, scene.effective_duration());

    let mut by_target: HashMap<&str, Vec<&AnimationTrack>> = HashMap::new();
    for track in &scene.animations {
        by_target.entry(track.target_id.as_str()).or_default().push(track);
    }

    let mut resolved = scene.clone();
    for element in &mut resolved.elements {
        resolve_element(element, &by_target, local_t);
    }
    resolved
}

fn resolve_element(element: &mut Element, by_target: &HashMap<&str, Vec<&AnimationTrack>>, t: f64) {
    // Recurse first: a parent group and its children are independent
    // targets, both may have tracks, and neither's resolution depends on
    // the other's (the delta composes onto whatever transform is already
    // there, static or just-resolved — order between siblings doesn't
    // matter, only parent-before-child would, and children are resolved
    // through their own `transform`/`opacity` fields regardless of what
    // their parent group does).
    match element {
        Element::Group(g) => for child in &mut g.children { resolve_element(child, by_target, t); },
        Element::Layer(l) => for child in &mut l.children { resolve_element(child, by_target, t); },
        _ => {}
    }

    let Some(id) = element.id() else { return };
    let Some(tracks) = by_target.get(id) else { return };
    let delta = AnimatedDelta::from_tracks(tracks, t);
    if delta.is_identity() {
        return;
    }

    apply_delta(element, delta);
}

fn apply_delta(element: &mut Element, delta: AnimatedDelta) {
    match element {
        Element::Rect(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
            apply_style(&mut e.style, &delta);
            apply_geometry_rect(e, &delta);
        }
        Element::Circle(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
            apply_style(&mut e.style, &delta);
            apply_geometry_circle(e, &delta);
        }
        Element::Ellipse(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
            apply_style(&mut e.style, &delta);
            apply_geometry_ellipse(e, &delta);
        }
        Element::Line(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
            apply_style(&mut e.style, &delta);
            apply_geometry_line(e, &delta);
        }
        Element::Polyline(e) | Element::Polygon(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
            apply_style(&mut e.style, &delta);
        }
        Element::Path(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
            apply_style(&mut e.style, &delta);
        }
        Element::Text(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
            apply_style(&mut e.style, &delta);
        }
        Element::Group(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            let mut style = e.style.take().unwrap_or_else(Style::empty);
            style.opacity = Some(style.opacity.unwrap_or(1.0) * delta.opacity);
            e.style = Some(style);
        }
        Element::Use(e) => {
            // No `style` field on `Use` — nothing to fold opacity into.
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
        }
        Element::Sdf(e) => {
            // `SdfNode` has no separate opacity channel (only `fill`/
            // `stroke` paints) — transform is the only animatable part
            // today.
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
        }
        Element::Splat(e) => {
            // No `Transform` field on `GaussianSplat` — TRS deltas fold
            // directly onto its own x/y/sigma/rotation fields instead.
            e.x += delta.translate_x;
            e.y += delta.translate_y;
            e.sigma_x *= delta.scale_x;
            e.sigma_y *= delta.scale_y;
            e.rotation += delta.rotate.to_radians();
            e.opacity *= delta.opacity;
        }
        Element::Layer(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            e.opacity *= delta.opacity;
            // Override, not compose — see `AnimatedDelta::z_index`'s own
            // doc for why this one channel doesn't go through the same
            // multiplicative model `opacity` just did two lines up.
            if let Some(z) = delta.z_index {
                e.z_index = z;
            }
        }
        Element::Image(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
        }
    }
}

fn apply_opacity(style: &mut Style, opacity_delta: f64) {
    style.opacity = Some(style.opacity.unwrap_or(1.0) * opacity_delta);
}

/// Writes the style channels onto a shape's own style. Each channel replaces
/// the value (a channel no track touches keeps it). A color channel starts
/// from the shape's color, or from black when the paint is not a color
/// (none, a gradient or pattern reference, `currentColor`, or unset), so the
/// first color track on such a paint turns it into a plain color. Group style
/// is not read by the CPU and GPU renderers, so groups are left alone.
/// The geometry channels, one function per shape. A size or radius never goes
/// below 0, and a channel with no track leaves the value as it is.
fn apply_geometry_rect(e: &mut msx_ast::Rect, d: &AnimatedDelta) {
    if let Some(v) = d.width {
        e.width = v.max(0.0);
    }
    if let Some(v) = d.height {
        e.height = v.max(0.0);
    }
    if let Some(v) = d.rx {
        e.rx = Some(v.max(0.0));
    }
    if let Some(v) = d.ry {
        e.ry = Some(v.max(0.0));
    }
}

fn apply_geometry_circle(e: &mut msx_ast::Circle, d: &AnimatedDelta) {
    if let Some(v) = d.r {
        e.r = v.max(0.0);
    }
}

fn apply_geometry_ellipse(e: &mut msx_ast::Ellipse, d: &AnimatedDelta) {
    if let Some(v) = d.rx {
        e.rx = v.max(0.0);
    }
    if let Some(v) = d.ry {
        e.ry = v.max(0.0);
    }
}

fn apply_geometry_line(e: &mut msx_ast::Line, d: &AnimatedDelta) {
    for (slot, v) in [(&mut e.x1, d.x1), (&mut e.y1, d.y1), (&mut e.x2, d.x2), (&mut e.y2, d.y2)] {
        if let Some(v) = v {
            *slot = v;
        }
    }
}

fn apply_style(style: &mut Style, delta: &AnimatedDelta) {
    if delta.fill.iter().any(Option::is_some) {
        style.fill = Some(Paint::Color(animated_color(style.fill.as_ref(), &delta.fill)));
    }
    if delta.stroke.iter().any(Option::is_some) {
        style.stroke = Some(Paint::Color(animated_color(style.stroke.as_ref(), &delta.stroke)));
    }
    if let Some(w) = delta.stroke_width {
        style.stroke_width = Some(w.max(0.0));
    }
    if let Some(o) = delta.dash_offset {
        style.stroke_dashoffset = Some(o);
    }
    if delta.dash_array.iter().any(Option::is_some) {
        // A one entry array is that entry twice. Entries a track leaves alone keep
        // the value of the shape, and 0 when it has no array.
        let base = style.stroke_dasharray.clone().unwrap_or_default();
        let first = base.first().copied().unwrap_or(0.0);
        let second = base.get(1).copied().unwrap_or(first);
        style.stroke_dasharray = Some(vec![
            delta.dash_array[0].unwrap_or(first).max(0.0),
            delta.dash_array[1].unwrap_or(second).max(0.0),
        ]);
    }
}

fn animated_color(paint: Option<&Paint>, channels: &[Option<f64>; 4]) -> Color {
    let base = match paint {
        Some(Paint::Color(c)) => *c,
        _ => Color::BLACK,
    };
    let pick = |value: Option<f64>, fallback: u8| value.map_or(fallback, |v| v.round().clamp(0.0, 255.0) as u8);
    Color::rgba(pick(channels[0], base.r), pick(channels[1], base.g), pick(channels[2], base.b), pick(channels[3], base.a))
}
