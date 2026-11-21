// core/msx-anim/src/tests.rs

use msx_ast::{
    AnimatedProperty, AnimationTrack, BlendMode, Canvas, Circle, Color, Element, GaussianSplat,
    Group, Image, Keyframe, Layer, LoopMode, MediaSource, Matrix2D, Paint, Rect, Scene, SdfNode,
    SdfTree, Style, Transform,
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
fn image_opacity_and_transform_animate_like_other_elements() {
    // Element::Image gets the exact same apply_delta treatment Rect/
    // Circle/etc already get (transform composes, opacity multiplies
    // onto style.opacity) — this just confirms that's actually wired up,
    // the same way opacity_track_multiplies_existing_style_opacity above
    // confirms it for Circle.
    let mut img = Image::new(MediaSource::FileRef("photo.png".to_string()), 0.0, 0.0, 50.0, 50.0);
    img.id = Some("pic".into());
    img.style.opacity = Some(0.5);
    let mut scene = blank_scene();
    scene.elements.push(Element::Image(img));
    scene.duration = 1.0;
    scene.animations.push(track(
        "pic",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 0.5), Keyframe::linear(1.0, 0.5)],
    ));
    scene.animations.push(track(
        "pic",
        AnimatedProperty::TranslateX,
        vec![Keyframe::linear(0.0, 12.0), Keyframe::linear(1.0, 12.0)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Image(img) => {
            assert!((img.style.opacity.unwrap() - 0.25).abs() < 1e-6, "0.5 static * 0.5 track = 0.25");
            let t = img.transform.as_ref().expect("translate track should have populated a transform");
            let m = t.to_matrix();
            assert!((m.e - 12.0).abs() < 1e-6, "translate_x should land in the matrix's own translation component");
        }
        _ => panic!("expected Image"),
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
fn layer_z_index_field_is_overridden_not_composed() {
    // Contrast with opacity above: a static z_index of 5.0 plus a
    // "z_index" track evaluating to 2.0 must land on exactly 2.0, not
    // 5.0*2.0 or 5.0+2.0 — z_index is a sort key, not a delta.
    let mut layer = Layer::new(vec![]).with_z_index(5.0);
    layer.id = Some("fx".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Layer(layer));
    scene.duration = 1.0;
    scene.animations.push(track(
        "fx",
        AnimatedProperty::ZIndex,
        vec![Keyframe::linear(0.0, 2.0), Keyframe::linear(1.0, 2.0)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Layer(l) => assert!((l.z_index - 2.0).abs() < 1e-6, "expected override to 2.0, got {}", l.z_index),
        _ => panic!("expected Layer"),
    }
}

#[test]
fn layer_z_index_is_untouched_without_a_track() {
    // The other half of the override-not-compose contract: no "z_index"
    // track at all must leave the static value exactly as authored, not
    // fall back to some identity/neutral number (there isn't one for a
    // sort key — see AnimatedDelta::z_index's doc).
    let mut layer = Layer::new(vec![]).with_z_index(3.5);
    layer.id = Some("fx".into());

    let mut scene = blank_scene();
    scene.elements.push(Element::Layer(layer));
    scene.duration = 1.0;
    scene.animations.push(track(
        "fx",
        AnimatedProperty::Opacity,
        vec![Keyframe::linear(0.0, 1.0), Keyframe::linear(1.0, 1.0)],
    ));

    let resolved = resolve_at_time(&scene, 0.0);
    match &resolved.elements[0] {
        Element::Layer(l) => assert!((l.z_index - 3.5).abs() < 1e-6, "expected untouched 3.5, got {}", l.z_index),
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

// ── Style channels ───────────────────────────────────────────────────────────

fn styled_rect(id: &str, fill: Paint, stroke: Paint) -> Rect {
    let style = Style { fill: Some(fill), stroke: Some(stroke), ..Style::default() };
    let mut r = Rect::new(0.0, 0.0, 10.0, 10.0, style);
    r.id = Some(id.to_string());
    r
}

fn resolved_rect_style(scene: &Scene, t: f64) -> Style {
    match &resolve_at_time(scene, t).elements[0] {
        Element::Rect(r) => r.style.clone(),
        _ => panic!("expected Rect"),
    }
}

#[test]
fn a_color_channel_replaces_only_its_own_component() {
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(styled_rect("box", Paint::Color(Color::rgba(10, 20, 30, 200)), Paint::None)));
    scene.duration = 1.0;
    scene.animations.push(track("box", AnimatedProperty::FillR, vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(1.0, 200.0)]));

    let style = resolved_rect_style(&scene, 0.5);
    assert_eq!(style.fill, Some(Paint::Color(Color::rgba(100, 20, 30, 200))));
    assert_eq!(style.stroke, Some(Paint::None), "the stroke has no track and stays as it was");
}

#[test]
fn four_fill_channels_make_a_color_cycle() {
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(styled_rect("box", Paint::Color(Color::rgb(255, 0, 0)), Paint::None)));
    scene.duration = 2.0;
    for (property, from, to) in [
        (AnimatedProperty::FillR, 255.0, 0.0),
        (AnimatedProperty::FillG, 0.0, 0.0),
        (AnimatedProperty::FillB, 0.0, 255.0),
        (AnimatedProperty::FillA, 255.0, 100.0),
    ] {
        scene.animations.push(track("box", property, vec![Keyframe::linear(0.0, from), Keyframe::linear(2.0, to)]));
    }
    assert_eq!(resolved_rect_style(&scene, 0.0).fill, Some(Paint::Color(Color::rgba(255, 0, 0, 255))));
    assert_eq!(resolved_rect_style(&scene, 1.0).fill, Some(Paint::Color(Color::rgba(128, 0, 128, 178))));
    assert_eq!(resolved_rect_style(&scene, 2.0).fill, Some(Paint::Color(Color::rgba(0, 0, 255, 100))));
}

#[test]
fn a_color_channel_on_a_paint_that_is_not_a_color_starts_from_black() {
    for paint in [Paint::None, Paint::Ref("url(#g)".into()), Paint::CurrentColor] {
        let mut scene = blank_scene();
        scene.elements.push(Element::Rect(styled_rect("box", Paint::Color(Color::WHITE), paint)));
        scene.duration = 1.0;
        scene.animations.push(track("box", AnimatedProperty::StrokeG, vec![Keyframe::linear(0.0, 90.0), Keyframe::linear(1.0, 90.0)]));
        assert_eq!(resolved_rect_style(&scene, 0.0).stroke, Some(Paint::Color(Color::rgba(0, 90, 0, 255))));
    }
}

#[test]
fn color_channels_are_rounded_and_clamped() {
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(styled_rect("box", Paint::Color(Color::BLACK), Paint::None)));
    scene.duration = 1.0;
    scene.animations.push(track("box", AnimatedProperty::FillR, vec![Keyframe::linear(0.0, 300.0), Keyframe::linear(1.0, 300.0)]));
    scene.animations.push(track("box", AnimatedProperty::FillG, vec![Keyframe::linear(0.0, -20.0), Keyframe::linear(1.0, -20.0)]));
    scene.animations.push(track("box", AnimatedProperty::FillB, vec![Keyframe::linear(0.0, 10.5), Keyframe::linear(1.0, 10.5)]));
    assert_eq!(resolved_rect_style(&scene, 0.0).fill, Some(Paint::Color(Color::rgba(255, 0, 11, 255))));
}

#[test]
fn stroke_width_and_dash_offset_replace_the_value_and_width_never_goes_negative() {
    let style = Style { stroke_width: Some(2.0), stroke_dashoffset: Some(5.0), ..Style::default() };
    let mut r = Rect::new(0.0, 0.0, 10.0, 10.0, style);
    r.id = Some("box".into());
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(r));
    scene.duration = 1.0;
    scene.animations.push(track("box", AnimatedProperty::StrokeWidth, vec![Keyframe::linear(0.0, 2.0), Keyframe::linear(1.0, -6.0)]));
    scene.animations.push(track("box", AnimatedProperty::DashOffset, vec![Keyframe::linear(0.0, 100.0), Keyframe::linear(1.0, 0.0)]));

    let mid = resolved_rect_style(&scene, 0.5);
    assert!((mid.stroke_width.unwrap() - 0.0).abs() < 1e-9, "-2 clamps to 0");
    assert!((mid.stroke_dashoffset.unwrap() - 50.0).abs() < 1e-9);
    let start = resolved_rect_style(&scene, 0.0);
    assert!((start.stroke_width.unwrap() - 2.0).abs() < 1e-9);
    assert!((start.stroke_dashoffset.unwrap() - 100.0).abs() < 1e-9);
}

#[test]
fn style_channels_leave_a_group_alone() {
    let group = Group { children: vec![Element::Rect(rect("inner"))], id: Some("g".into()), transform: None, style: None };
    let mut scene = blank_scene();
    scene.elements.push(Element::Group(group));
    scene.duration = 1.0;
    scene.animations.push(track("g", AnimatedProperty::FillR, vec![Keyframe::linear(0.0, 5.0), Keyframe::linear(1.0, 5.0)]));
    let resolved = resolve_at_time(&scene, 0.5);
    let Element::Group(g) = &resolved.elements[0] else { panic!("expected Group") };
    assert!(g.style.as_ref().is_none_or(|s| s.fill.is_none()));
}

#[test]
fn a_delta_with_only_a_style_channel_is_not_the_identity() {
    use crate::delta::AnimatedDelta;
    let t = track("x", AnimatedProperty::DashOffset, vec![Keyframe::linear(0.0, 0.0)]);
    let delta = AnimatedDelta::from_tracks(&[&t], 0.0);
    assert!(delta.has_style());
    assert!(!delta.is_identity(), "a dash offset of 0 is still an override");
    assert!(AnimatedDelta::default().is_identity());
}

// ── Geometry channels ────────────────────────────────────────────────────────

fn one_track_scene(element: Element, id: &str, property: AnimatedProperty, from: f64, to: f64) -> Scene {
    let mut scene = blank_scene();
    scene.elements.push(element);
    scene.duration = 1.0;
    scene.animations.push(track(id, property, vec![Keyframe::linear(0.0, from), Keyframe::linear(1.0, to)]));
    scene
}

#[test]
fn rect_width_height_and_corner_radii_replace_the_shape_values() {
    let mut r = Rect::new(2.0, 3.0, 10.0, 20.0, Style::default());
    r.id = Some("box".into());
    let mut scene = blank_scene();
    scene.elements.push(Element::Rect(r));
    scene.duration = 1.0;
    for (p, from, to) in [(AnimatedProperty::Width, 10.0, 50.0), (AnimatedProperty::Height, 20.0, 0.0), (AnimatedProperty::Rx, 0.0, 4.0), (AnimatedProperty::Ry, 0.0, 6.0)] {
        scene.animations.push(track("box", p, vec![Keyframe::linear(0.0, from), Keyframe::linear(1.0, to)]));
    }
    let Element::Rect(mid) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("a rect") };
    assert!((mid.width - 30.0).abs() < 1e-9 && (mid.height - 10.0).abs() < 1e-9);
    assert_eq!((mid.x, mid.y), (2.0, 3.0), "the position is not touched");
    assert!((mid.rx.unwrap() - 2.0).abs() < 1e-9 && (mid.ry.unwrap() - 3.0).abs() < 1e-9);
}

#[test]
fn sizes_and_radii_never_go_below_zero() {
    let mut r = Rect::new(0.0, 0.0, 10.0, 10.0, Style::default());
    r.id = Some("box".into());
    let scene = one_track_scene(Element::Rect(r), "box", AnimatedProperty::Width, -5.0, -5.0);
    let Element::Rect(r) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("a rect") };
    assert_eq!(r.width, 0.0);
    let mut c = Circle::new(5.0, 5.0, 3.0, Style::default());
    c.id = Some("dot".into());
    let scene = one_track_scene(Element::Circle(c), "dot", AnimatedProperty::R, -1.0, -1.0);
    let Element::Circle(c) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("a circle") };
    assert_eq!(c.r, 0.0);
}

