// render/msx-render-gpu/src/paint_tests.rs
//! Pixel tests for gradients, text, document order and `use` on the GPU
//! renderer. Each test skips when no GPU adapter is available, like the
//! other render tests in this crate. Scenes use an opaque background, so
//! the read-back pixels are plain RGB.

use msx_ast::{
    Canvas, Circle, Color, ConicGradient, Def, Element, Line, LinearGradient, Paint, RadialGradient, Rect, Scene, SdfNode, SdfTree,
    Stop, Style, Text, TextAnchor, Use,
};
use msx_render_core::{RenderTarget, Renderer};

use crate::GpuRenderer;

fn gpu() -> Option<GpuRenderer> {
    match GpuRenderer::new() {
        Ok(r) => Some(r),
        Err(_) => {
            eprintln!("skipping: no GPU adapter available in this environment");
            None
        }
    }
}

fn scene(w: f64, h: f64, defs: Vec<Def>, elements: Vec<Element>) -> Scene {
    let mut s = Scene::new(Canvas::new(w, h, Color::BLACK));
    s.defs = defs;
    s.elements = elements;
    s
}

fn render_gpu(r: &GpuRenderer, s: &Scene) -> RenderTarget {
    let mut t = RenderTarget::new(s.canvas.width as u32, s.canvas.height as u32);
    r.render(s, &mut t);
    t
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

fn circle(cx: f64, cy: f64, r: f64, style: Style) -> Element {
    Element::Circle(Circle { cx, cy, r, id: None, transform: None, style })
}

fn red_blue() -> Vec<Stop> {
    vec![Stop::new(0.0, Color::rgb(255, 0, 0)), Stop::new(1.0, Color::rgb(0, 0, 255))]
}

fn horizontal(id: &str) -> Def {
    Def::LinearGradient(LinearGradient::new(id.into(), 0.0, 0.0, 1.0, 0.0, red_blue()))
}

fn conic_def() -> Def {
    Def::ConicGradient(ConicGradient::new("c".into(), 0.5, 0.5, 0.0, red_blue()))
}

fn text_el(x: f64, y: f64, content: &str, style: Style) -> Element {
    Element::Text(Text { x, y, content: content.into(), id: None, transform: None, style })
}

fn text_style(paint: Paint) -> Style {
    Style { font_size: Some(30.0), ..fill_style(paint) }
}

// ── Gradients ────────────────────────────────────────────────────────────────

#[test]
fn linear_interpolates_across_the_shape() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(100.0, 20.0, vec![horizontal("g")], vec![rect(0.0, 0.0, 100.0, 20.0, fill_style(url("g")))]));
    let (l, m, e) = (t.get_pixel(5, 10), t.get_pixel(50, 10), t.get_pixel(95, 10));
    assert!(l[0] > 220 && l[2] < 35, "left should be reddish, got {l:?}");
    assert!((m[0] as i32 - m[2] as i32).abs() < 20 && m[0] > 90, "middle should be mixed, got {m:?}");
    assert!(e[2] > 220 && e[0] < 35, "right should be bluish, got {e:?}");
}

#[test]
fn gradient_maps_to_the_shape_box_not_the_canvas() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(300.0, 20.0, vec![horizontal("g")], vec![rect(100.0, 0.0, 100.0, 20.0, fill_style(url("g")))]));
    assert!(t.get_pixel(105, 10)[0] > 220, "start of the box should be red");
    assert!(t.get_pixel(195, 10)[2] > 220, "end of the box should be blue");
}

#[test]
fn gradient_follows_a_transform() {
    let Some(r) = gpu() else { return };
    let mut style = fill_style(url("g"));
    style.opacity = Some(1.0);
    let el = Element::Rect(Rect {
        x: 0.0, y: 0.0, width: 100.0, height: 20.0, rx: None, ry: None, id: None,
        transform: Some(msx_ast::Transform::Translate { x: 100.0, y: 0.0 }),
        style,
    });
    let t = render_gpu(&r, &scene(300.0, 20.0, vec![horizontal("g")], vec![el]));
    assert!(t.get_pixel(105, 10)[0] > 220 && t.get_pixel(195, 10)[2] > 220);
    assert_eq!(t.get_pixel(50, 10), [0, 0, 0, 255], "nothing is drawn at the untranslated position");
}

#[test]
fn radial_centre_and_edge_differ() {
    let Some(r) = gpu() else { return };
    let def = Def::RadialGradient(RadialGradient::new("g".into(), 0.5, 0.5, 0.5, 0.5, 0.5, red_blue()));
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![def], vec![circle(50.0, 50.0, 40.0, fill_style(url("g")))]));
    let (c, e) = (t.get_pixel(50, 50), t.get_pixel(50, 13));
    assert!(c[0] > 200 && c[2] < 60, "centre should be red, got {c:?}");
    assert!(e[2] > 150 && e[0] < 100, "edge should be blue-ish, got {e:?}");
}

