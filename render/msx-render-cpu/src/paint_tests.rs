// render/msx-render-cpu/src/paint_tests.rs
//! Pixel tests for gradients, text and document order on the CPU renderer.
//! Pixels are read premultiplied straight from the pixmap.

use msx_ast::{
    Canvas, Color, ConicGradient, Def, Element, Line, LinearGradient, Paint, RadialGradient, Rect, Scene, SdfNode, SdfTree, ShaderDef,
    Stop, Style, Text, TextAnchor,
};
use tiny_skia::Pixmap;

use crate::render_to_pixmap;

fn px(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * p.width() + x) * 4) as usize;
    let d = p.data();
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

fn scene(w: f64, h: f64, defs: Vec<Def>, elements: Vec<Element>) -> Scene {
    let mut s = Scene::new(Canvas::new(w, h, Color::TRANSPARENT));
    s.defs = defs;
    s.elements = elements;
    s
}

fn fill_style(paint: Paint) -> Style {
    Style { fill: Some(paint), stroke: Some(Paint::None), stroke_width: Some(0.0), opacity: Some(1.0), ..Default::default() }
}

fn stroke_style(paint: Paint, width: f64) -> Style {
    Style { fill: Some(Paint::None), stroke: Some(paint), stroke_width: Some(width), opacity: Some(1.0), ..Default::default() }
}

fn url(id: &str) -> Paint {
    Paint::Ref(format!("url(#{id})"))
}

fn rect(x: f64, y: f64, w: f64, h: f64, style: Style) -> Element {
    Element::Rect(Rect { x, y, width: w, height: h, rx: None, ry: None, id: None, transform: None, style })
}

fn red_blue() -> Vec<Stop> {
    vec![Stop::new(0.0, Color::rgb(255, 0, 0)), Stop::new(1.0, Color::rgb(0, 0, 255))]
}

fn horizontal(id: &str) -> Def {
    Def::LinearGradient(LinearGradient::new(id.into(), 0.0, 0.0, 1.0, 0.0, red_blue()))
}

fn text_el(x: f64, y: f64, content: &str, style: Style) -> Element {
    Element::Text(Text { x, y, content: content.into(), id: None, transform: None, style })
}

fn text_style(paint: Paint) -> Style {
    Style { font_size: Some(30.0), ..fill_style(paint) }
}

// ── Linear and radial ────────────────────────────────────────────────────────

#[test]
fn linear_interpolates_across_the_shape() {
    let s = scene(100.0, 20.0, vec![horizontal("g")], vec![rect(0.0, 0.0, 100.0, 20.0, fill_style(url("g")))]);
    let p = render_to_pixmap(&s);
    let (l, m, r) = (px(&p, 5, 10), px(&p, 50, 10), px(&p, 95, 10));
    assert!(l[0] > 220 && l[2] < 35, "left should be reddish, got {l:?}");
    assert!((m[0] as i32 - m[2] as i32).abs() < 20 && m[0] > 90, "middle should be mixed, got {m:?}");
    assert!(r[2] > 220 && r[0] < 35, "right should be bluish, got {r:?}");
}

#[test]
fn gradient_maps_to_the_shape_box_not_the_canvas() {
    let s = scene(300.0, 20.0, vec![horizontal("g")], vec![rect(100.0, 0.0, 100.0, 20.0, fill_style(url("g")))]);
    let p = render_to_pixmap(&s);
    let (l, r) = (px(&p, 105, 10), px(&p, 195, 10));
    assert!(l[0] > 220, "start of the box should be red, got {l:?}");
    assert!(r[2] > 220, "end of the box should be blue, got {r:?}");
}

#[test]
fn radial_centre_and_edge_differ() {
    let def = Def::RadialGradient(RadialGradient::new("g".into(), 0.5, 0.5, 0.5, 0.5, 0.5, red_blue()));
    let circle = Element::Circle(msx_ast::Circle { cx: 50.0, cy: 50.0, r: 40.0, id: None, transform: None, style: fill_style(url("g")) });
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![def], vec![circle]));
    let (c, e) = (px(&p, 50, 50), px(&p, 50, 13));
    assert!(c[0] > 200 && c[2] < 60, "centre should be red, got {c:?}");
    assert!(e[2] > 150 && e[0] < 100, "edge should be blue-ish, got {e:?}");
}

#[test]
fn gradient_stroke_on_a_diagonal_line() {
    let line = Element::Line(Line { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0, id: None, transform: None, style: stroke_style(url("g"), 8.0) });
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![horizontal("g")], vec![line]));
    let (a, b) = (px(&p, 12, 12), px(&p, 88, 88));
    assert!(a[3] > 200 && a[0] > a[2], "start of the line should be reddish, got {a:?}");
    assert!(b[3] > 200 && b[2] > b[0], "end of the line should be bluish, got {b:?}");
}

#[test]
fn horizontal_line_with_gradient_stroke_draws_nothing() {
    // The line has an empty bounding box, so objectBoundingBox units are
    // undefined. SVG paints nothing, and so does this renderer.
    let line = Element::Line(Line { x1: 10.0, y1: 10.0, x2: 90.0, y2: 10.0, id: None, transform: None, style: stroke_style(url("g"), 6.0) });
    let p = render_to_pixmap(&scene(100.0, 20.0, vec![horizontal("g")], vec![line]));
    assert_eq!(px(&p, 50, 10)[3], 0);
}

