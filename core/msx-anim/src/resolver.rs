// core/msx-anim/src/resolver.rs
//! The resolver: `resolve_at_time(scene, t) -> Scene`.
//!
//! Pure data transform, no rendering knowledge. Groups tracks by
//! `target_id`, walks the element tree once, and for every element whose
//! `id` has at least one track, composes the evaluated delta onto that
//! element's existing static state.

use std::collections::HashMap;

use msx_ast::{AnimationTrack, Element, Scene, Style};

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
        }
        Element::Circle(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
        }
        Element::Ellipse(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
        }
        Element::Line(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
        }
        Element::Polyline(e) | Element::Polygon(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
        }
        Element::Path(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
        }
        Element::Text(e) => {
            e.transform = Some(delta.compose_transform(e.transform.as_ref()));
            apply_opacity(&mut e.style, delta.opacity);
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
    }
}

fn apply_opacity(style: &mut Style, opacity_delta: f64) {
    style.opacity = Some(style.opacity.unwrap_or(1.0) * opacity_delta);
}