#[test]
fn radial_with_an_off_centre_focal_point_shifts_the_highlight() {
    let Some(r) = gpu() else { return };
    let def = Def::RadialGradient(RadialGradient::new("g".into(), 0.5, 0.5, 0.5, 0.3, 0.3, red_blue()));
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![def], vec![rect(0.0, 0.0, 100.0, 100.0, fill_style(url("g")))]));
    let focal = t.get_pixel(30, 30);
    let opposite = t.get_pixel(70, 70);
    assert!(focal[0] > opposite[0], "the focal point is the start color: {focal:?} vs {opposite:?}");
}

#[test]
fn conic_fill_starts_at_top_runs_clockwise_and_wraps() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![conic_def()], vec![rect(0.0, 0.0, 100.0, 100.0, fill_style(url("c")))]));
    let (a, b, c) = (t.get_pixel(53, 10), t.get_pixel(50, 90), t.get_pixel(47, 10));
    assert!(a[0] > 235 && a[2] < 20, "t near 0 is the first stop, got {a:?}");
    assert!((110..=145).contains(&b[0]) && (110..=145).contains(&b[2]), "half a turn is the 50 percent mix, got {b:?}");
    assert!(c[2] > 235 && c[0] < 20, "t near 1 wraps to the last stop, got {c:?}");
    let (right, left) = (t.get_pixel(90, 50), t.get_pixel(10, 50));
    assert!(right[0] > left[0], "a quarter turn (right) is redder than three quarters (left)");
}

#[test]
fn conic_sweep_is_not_stretched_by_a_wide_box() {
    // On a 200x50 box, the pixel at (150, 1) sits 50.5 right of and 23.5
    // above the centre, so its local angle is 25 degrees right of the top
    // and t is about 0.18. A sweep stretched to the box shape would put it
    // near t = 0.08 instead.
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(200.0, 50.0, vec![conic_def()], vec![rect(0.0, 0.0, 200.0, 50.0, fill_style(url("c")))]));
    let p = t.get_pixel(150, 1);
    assert!((200..=218).contains(&(p[0] as i32)) && (38..=56).contains(&(p[2] as i32)), "expected about (209, 0, 46) at t = 0.18, got {p:?}");
}

#[test]
fn gradient_stroke_on_a_diagonal_line() {
    let Some(r) = gpu() else { return };
    let line = Element::Line(Line { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0, id: None, transform: None, style: stroke_style(url("g"), 8.0) });
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![horizontal("g")], vec![line]));
    let (a, b) = (t.get_pixel(12, 12), t.get_pixel(88, 88));
    assert!(a[0] > a[2] && a[0] > 150, "start of the line should be reddish, got {a:?}");
    assert!(b[2] > b[0] && b[2] > 150, "end of the line should be bluish, got {b:?}");
}

#[test]
fn horizontal_line_with_gradient_stroke_draws_nothing() {
    let Some(r) = gpu() else { return };
    let line = Element::Line(Line { x1: 10.0, y1: 10.0, x2: 90.0, y2: 10.0, id: None, transform: None, style: stroke_style(url("g"), 6.0) });
    let t = render_gpu(&r, &scene(100.0, 20.0, vec![horizontal("g")], vec![line]));
    assert_eq!(t.get_pixel(50, 10), [0, 0, 0, 255]);
}

#[test]
fn gradient_respects_element_opacity() {
    let Some(r) = gpu() else { return };
    let mut style = fill_style(url("g"));
    style.opacity = Some(0.5);
    let t = render_gpu(&r, &scene(100.0, 20.0, vec![horizontal("g")], vec![rect(0.0, 0.0, 100.0, 20.0, style)]));
    // Mid gradient is about (128, 0, 128) at full opacity, half of that over black.
    let m = t.get_pixel(50, 10);
    assert!((50..=78).contains(&(m[0] as i32)) && (50..=78).contains(&(m[2] as i32)), "got {m:?}");
}