#[test]
fn gradient_respects_element_opacity() {
    let mut style = fill_style(url("g"));
    style.opacity = Some(0.5);
    let p = render_to_pixmap(&scene(100.0, 20.0, vec![horizontal("g")], vec![rect(0.0, 0.0, 100.0, 20.0, style)]));
    let a = px(&p, 50, 10)[3];
    assert!((120..=136).contains(&a), "alpha should be about half, got {a}");
}

#[test]
fn broken_reference_paints_nothing() {
    let p = render_to_pixmap(&scene(20.0, 20.0, vec![], vec![rect(0.0, 0.0, 20.0, 20.0, fill_style(url("missing")))]));
    assert_eq!(px(&p, 10, 10)[3], 0);
}

#[test]
fn shader_def_paints_its_fallback_color() {
    let def = Def::Shader(ShaderDef::new("sh", "shaders/x.wgsl", Color::rgb(107, 70, 255)));
    let p = render_to_pixmap(&scene(20.0, 20.0, vec![def], vec![rect(0.0, 0.0, 20.0, 20.0, fill_style(url("sh")))]));
    assert_eq!(px(&p, 10, 10), [107, 70, 255, 255]);
}

// ── Conic ────────────────────────────────────────────────────────────────────

fn conic_def() -> Def {
    Def::ConicGradient(ConicGradient::new("c".into(), 0.5, 0.5, 0.0, red_blue()))
}

#[test]
fn conic_fill_starts_at_top_and_wraps() {
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![conic_def()], vec![rect(0.0, 0.0, 100.0, 100.0, fill_style(url("c")))]));
    let just_right_of_top = px(&p, 53, 10);
    let bottom = px(&p, 50, 90);
    let just_left_of_top = px(&p, 47, 10);
    assert!(just_right_of_top[0] > 235 && just_right_of_top[2] < 20, "t near 0 is the first stop, got {just_right_of_top:?}");
    assert!(
        (110..=145).contains(&bottom[0]) && (110..=145).contains(&bottom[2]),
        "half a turn is the 50 percent mix, got {bottom:?}"
    );
    assert!(just_left_of_top[2] > 235 && just_left_of_top[0] < 20, "t near 1 wraps to the last stop, got {just_left_of_top:?}");
}

#[test]
fn conic_runs_clockwise() {
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![conic_def()], vec![rect(0.0, 0.0, 100.0, 100.0, fill_style(url("c")))]));
    let right = px(&p, 90, 50);
    let left = px(&p, 10, 50);
    assert!(right[0] > left[0], "a quarter turn (right) is redder than three quarters (left): {right:?} vs {left:?}");
}

#[test]
fn conic_stroke_colors_the_outline_only() {
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![conic_def()], vec![rect(10.0, 10.0, 80.0, 80.0, stroke_style(url("c"), 6.0))]));
    let edge = px(&p, 56, 10);
    assert!(edge[3] > 200 && edge[0] > edge[2], "top edge just right of centre should be red-ish, got {edge:?}");
    assert_eq!(px(&p, 50, 50)[3], 0, "interior stays empty for a stroke-only shape");
}

// ── Text ─────────────────────────────────────────────────────────────────────