#[test]
fn circle_radius_ellipse_radii_and_line_ends() {
    let mut c = Circle::new(5.0, 5.0, 0.0, Style::default());
    c.id = Some("dot".into());
    let scene = one_track_scene(Element::Circle(c), "dot", AnimatedProperty::R, 0.0, 8.0);
    let Element::Circle(c) = &resolve_at_time(&scene, 0.25).elements[0] else { panic!("a circle") };
    assert!((c.r - 2.0).abs() < 1e-9 && (c.cx, c.cy) == (5.0, 5.0));

    let mut e = msx_ast::Ellipse::new(5.0, 5.0, 4.0, 2.0, Style::default());
    e.id = Some("oval".into());
    let mut scene = one_track_scene(Element::Ellipse(e), "oval", AnimatedProperty::Rx, 4.0, 8.0);
    scene.animations.push(track("oval", AnimatedProperty::Ry, vec![Keyframe::linear(0.0, 2.0), Keyframe::linear(1.0, 6.0)]));
    let Element::Ellipse(e) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("an ellipse") };
    assert!((e.rx - 6.0).abs() < 1e-9 && (e.ry - 4.0).abs() < 1e-9);

    let mut l = msx_ast::Line::new(0.0, 0.0, 10.0, 10.0, Style::default());
    l.id = Some("ln".into());
    let mut scene = one_track_scene(Element::Line(l), "ln", AnimatedProperty::X2, 10.0, 20.0);
    scene.animations.push(track("ln", AnimatedProperty::Y1, vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(1.0, 4.0)]));
    let Element::Line(l) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("a line") };
    assert!((l.x2 - 15.0).abs() < 1e-9 && (l.y1 - 2.0).abs() < 1e-9 && (l.x1, l.y2) == (0.0, 10.0));
}