#[test]
fn flat_and_gradient_shapes_keep_document_order() {
    // flat red, then gradient, then flat green, all overlapping at the
    // centre. The last one painted must win.
    let Some(r) = gpu() else { return };
    let red = rect(0.0, 0.0, 60.0, 60.0, fill_style(Paint::Color(Color::rgb(255, 0, 0))));
    let grad = rect(10.0, 10.0, 60.0, 60.0, fill_style(url("g")));
    let green = rect(20.0, 20.0, 60.0, 60.0, fill_style(Paint::Color(Color::rgb(0, 255, 0))));
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![horizontal("g")], vec![red, grad, green]));
    assert_eq!(t.get_pixel(40, 40), [0, 255, 0, 255], "green was painted last");
    let only_grad = t.get_pixel(65, 15);
    assert!(only_grad[2] > only_grad[0], "right end of the gradient box is bluish, got {only_grad:?}");
    let grad_over_red = t.get_pixel(12, 12);
    assert!(grad_over_red[0] > 200 && grad_over_red[2] < 60, "gradient start covers the red square, got {grad_over_red:?}");
}

#[test]
fn broken_reference_paints_nothing() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(20.0, 20.0, vec![], vec![rect(0.0, 0.0, 20.0, 20.0, fill_style(url("missing")))]));
    assert_eq!(t.get_pixel(10, 10), [0, 0, 0, 255]);
}

// ── Text ─────────────────────────────────────────────────────────────────────

fn ink_bounds(t: &RenderTarget) -> Option<(u32, u32, u32, u32, usize)> {
    let (mut x0, mut y0, mut x1, mut y1, mut n) = (u32::MAX, u32::MAX, 0, 0, 0usize);
    for y in 0..t.height {
        for x in 0..t.width {
            if t.get_pixel(x, y) != [0, 0, 0, 255] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
                n += 1;
            }
        }
    }
    (n > 0).then_some((x0, y0, x1, y1, n))
}

/// Covered area of white-on-black ink: the red channel summed as coverage.
fn ink_area(t: &RenderTarget) -> f32 {
    let mut sum = 0.0f32;
    for y in 0..t.height {
        for x in 0..t.width {
            sum += t.get_pixel(x, y)[0] as f32 / 255.0;
        }
    }
    sum
}

#[test]
fn text_draws_inside_its_box_and_nowhere_else() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(200.0, 60.0, vec![], vec![text_el(20.0, 40.0, "Hello", text_style(Paint::Color(Color::WHITE)))]));
    let (x0, _, x1, _, n) = ink_bounds(&t).expect("text should draw something");
    assert!(n > 100, "expected glyph pixels, got {n}");
    assert!(x0 >= 18 && x1 < 130, "ink should stay near the text box, got {x0}..{x1}");
}

#[test]
fn text_gradient_runs_left_to_right() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(200.0, 60.0, vec![horizontal("g")], vec![text_el(10.0, 40.0, "MMMM", text_style(url("g")))]));
    let (lo, _, hi, _, _) = ink_bounds(&t).unwrap();
    let third = (hi - lo) / 3;
    let sum = |from: u32, to: u32| {
        let (mut rr, mut bb) = (0u32, 0u32);
        for x in from..to {
            for y in 0..t.height {
                let p = t.get_pixel(x, y);
                if p != [0, 0, 0, 255] {
                    rr += p[0] as u32;
                    bb += p[2] as u32;
                }
            }
        }
        (rr, bb)
    };
    let (lr, lb) = sum(lo, lo + third);
    let (rr, rb) = sum(hi - third, hi);
    assert!(lr > lb && rb > rr, "left third redder ({lr} vs {lb}), right third bluer ({rb} vs {rr})");
}

#[test]
fn text_anchor_middle_centres_on_the_anchor() {
    let Some(r) = gpu() else { return };
    let mut style = text_style(Paint::Color(Color::WHITE));
    style.text_anchor = Some(TextAnchor::Middle);
    let t = render_gpu(&r, &scene(200.0, 60.0, vec![], vec![text_el(100.0, 40.0, "HHHH", style)]));
    let (x0, _, x1, _, _) = ink_bounds(&t).unwrap();
    let centre = (x0 + x1) as f32 / 2.0;
    assert!((centre - 100.0).abs() < 4.0, "ink centre {centre} should be near 100");
}

// ── Document order ───────────────────────────────────────────────────────────

fn green_sdf_circle() -> Element {
    Element::Sdf(SdfNode::new(SdfTree::Circle { cx: 50.0, cy: 50.0, r: 30.0 }, Paint::Color(Color::rgb(0, 255, 0))))
}

fn cover(color: Color) -> Element {
    rect(0.0, 0.0, 100.0, 100.0, fill_style(Paint::Color(color)))
}

#[test]
fn document_order_rect_after_sdf_wins_the_overlap() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![], vec![green_sdf_circle(), cover(Color::rgb(255, 0, 0))]));
    assert_eq!(t.get_pixel(50, 50), [255, 0, 0, 255]);
}