fn opaque_columns(p: &Pixmap) -> Option<(u32, u32)> {
    let mut lo = u32::MAX;
    let mut hi = 0;
    for y in 0..p.height() {
        for x in 0..p.width() {
            if px(p, x, y)[3] > 0 {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
    }
    (lo != u32::MAX).then_some((lo, hi))
}

#[test]
fn text_draws_inside_its_box_and_nowhere_else() {
    let t = text_el(20.0, 40.0, "Hello", text_style(Paint::Color(Color::BLACK)));
    let p = render_to_pixmap(&scene(200.0, 60.0, vec![], vec![t]));
    let inside = (20..120).flat_map(|x| (15..45).map(move |y| (x, y))).filter(|&(x, y)| px(&p, x, y)[3] > 0).count();
    assert!(inside > 100, "expected glyph pixels in the box, got {inside}");
    let (_, right) = opaque_columns(&p).unwrap();
    assert!(right < 130, "text should not reach far right, rightmost column {right}");
    assert_eq!(px(&p, 190, 30)[3], 0);
}

#[test]
fn text_gradient_runs_left_to_right() {
    let t = text_el(10.0, 40.0, "MMMM", text_style(url("g")));
    let p = render_to_pixmap(&scene(200.0, 60.0, vec![horizontal("g")], vec![t]));
    let (lo, hi) = opaque_columns(&p).unwrap();
    let third = (hi - lo) / 3;
    let sum = |from: u32, to: u32| {
        let (mut r, mut b) = (0u32, 0u32);
        for x in from..to {
            for y in 0..p.height() {
                let c = px(&p, x, y);
                if c[3] == 255 {
                    r += c[0] as u32;
                    b += c[2] as u32;
                }
            }
        }
        (r, b)
    };
    let (lr, lb) = sum(lo, lo + third);
    let (rr, rb) = sum(hi - third, hi);
    assert!(lr > lb, "left third should be redder: r {lr} b {lb}");
    assert!(rb > rr, "right third should be bluer: r {rr} b {rb}");
}

#[test]
fn text_anchor_middle_centres_on_the_anchor() {
    let mut style = text_style(Paint::Color(Color::BLACK));
    style.text_anchor = Some(TextAnchor::Middle);
    let p = render_to_pixmap(&scene(200.0, 60.0, vec![], vec![text_el(100.0, 40.0, "HHHH", style)]));
    let (lo, hi) = opaque_columns(&p).unwrap();
    let centre = (lo + hi) as f32 / 2.0;
    assert!((centre - 100.0).abs() < 4.0, "ink centre {centre} should be near 100");
}

#[test]
fn text_stroke_draws_an_outline() {
    let style = Style { font_size: Some(40.0), ..stroke_style(Paint::Color(Color::rgb(255, 0, 0)), 2.0) };
    let p = render_to_pixmap(&scene(100.0, 60.0, vec![], vec![text_el(10.0, 45.0, "O", style)]));
    assert!(opaque_columns(&p).is_some(), "stroke-only text should still draw its outline");
}

// ── Document order ───────────────────────────────────────────────────────────

fn green_sdf_circle() -> Element {
    Element::Sdf(SdfNode::new(SdfTree::Circle { cx: 50.0, cy: 50.0, r: 30.0 }, Paint::Color(Color::rgb(0, 255, 0))))
}

fn red_cover() -> Element {
    rect(0.0, 0.0, 100.0, 100.0, fill_style(Paint::Color(Color::rgb(255, 0, 0))))
}

#[test]
fn document_order_rect_after_sdf_wins_the_overlap() {
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![], vec![green_sdf_circle(), red_cover()]));
    assert_eq!(px(&p, 50, 50), [255, 0, 0, 255]);
}

#[test]
fn document_order_sdf_after_rect_wins_the_overlap() {
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![], vec![red_cover(), green_sdf_circle()]));
    assert_eq!(px(&p, 50, 50), [0, 255, 0, 255]);
}

// ── Smooth curves ────────────────────────────────────────────────────────────

fn path_el(d: &str, style: Style) -> Element {
    Element::Path(msx_ast::Path { commands: msx_ast::path::parse_d(d).unwrap(), d_raw: d.to_string(), id: None, transform: None, style })
}

#[test]
fn smooth_quad_reflects_the_previous_control_point() {
    // The second T reflects the first one's control point (30, 30) about
    // (40, 140), which is (50, 250). Its curve passes (77.5, 167.5). With the
    // control left at the current point it would be a straight line, which
    // stays near y = 108 at that x.
    let style = Style { stroke_width: Some(4.0), ..stroke_style(Paint::Color(Color::rgb(0, 128, 0)), 4.0) };
    let p = render_to_pixmap(&scene(200.0, 200.0, vec![], vec![path_el("M 30 30 T 40 140 T 170 30", style)]));
    assert!(px(&p, 77, 167)[3] > 100, "the curved segment must pass through (77, 167)");
    assert_eq!(px(&p, 77, 108)[3], 0, "a straight segment would pass here");
}

#[test]
fn smooth_cubic_reflects_the_previous_control_point() {
    // After C ... 100 20 100 100, the S reflects (100, 20) about (100, 100)
    // to (100, 180), so the second curve bulges down through (140, 160).
    let style = Style { stroke_width: Some(4.0), ..stroke_style(Paint::Color(Color::rgb(0, 128, 0)), 4.0) };
    let d = "M 20 100 C 20 20 100 20 100 100 S 180 180 180 100";
    let p = render_to_pixmap(&scene(200.0, 200.0, vec![], vec![path_el(d, style.clone())]));
    assert!(px(&p, 140, 160)[3] > 100, "the reflected control point bulges the curve down");
    assert_eq!(px(&p, 140, 130)[3], 0);
    let rel = "m 20 100 c 0 -80 80 -80 80 0 s 80 80 80 0";
    let q = render_to_pixmap(&scene(200.0, 200.0, vec![], vec![path_el(rel, style)]));
    assert!(px(&q, 140, 160)[3] > 100, "relative commands reflect too");
}

#[test]
fn a_smooth_command_after_a_non_curve_uses_the_current_point() {
    // S after L has no control point to reflect, so the first control is the
    // current point and the segment starts straight along the line.
    let style = Style { stroke_width: Some(2.0), ..stroke_style(Paint::Color(Color::rgb(0, 128, 0)), 2.0) };
    let p = render_to_pixmap(&scene(100.0, 100.0, vec![], vec![path_el("M 10 50 L 50 50 S 90 50 90 10", style)]));
    assert!(px(&p, 30, 50)[3] > 100);
    assert!(px(&p, 70, 49)[3] > 0 || px(&p, 70, 45)[3] > 0 || px(&p, 70, 40)[3] > 0, "the curve then rises toward (90, 10)");
}
