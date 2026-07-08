// core/msx-anim/src/tests.rs

use msx_ast::{
    AnimatedProperty, AnimationTrack, BlendMode, Canvas, Circle, Color, Element, GaussianSplat,
    Group, Keyframe, Layer, LoopMode, Matrix2D, Rect, Scene, Style, Transform,
};

use crate::resolve_at_time;

fn blank_scene() -> Scene {
    Scene::new(Canvas::new(200.0, 200.0, Color::WHITE))
}

fn rect(id: &str) -> Rect {
    let mut r = Rect::new(0.0, 0.0, 10.0, 10.0, Style::default());
    r.id = Some(id.to_string());
    r
}

fn track(target: &str, property: AnimatedProperty, keyframes: Vec<Keyframe>) -> AnimationTrack {
    AnimationTrack::new(target, property, keyframes)
}

#[test]
fn scene_with_no_tracks_is_returned_unchanged() {
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(rect("box")));
    let resolved = resolve_at_time(&scene, 5.0);
    assert_eq!(resolved.elements.len(), 1);
    match &resolved.elements[0] {
        Element::Rect(r) => assert!(r.transform.is_none()),
        _ => panic!("expected Rect"),
    }
}

#[test]
fn element_with_no_matching_track_is_untouched() {
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(rect("box")));
    scene.duration = 1.0;
    scene.animations.push(track(
        "other_id",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(1.0, 1.0)],
    ));
    let resolved = resolve_at_time(&scene, 0.5);
    match &resolved.elements[0] {
        Element::Rect(r) => assert!(r.transform.is_none()),
        _ => panic!("expected Rect"),
    }
}

#[test]
fn translate_track_composes_onto_absent_static_transform() {
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(rect("box")));
    scene.duration = 2.0;
    scene.animations.push(track(
        "box",
        AnimatedProperty::TranslateX,
        vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(2.0, 100.0)],
    ));

    let resolved = resolve_at_time(&scene, 1.0);
    match &resolved.elements[0] {
        Element::Rect(r) => match r.transform.as_ref().unwrap() {
            Transform::Matrix(m) => assert!((m.e - 50.0).abs() < 1e-6, "expected e=50, got {}", m.e),
            other => panic!("expected Matrix, got {:?}", other),
        },
        _ => panic!("expected Rect"),
    }
}