#[test]
fn document_order_sdf_after_rect_wins_the_overlap() {
    let Some(r) = gpu() else { return };
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![], vec![cover(Color::rgb(255, 0, 0)), green_sdf_circle()]));
    assert_eq!(t.get_pixel(50, 50), [0, 255, 0, 255]);
}

#[test]
fn document_order_vector_sdf_vector_sandwich() {
    // red cover, green SDF disc, small blue square on top of the disc. A
    // type-batched renderer would draw both rects first and the disc last.
    let Some(r) = gpu() else { return };
    let blue = rect(40.0, 40.0, 20.0, 20.0, fill_style(Paint::Color(Color::rgb(0, 0, 255))));
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![], vec![cover(Color::rgb(255, 0, 0)), green_sdf_circle(), blue]));
    assert_eq!(t.get_pixel(50, 50), [0, 0, 255, 255], "blue square is on top of the disc");
    assert_eq!(t.get_pixel(50, 28), [0, 255, 0, 255], "disc is on top of the red cover");
    assert_eq!(t.get_pixel(5, 5), [255, 0, 0, 255]);
}

#[test]
fn document_order_holds_inside_a_mixed_group() {
    let Some(r) = gpu() else { return };
    let blue = rect(40.0, 40.0, 20.0, 20.0, fill_style(Paint::Color(Color::rgb(0, 0, 255))));
    let group = Element::Group(msx_ast::Group::new(vec![cover(Color::rgb(255, 0, 0)), green_sdf_circle(), blue]));
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![], vec![group]));
    assert_eq!(t.get_pixel(50, 50), [0, 0, 255, 255]);
    assert_eq!(t.get_pixel(50, 28), [0, 255, 0, 255]);
}

#[test]
fn use_resolves_a_target_that_sits_in_another_run() {
    // The circle, the SDF node and the use land in three separate runs. The
    // use must still find its target.
    let Some(r) = gpu() else { return };
    let mut target = circle(20.0, 20.0, 10.0, fill_style(Paint::Color(Color::rgb(255, 0, 0))));
    if let Element::Circle(c) = &mut target {
        c.id = Some("dot".into());
    }
    let sdf = Element::Sdf(SdfNode::new(SdfTree::Circle { cx: 80.0, cy: 80.0, r: 5.0 }, Paint::Color(Color::rgb(0, 255, 0))));
    let u = Use { href: "#dot".into(), x: 50.0, y: 0.0, id: None, transform: None };
    let t = render_gpu(&r, &scene(100.0, 100.0, vec![], vec![target, sdf, Element::Use(u)]));
    assert_eq!(t.get_pixel(20, 20), [255, 0, 0, 255], "the original circle");
    assert_eq!(t.get_pixel(70, 20), [255, 0, 0, 255], "the use, shifted 50 to the right");
}

// ── CPU and GPU agree ────────────────────────────────────────────────────────

fn parity_scene() -> Scene {
    let stops3 = vec![Stop::new(0.0, Color::rgb(67, 233, 123)), Stop::new(0.5, Color::rgb(56, 249, 215)), Stop::new(1.0, Color::rgb(107, 70, 255))];
    let defs = vec![
        horizontal("warm"),
        Def::LinearGradient(LinearGradient::new("cool".into(), 0.0, 0.0, 0.0, 1.0, red_blue())),
        Def::LinearGradient(LinearGradient::new("diag".into(), 0.0, 0.0, 1.0, 1.0, stops3)),
        Def::RadialGradient(RadialGradient::new("spot".into(), 0.5, 0.5, 0.5, 0.3, 0.3, red_blue())),
        conic_def(),
        Def::LinearGradient(LinearGradient::new(
            "fade".into(), 0.0, 0.0, 1.0, 0.0,
            vec![Stop::new(0.0, Color::rgb(255, 255, 255)), Stop::new(1.0, Color::rgba(255, 255, 255, 0))],
        )),
    ];
    let mut half = fill_style(url("warm"));
    half.opacity = Some(0.5);
    scene(
        320.0,
        220.0,
        defs,
        vec![
            rect(10.0, 10.0, 90.0, 50.0, fill_style(url("warm"))),
            rect(110.0, 10.0, 90.0, 50.0, fill_style(url("cool"))),
            rect(210.0, 10.0, 90.0, 50.0, fill_style(url("diag"))),
            rect(10.0, 70.0, 90.0, 30.0, half),
            rect(110.0, 70.0, 190.0, 30.0, fill_style(url("fade"))),
            circle(60.0, 160.0, 45.0, fill_style(url("spot"))),
            circle(170.0, 160.0, 45.0, fill_style(url("c"))),
            rect(235.0, 115.0, 70.0, 90.0, stroke_style(url("diag"), 8.0)),
        ],
    )
}

