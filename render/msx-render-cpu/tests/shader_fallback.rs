// render/msx-render-cpu/tests/shader_fallback.rs
//! Pixel-level proof that a shape filled/stroked with `Paint::Ref` pointing
//! at a `Def::Shader` actually paints something — not just "the code
//! compiles". Covers both rendering paths in this crate, since they used
//! to disagree: `rasterizer.rs::resolve_paint` (every SVG-native shape —
//! Rect/Circle/Path/...) already resolved `Paint::Ref` against `defs`
//! before this shader work started; `sdf_raster.rs::paint_to_color` did
//! not — it unconditionally returned transparent for *any* `Paint::Ref`,
//! gradient or shader, since defs was never threaded into the SDF call
//! path at all. Fixed alongside these tests; see `sdf_raster.rs` history.

use msx_ast::{
    Canvas, Color, Def, Element, Paint, Rect, Scene, SdfNode, SdfTree, ShaderDef, Stop, Style,
    LinearGradient,
};
use msx_render_cpu::render_to_pixmap;

fn blank_scene(w: f64, h: f64) -> Scene {
    Scene::new(Canvas::new(w, h, Color::BLACK))
}

/// tiny-skia stores premultiplied color; for a fully-opaque source over a
/// fully-opaque background, premultiplied == straight, so a direct
/// component compare against the expected `Color` is exact.
fn assert_pixel_is(pixmap: &tiny_skia::Pixmap, x: u32, y: u32, expected: Color) {
    let p = pixmap.pixel(x, y).expect("pixel in bounds");
    assert_eq!(
        (p.red(), p.green(), p.blue()),
        (expected.r, expected.g, expected.b),
        "pixel ({x},{y}) = ({}, {}, {}), expected ({}, {}, {})",
        p.red(), p.green(), p.blue(), expected.r, expected.g, expected.b
    );
}

#[test]
fn rect_filled_with_shader_ref_paints_fallback_color() {
    let fallback = Color::rgb(107, 70, 255);
    let mut scene = blank_scene(100.0, 100.0);
    scene.defs.push(Def::Shader(ShaderDef::new("plasma", "x.wgsl", fallback)));

    let mut style = Style::default();
    style.fill = Some(Paint::Ref("url(#plasma)".to_string()));
    style.stroke = Some(Paint::None);
    scene.elements.push(Element::Rect(Rect::new(10.0, 10.0, 50.0, 50.0, style)));

    let pixmap = render_to_pixmap(&scene);
    assert_pixel_is(&pixmap, 35, 35, fallback); // well inside the rect
}

#[test]
fn sdf_filled_with_shader_ref_paints_fallback_color() {
    // The path that was actually broken before this test existed: an SDF
    // node's fill never resolved Paint::Ref against defs at all.
    let fallback = Color::rgb(45, 212, 255);
    let mut scene = blank_scene(100.0, 100.0);
    scene.defs.push(Def::Shader(ShaderDef::new("plasma", "x.wgsl", fallback)));

    let mut sdf = SdfNode::new(SdfTree::Circle { cx: 50.0, cy: 50.0, r: 30.0 }, Paint::Ref("url(#plasma)".to_string()));
    sdf.id = Some("blob".into());
    scene.elements.push(Element::Sdf(sdf));

    let pixmap = render_to_pixmap(&scene);
    assert_pixel_is(&pixmap, 50, 50, fallback); // dead center of the circle
}

#[test]
fn sdf_stroke_and_fill_resolve_independently_against_different_defs() {
    let fill_fallback = Color::rgb(45, 212, 255);
    let mut scene = blank_scene(120.0, 120.0);
    scene.defs.push(Def::Shader(ShaderDef::new("plasma", "x.wgsl", fill_fallback)));
    scene.defs.push(Def::LinearGradient(LinearGradient::new(
        "grad".into(), 0.0, 0.0, 1.0, 0.0,
        vec![Stop::new(0.0, Color::rgb(0, 0, 0)), Stop::new(1.0, Color::rgb(200, 0, 0))],
    )));

    let mut sdf = SdfNode::new(SdfTree::Ring { cx: 60.0, cy: 60.0, r: 40.0, thickness: 10.0 }, Paint::Ref("url(#plasma)".to_string()));
    sdf.id = Some("ringed".into());
    sdf.stroke = Some(Paint::Ref("url(#grad)".to_string()));
    sdf.stroke_width = Some(4.0);
    scene.elements.push(Element::Sdf(sdf));

    let pixmap = render_to_pixmap(&scene);
    // Fill (the ring band itself, at its inner edge) should be the shader's
    // fallback — nowhere near the gradient's average color (100,0,0).
    assert_pixel_is(&pixmap, 60, 100, fill_fallback);
}

#[test]
fn unresolvable_ref_paints_nothing_rather_than_panicking() {
    let mut scene = blank_scene(60.0, 60.0);
    // Deliberately no matching def — "url(#does_not_exist)".
    let mut style = Style::default();
    style.fill = Some(Paint::Ref("url(#does_not_exist)".to_string()));
    scene.elements.push(Element::Rect(Rect::new(0.0, 0.0, 60.0, 60.0, style)));

    let pixmap = render_to_pixmap(&scene); // must not panic
    // Background (black) shows through since nothing painted.
    assert_pixel_is(&pixmap, 30, 30, Color::BLACK);
}

#[test]
fn sdf_gradient_ref_still_works_after_the_defs_threading_fix() {
    // Regression guard: fixing the shader-specific gap must not disturb
    // the pre-existing (if simplified — flat average-of-stops) gradient
    // handling for SDF shapes.
    let mut scene = blank_scene(100.0, 100.0);
    scene.defs.push(Def::LinearGradient(LinearGradient::new(
        "grad".into(), 0.0, 0.0, 1.0, 0.0,
        vec![Stop::new(0.0, Color::rgb(0, 0, 0)), Stop::new(1.0, Color::rgb(100, 100, 100))],
    )));
    let mut sdf = SdfNode::new(SdfTree::Circle { cx: 50.0, cy: 50.0, r: 30.0 }, Paint::Ref("url(#grad)".to_string()));
    sdf.id = Some("g".into());
    scene.elements.push(Element::Sdf(sdf));

    let pixmap = render_to_pixmap(&scene);
    assert_pixel_is(&pixmap, 50, 50, Color::rgb(50, 50, 50)); // average of the two stops
}