#[test]
fn animated_translate_adds_onto_existing_static_translate() {
    let mut r = rect("box");
    r.transform = Some(Transform::Translate { x: 10.0, y: 0.0 });
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(r));
    scene.duration = 1.0;
    scene.animations.push(track(
        "box",
        AnimatedProperty::TranslateX,
        vec![Keyframe::linear(0.0, 5.0), Keyframe::linear(1.0, 5.0)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Rect(r) => match r.transform.as_ref().unwrap() {
            Transform::Matrix(m) => assert!((m.e - 15.0).abs() < 1e-6, "expected static 10 + animated 5 = 15, got {}", m.e),
            other => panic!("expected Matrix, got {:?}", other),
        },
        _ => panic!("expected Rect"),
    }
}

#[test]
fn rotate_delta_pivots_the_whole_statically_positioned_element_around_the_parent_origin() {
    let mut r = rect("box");
    r.transform = Some(Transform::Translate { x: 100.0, y: 100.0 });
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(r));
    scene.duration = 1.0;
    scene.animations.push(track(
        "box",
        AnimatedProperty::Rotate,
        vec![Keyframe::linear(0.0, 90.0), Keyframe::linear(1.0, 90.0)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Rect(r) => match r.transform.as_ref().unwrap() {
            Transform::Matrix(m) => {
                let p = m.transform_point(msx_ast::Point::new(0.0, 0.0));
                assert!((p.x - (-100.0)).abs() < 1e-6, "expected x=-100, got {}", p.x);
                assert!((p.y - 100.0).abs() < 1e-6, "expected y=100, got {}", p.y);
            }
            other => panic!("expected Matrix, got {:?}", other),
        },
        _ => panic!("expected Rect"),
    }
}

#[test]
fn opacity_track_multiplies_existing_style_opacity() {
    let mut c = Circle::new(0.0, 0.0, 5.0, Style::default());
    c.id = Some("dot".into());
    c.style.opacity = Some(0.5);
    let mut scene = blank_scene();
    scene.elements.push(Element::Circle(c));
    scene.duration = 1.0;
    scene.animations.push(track(
        "dot",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 0.5), Keyframe::linear(1.0, 0.5)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Circle(c) => assert!((c.style.opacity.unwrap() - 0.25).abs() < 1e-6),
        _ => panic!("expected Circle"),
    }
}

#[test]
fn tracks_recurse_into_group_children() {
    let mut inner = rect("inner");
    inner.style = Style::default();
    let mut group = Group::new(vec![Element::Rect(inner)]);
    group.id = Some("outer".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Group(group));
    scene.duration = 1.0;
    scene.animations.push(track(
        "inner",
        AnimatedProperty::TranslateY,
        vec![Keyframe::linear(0.0, 40.0), Keyframe::linear(1.0, 40.0)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Group(g) => match &g.children[0] {
            Element::Rect(r) => match r.transform.as_ref().unwrap() {
                Transform::Matrix(m) => assert!((m.f - 40.0).abs() < 1e-6),
                other => panic!("expected Matrix, got {:?}", other),
            },
            _ => panic!("expected Rect"),
        },
        _ => panic!("expected Group"),
    }
}

#[test]
fn layer_opacity_field_is_multiplied_directly() {
    let mut layer = Layer::new(vec![]).with_blend(BlendMode::Multiply).with_opacity(0.8);
    layer.id = Some("fx".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Layer(layer));
    scene.duration = 1.0;
    scene.animations.push(track(
        "fx",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 0.5), Keyframe::linear(1.0, 0.5)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Layer(l) => assert!((l.opacity - 0.4).abs() < 1e-6),
        _ => panic!("expected Layer"),
    }
}

#[test]
fn splat_fields_are_updated_directly_since_it_has_no_transform() {
    let mut splat = GaussianSplat::circle(50.0, 50.0, 20.0, Color::WHITE, 1.0);
    splat.id = Some("blob".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Splat(splat));
    scene.duration = 1.0;
    scene.animations.push(track(
        "blob",
        AnimatedProperty::TranslateX,
        vec![Keyframe::linear(0.0, 10.0), Keyframe::linear(1.0, 10.0)],
    ));
    scene.animations.push(track(
        "blob",
        AnimatedProperty::ScaleX,
        vec![Keyframe::linear(0.0, 2.0), Keyframe::linear(1.0, 2.0)],
    ));
    scene.animations.push(track(
        "blob",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 0.5), Keyframe::linear(1.0, 0.5)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Splat(s) => {
            assert!((s.x - 60.0).abs() < 1e-6);
            assert!((s.sigma_x - 40.0).abs() < 1e-6);
            assert!((s.opacity - 0.5).abs() < 1e-6);
        }
        _ => panic!("expected Splat"),
    }
}

#[test]
fn loop_mode_wraps_time_before_evaluating_tracks() {
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(rect("box")));
    scene.duration = 2.0;
    scene.loop_mode = LoopMode::Loop;
    scene.animations.push(track(
        "box",
        AnimatedProperty::TranslateX,
        vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(2.0, 100.0)],
    ));

    // t=5.0 with duration 2.0 wraps to local_t=1.0 -> halfway -> 50.0
    let resolved = resolve_at_time(&scene, 5.0);
    match &resolved.elements[0] {
        Element::Rect(r) => match r.transform.as_ref().unwrap() {
            Transform::Matrix(m) => assert!((m.e - 50.0).abs() < 1e-6, "expected e=50, got {}", m.e),
            other => panic!("expected Matrix, got {:?}", other),
        },
        _ => panic!("expected Rect"),
    }
}

#[test]
fn identity_matrix_check_sanity() {
    assert!(Matrix2D::identity().is_identity());
                          }