/// Sample points at least `margin` px inside each shape, away from the
/// conic seam and centre where the two renderers legitimately differ by a
/// pixel.
fn parity_samples() -> Vec<(u32, u32)> {
    let mut pts = Vec::new();
    let margin = 5.0f32;
    let rects = [(10.0, 10.0, 90.0, 50.0), (110.0, 10.0, 90.0, 50.0), (210.0, 10.0, 90.0, 50.0), (10.0, 70.0, 90.0, 30.0), (110.0, 70.0, 190.0, 30.0)];
    for (x, y, w, h) in rects {
        let mut py = y + margin;
        while py < y + h - margin {
            let mut px = x + margin;
            while px < x + w - margin {
                pts.push((px as u32, py as u32));
                px += 6.0;
            }
            py += 6.0;
        }
    }
    for (cx, cy, r, conic) in [(60.0f32, 160.0f32, 45.0f32, false), (170.0, 160.0, 45.0, true)] {
        let mut py = cy - r;
        while py <= cy + r {
            let mut px = cx - r;
            while px <= cx + r {
                let (dx, dy) = (px - cx, py - cy);
                let dist = (dx * dx + dy * dy).sqrt();
                let near_seam = conic && dy < 0.0 && dx.abs() < 3.0;
                let near_centre = conic && dist < 6.0;
                if dist < r - margin && !near_seam && !near_centre {
                    pts.push((px as u32, py as u32));
                }
                px += 5.0;
            }
            py += 5.0;
        }
    }
    pts
}

#[test]
fn gradient_interiors_match_the_cpu_renderer() {
    let Some(gpu_r) = gpu() else { return };
    let s = parity_scene();
    let mut cpu_t = RenderTarget::new(320, 220);
    msx_render_cpu::CpuRenderer::new().render(&s, &mut cpu_t);
    let gpu_t = render_gpu(&gpu_r, &s);

    let samples = parity_samples();
    assert!(samples.len() > 200, "expected a decent sample, got {}", samples.len());
    let mut worst = 0i32;
    let mut at = (0, 0);
    for &(x, y) in &samples {
        let (c, g) = (cpu_t.get_pixel(x, y), gpu_t.get_pixel(x, y));
        let d = (0..3).map(|i| (c[i] as i32 - g[i] as i32).abs()).max().unwrap();
        if d > worst {
            worst = d;
            at = (x, y);
        }
    }
    eprintln!("max CPU/GPU channel difference over {} samples: {worst} at {at:?}", samples.len());
    assert!(worst <= 6, "max channel difference {worst} at {at:?}: cpu {:?} gpu {:?}", cpu_t.get_pixel(at.0, at.1), gpu_t.get_pixel(at.0, at.1));
}

#[test]
fn text_ink_matches_the_cpu_renderer() {
    let Some(gpu_r) = gpu() else { return };
    let s = scene(260.0, 70.0, vec![], vec![text_el(10.0, 50.0, "MSX Text 42", Style { font_size: Some(40.0), ..fill_style(Paint::Color(Color::WHITE)) })]);
    let mut cpu_t = RenderTarget::new(260, 70);
    msx_render_cpu::CpuRenderer::new().render(&s, &mut cpu_t);
    let gpu_t = render_gpu(&gpu_r, &s);

    // The CPU antialiases and the GPU does not, so edge pixels differ.
    // Compare where the ink is and how much area it covers instead.
    let (cx0, cy0, cx1, cy1, _) = ink_bounds(&cpu_t).unwrap();
    let (gx0, gy0, gx1, gy1, _) = ink_bounds(&gpu_t).unwrap();
    assert!((cx0 as i32 - gx0 as i32).abs() <= 2 && (cx1 as i32 - gx1 as i32).abs() <= 2, "x extent cpu {cx0}..{cx1} gpu {gx0}..{gx1}");
    assert!((cy0 as i32 - gy0 as i32).abs() <= 2 && (cy1 as i32 - gy1 as i32).abs() <= 2, "y extent cpu {cy0}..{cy1} gpu {gy0}..{gy1}");
    let (ca, ga) = (ink_area(&cpu_t), ink_area(&gpu_t));
    let ratio = ga / ca;
    eprintln!("covered area cpu {ca:.0} gpu {ga:.0} ratio {ratio:.3}");
    assert!((0.95..=1.05).contains(&ratio), "covered area cpu {ca:.0} gpu {ga:.0}, ratio {ratio}");
}
