// core/msx-anim/src/tests.rs

use msx_ast::{
    AnimatedProperty, AnimationTrack, BlendMode, Canvas, Circle, Color, Element, GaussianSplat,
    Group, Keyframe, Layer, LoopMode, Matrix2D, Paint, Rect, Scene, SdfNode, SdfTree, Style,
    Transform,
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
fn sdf_transform_animates_like_other_elements() {
    let mut sdf = SdfNode::new(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 40.0 }, Paint::Color(Color::WHITE));
    sdf.id = Some("blob_sdf".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Sdf(sdf));
    scene.duration = 2.0;
    scene.animations.push(track(
        "blob_sdf",
        AnimatedProperty::TranslateX,
        vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(2.0, 80.0)],
    ));

    let resolved = resolve_at_time(&scene, 1.0);
    match &resolved.elements[0] {
        Element::Sdf(s) => {
            assert_eq!(s.id.as_deref(), Some("blob_sdf"));
            // Tree/fill must be untouched — only the transform moves.
            assert!(matches!(s.tree, SdfTree::Circle { r, .. } if (r - 40.0).abs() < 1e-9));
            match s.transform.as_ref().unwrap() {
                Transform::Matrix(m) => assert!((m.e - 40.0).abs() < 1e-6, "expected e=40, got {}", m.e),
                other => panic!("expected Matrix, got {:?}", other),
            }
        }
        _ => panic!("expected Sdf"),
    }
}

#[test]
fn mixed_scene_animates_sdf_and_splat_independently() {
    // One SDF blob sliding right, one Gaussian splat pulsing/fading — two
    // completely different composition paths (Transform-based vs raw-
    // field-based, see `apply_delta`'s Sdf/Splat arms) driven by separate
    // tracks in the same `resolve_at_time` call. Neither should leak into
    // the other's fields.
    let mut sdf = SdfNode::new(
        SdfTree::Box { x: 0.0, y: 0.0, width: 30.0, height: 30.0, corner_radius: 4.0 },
        Paint::Color(Color::rgb(255, 136, 0)),
    );
    sdf.id = Some("sdf_a".into());

    let mut splat = GaussianSplat::circle(50.0, 50.0, 15.0, Color::WHITE, 1.0);
    splat.id = Some("splat_a".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Sdf(sdf));
    scene.elements.push(Element::Splat(splat));
    scene.duration = 1.0;

    scene.animations.push(track(
        "sdf_a",
        AnimatedProperty::TranslateX,
        vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(1.0, 60.0)],
    ));
    scene.animations.push(track(
        "sdf_a",
        AnimatedProperty::Rotate,
        vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(1.0, 45.0)],
    ));
    scene.animations.push(track(
        "splat_a",
        AnimatedProperty::ScaleX,
        vec![Keyframe::linear(0.0, 1.0), Keyframe::linear(1.0, 2.0)],
    ));
    scene.animations.push(track(
        "splat_a",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 1.0), Keyframe::linear(1.0, 0.2)],
    ));

    let resolved = resolve_at_time(&scene, 1.0);
    assert_eq!(resolved.elements.len(), 2);

    match &resolved.elements[0] {
        Element::Sdf(s) => match s.transform.as_ref().unwrap() {
            Transform::Matrix(m) => {
                let p = m.transform_point(msx_ast::Point::new(0.0, 0.0));
                assert!((p.x - 60.0).abs() < 1e-6, "sdf translate_x wrong: {}", p.x);
            }
            other => panic!("expected Matrix, got {:?}", other),
        },
        _ => panic!("expected Sdf at index 0"),
    }

    match &resolved.elements[1] {
        Element::Splat(s) => {
            // Splat has no `transform` — the rotate track on "sdf_a" must
            // not have touched it, and its own translate/y/rotation stay
            // at identity since only scale_x + opacity were tracked.
            assert!((s.sigma_x - 30.0).abs() < 1e-6, "splat_a scale_x wrong: {}", s.sigma_x);
            assert!((s.sigma_y - 15.0).abs() < 1e-6, "splat_a scale_y should be untouched: {}", s.sigma_y);
            assert!((s.opacity - 0.2).abs() < 1e-6, "splat_a opacity wrong: {}", s.opacity);
            assert!((s.x - 50.0).abs() < 1e-6, "splat_a x should be untouched: {}", s.x);
            assert!((s.rotation - 0.0).abs() < 1e-6, "splat_a rotation should be untouched: {}", s.rotation);
        }
        _ => panic!("expected Splat at index 1"),
    }
}

#[test]
fn group_containing_sdf_and_splat_children_animate_independently_of_the_group() {
    let mut sdf = SdfNode::new(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 20.0 }, Paint::Color(Color::WHITE));
    sdf.id = Some("child_sdf".into());

    let mut splat = GaussianSplat::circle(0.0, 0.0, 10.0, Color::WHITE, 1.0);
    splat.id = Some("child_splat".into());

    let mut group = Group::new(vec![Element::Sdf(sdf), Element::Splat(splat)]);
    group.id = Some("fx_group".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Group(group));
    scene.duration = 1.0;

    // The group itself fades...
    scene.animations.push(track(
        "fx_group",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 1.0), Keyframe::linear(1.0, 0.5)],
    ));
    // ...independently of its SDF child sliding down...
    scene.animations.push(track(
        "child_sdf",
        AnimatedProperty::TranslateY,
        vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(1.0, 25.0)],
    ));
    // ...and its splat child growing.
    scene.animations.push(track(
        "child_splat",
        AnimatedProperty::ScaleY,
        vec![Keyframe::linear(0.0, 1.0), Keyframe::linear(1.0, 3.0)],
    ));

    let resolved = resolve_at_time(&scene, 1.0);
    match &resolved.elements[0] {
        Element::Group(g) => {
            assert!((g.style.as_ref().unwrap().opacity.unwrap() - 0.5).abs() < 1e-6);
            assert_eq!(g.children.len(), 2);

            match &g.children[0] {
                Element::Sdf(s) => match s.transform.as_ref().unwrap() {
                    Transform::Matrix(m) => assert!((m.f - 25.0).abs() < 1e-6, "child_sdf translate_y wrong: {}", m.f),
                    other => panic!("expected Matrix, got {:?}", other),
                },
                _ => panic!("expected Sdf child"),
            }

            match &g.children[1] {
                Element::Splat(s) => assert!((s.sigma_y - 30.0).abs() < 1e-6, "child_splat scale_y wrong: {}", s.sigma_y),
                _ => panic!("expected Splat child"),
            }
        }
        _ => panic!("expected Group"),
    }
}

#[test]
fn identity_matrix_check_sanity() {
    assert!(Matrix2D::identity().is_identity());
}