#[test]
fn a_geometry_channel_on_a_shape_that_does_not_have_it_changes_nothing() {
    let mut c = Circle::new(5.0, 5.0, 3.0, Style::default());
    c.id = Some("dot".into());
    let scene = one_track_scene(Element::Circle(c), "dot", AnimatedProperty::Width, 50.0, 50.0);
    let Element::Circle(c) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("a circle") };
    assert_eq!(c.r, 3.0);
}

#[test]
fn the_dash_array_channels_make_a_two_entry_array_and_keep_the_other_entry() {
    let style = Style { stroke_dasharray: Some(vec![4.0, 9.0]), ..Style::default() };
    let mut r = Rect::new(0.0, 0.0, 10.0, 10.0, style);
    r.id = Some("box".into());
    let scene = one_track_scene(Element::Rect(r), "box", AnimatedProperty::DashArray0, 0.0, 10.0);
    let Element::Rect(r) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("a rect") };
    assert_eq!(r.style.stroke_dasharray, Some(vec![5.0, 9.0]));
    // With no array and one entry set, the other entry follows the first.
    let mut bare = Rect::new(0.0, 0.0, 10.0, 10.0, Style::default());
    bare.id = Some("b".into());
    let scene = one_track_scene(Element::Rect(bare), "b", AnimatedProperty::DashArray1, 6.0, 6.0);
    let Element::Rect(r) = &resolve_at_time(&scene, 0.5).elements[0] else { panic!("a rect") };
    assert_eq!(r.style.stroke_dasharray, Some(vec![0.0, 6.0]));
}

#[test]
fn a_delta_with_only_a_geometry_channel_is_not_the_identity() {
    use crate::delta::AnimatedDelta;
    let t = track("x", AnimatedProperty::Height, vec![Keyframe::linear(0.0, 0.0)]);
    let delta = AnimatedDelta::from_tracks(&[&t], 0.0);
    assert!(delta.has_geometry() && !delta.has_style());
    assert!(!delta.is_identity());
}
