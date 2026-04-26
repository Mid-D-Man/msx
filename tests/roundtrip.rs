// tests/roundtrip.rs
//! Integration tests — every test must produce identical SVG output
//! after a source → binary → decode → render roundtrip.

use msx::{compile, decode, parse_scene, render};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normalise_svg(svg: &str) -> String {
    svg.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn check_roundtrip(label: &str, source: &str) {
    let scene_a = parse_scene(source)
        .unwrap_or_else(|e| panic!("[{}] parse failed: {}", label, e));

    let svg_a = render(&scene_a);

    let binary = compile(&scene_a, true)
        .unwrap_or_else(|e| panic!("[{}] compile failed: {}", label, e));

    let scene_b = decode(&binary)
        .unwrap_or_else(|e| panic!("[{}] decode failed: {}", label, e));

    let svg_b = render(&scene_b);

    let na = normalise_svg(&svg_a);
    let nb = normalise_svg(&svg_b);

    if na != nb {
        let chars_a: Vec<char> = na.chars().collect();
        let chars_b: Vec<char> = nb.chars().collect();
        for (i, (a, b)) in chars_a.iter().zip(chars_b.iter()).enumerate() {
            if a != b {
                let ctx_a = &na[i.saturating_sub(30)..((i + 30).min(na.len()))];
                let ctx_b = &nb[i.saturating_sub(30)..((i + 30).min(nb.len()))];
                panic!("[{}] SVG mismatch at char {}: {:?} vs {:?}", label, i, ctx_a, ctx_b);
            }
        }
        if na.len() != nb.len() {
            panic!("[{}] SVG length mismatch: {} vs {}", label, na.len(), nb.len());
        }
    }

    let ratio = binary.len() as f64 / svg_a.len() as f64 * 100.0;
    println!("[{}] PASS — {} elements, {}B binary, {}B svg ({:.1}%)",
        label, scene_a.element_count(), binary.len(), svg_a.len(), ratio);
}

// ── Basic geometry ────────────────────────────────────────────────────────────

#[test]
fn roundtrip_circle() {
    check_roundtrip("circle", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 200, height = 200, background = "#ffffff" }
  elements::
    { type = "circle", cx = 100, cy = 100, r = 50,
      style = { fill = "#ff0000", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"##);
}

#[test]
fn roundtrip_rect_rounded() {
    check_roundtrip("rect_rounded", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 300, background = "#1a1a2e" }
  elements::
    { type = "rect", x = 20, y = 20, width = 200, height = 120, rx = 12,
      style = { fill = "#0f3460", stroke = "#4a9eff", stroke_width = 2.0, opacity = 1.0 } }
)
"##);
}

#[test]
fn roundtrip_ellipse() {
    check_roundtrip("ellipse", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 500, height = 300, background = "#ffffff" }
  elements::
    { type = "ellipse", cx = 250, cy = 150, rx = 180, ry = 80,
      style = { fill = "#a78bfa", stroke = "#7c3aed", stroke_width = 3.0, opacity = 0.9 } }
)
"##);
}

#[test]
fn roundtrip_line() {
    check_roundtrip("line", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 400, background = "#000000" }
  elements::
    { type = "line", x1 = 10, y1 = 10, x2 = 390, y2 = 390,
      style = { fill = "none", stroke = "#f5a623", stroke_width = 4.0, opacity = 1.0 } }
)
"##);
}

#[test]
fn roundtrip_path_triangle() {
    check_roundtrip("path_triangle", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 500, height = 500, background = "#0d1117" }
  elements::
    { type = "path", d = "M 50 450 L 250 50 L 450 450 Z",
      style = { fill = "#3498db", stroke = "#2980b9", stroke_width = 3.0, opacity = 0.9 } }
)
"##);
}

#[test]
fn roundtrip_path_cubic_bezier() {
    check_roundtrip("path_cubic", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 500, height = 500, background = "#0d1117" }
  elements::
    { type = "path",
      d = "M 250 120 C 340 60 440 140 420 240 C 400 340 300 420 210 390 C 120 360 80 260 100 180 C 120 100 160 180 250 120 Z",
      style = { fill = "none", stroke = "#e74c3c", stroke_width = 4.0, opacity = 1.0 } }
)
"##);
}

#[test]
fn roundtrip_path_arc() {
    check_roundtrip("path_arc", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 500, height = 500, background = "#0d1117" }
  elements::
    { type = "path", d = "M 120 380 A 130 130 0 0 1 380 380",
      style = { fill = "none", stroke = "#f5a623", stroke_width = 5.0, opacity = 1.0 } }
)
"##);
}

#[test]
fn roundtrip_text() {
    check_roundtrip("text", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 100, background = "#ffffff" }
  elements::
    { type = "text", x = 200, y = 60, content = "Hello MSX",
      style = { fill = "#000000", font_size = 24, text_anchor = "middle",
                stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"##);
}

// ── Groups and nesting ────────────────────────────────────────────────────────

#[test]
fn roundtrip_group_with_transform() {
    check_roundtrip("group_transform", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 400, background = "#f0f0f0" }
  elements::
    { type = "group",
      transform = { type = "translate", x = 100, y = 100 },
      elements = [
        { type = "rect", x = 0, y = 0, width = 80, height = 80,
          style = { fill = "#007bff", stroke = "none", stroke_width = 0, opacity = 1.0 } }
        { type = "circle", cx = 40, cy = 40, r = 20,
          style = { fill = "#ffffff", stroke = "none", stroke_width = 0, opacity = 0.8 } }
      ] }
)
"##);
}

#[test]
fn roundtrip_nested_groups() {
    check_roundtrip("nested_groups", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 400, background = "#ffffff" }
  elements::
    { type = "group",
      elements = [
        { type = "group",
          transform = { type = "rotate", angle = 45 },
          elements = [
            { type = "rect", x = -25, y = -25, width = 50, height = 50,
              style = { fill = "#e94560", stroke = "none", stroke_width = 0, opacity = 1.0 } }
          ] }
      ] }
)
"##);
}

// ── QuickFuncs evaluated before roundtrip ────────────────────────────────────

#[test]
fn roundtrip_quickfunc_badge() {
    check_roundtrip("quickfunc_badge", r##"
@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~badge<object>(x, y, label, color) {
    return {
      type = "group"
      elements = [
        { type = "rect", x = x, y = y, width = 90, height = 30, rx = 15,
          style = { fill = color, stroke = "none", stroke_width = 0, opacity = 1.0 } }
        { type = "text", x = x + 45, y = y + 20, content = label,
          style = { fill = "#ffffff", font_size = 12, text_anchor = "middle",
                    font_weight = "bold", stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }
)
@DATA(
  scene: { width = 500, height = 100, background = "#f4f5f7" }
  elements::
    badge(20,  30, "primary", "#007bff")
    badge(130, 30, "success", "#28a745")
    badge(240, 30, "danger",  "#dc3545")
)
"##);
}

#[test]
fn roundtrip_parametric_circles() {
    check_roundtrip("parametric_circles", r##"
@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~dot<object>(cx, cy, r, color) {
    return {
      type = "circle"
      cx = cx  cy = cy  r = r
      style = { fill = color, stroke = "none", stroke_width = 0, opacity = 0.8 }
    }
  }
)
@DATA(
  scene: { width = 600, height = 200, background = "#1a1a2e" }
  elements::
    dot(60,  100, 40, "#e94560")
    dot(160, 100, 35, "#533483")
    dot(260, 100, 45, "#0f3460")
    dot(360, 100, 30, "#4a9eff")
    dot(460, 100, 50, "#22c55e")
    dot(540, 100, 25, "#f5a623")
)
"##);
}

// ── Gradients ─────────────────────────────────────────────────────────────────

#[test]
fn roundtrip_linear_gradient() {
    check_roundtrip("linear_gradient", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 600, height = 200, background = "#ffffff" }
  defs::
    { type = "linear_gradient", id = "sunset",
      x1 = 0.0, y1 = 0.0, x2 = 1.0, y2 = 0.0,
      stops = [
        { offset = 0.0, color = "#f7971e", opacity = 1.0 }
        { offset = 1.0, color = "#ffd200", opacity = 1.0 }
      ] }
  elements::
    { type = "rect", x = 50, y = 50, width = 500, height = 100,
      style = { fill = "url(#sunset)", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"##);
}

#[test]
fn roundtrip_radial_gradient() {
    check_roundtrip("radial_gradient", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 400, background = "#000000" }
  defs::
    { type = "radial_gradient", id = "glow",
      cx = 0.5, cy = 0.5, r = 0.5,
      stops = [
        { offset = 0.0, color = "#4facfe", opacity = 1.0 }
        { offset = 1.0, color = "#00f2fe", opacity = 0.0 }
      ] }
  elements::
    { type = "circle", cx = 200, cy = 200, r = 180,
      style = { fill = "url(#glow)", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"##);
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[test]
fn roundtrip_many_elements_compression_ratio() {
    let colors = ["#e94560", "#533483", "#0f3460", "#4a9eff", "#22c55e",
                  "#f5a623", "#a78bfa", "#ef4444", "#3b82f6", "#10b981"];
    let mut src = String::from(
r#"@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~dot<object>(cx, cy, r, color) {
    return { type = "circle", cx = cx, cy = cy, r = r,
             style = { fill = color, stroke = "none", stroke_width = 0, opacity = 0.9 } }
  }
)
@DATA(
  scene = { width = 1000, height = 1000, background = #ffffff }
  elements::
"#);
    for i in 0..200usize {
        let x = (i % 20) * 50 + 25;
        let y = (i / 20) * 50 + 25;
        let r = 15 + (i % 5) * 3;
        // colors injected as bare hex (DixScript HexColor type)
        let c = colors[i % colors.len()];
        src.push_str(&format!("    dot({}, {}, {}, {})\n", x, y, r, c));
    }
    src.push(')');

    let scene_a = parse_scene(&src).expect("parse 200 circles");
    let svg_a   = render(&scene_a);
    let binary  = compile(&scene_a, true).expect("compile 200 circles");
    let scene_b = decode(&binary).expect("decode 200 circles");
    let svg_b   = render(&scene_b);

    assert_eq!(normalise_svg(&svg_a), normalise_svg(&svg_b));
    assert!(binary.len() < svg_a.len(),
        "compressed binary ({} B) should be smaller than SVG ({} B)",
        binary.len(), svg_a.len());
}

#[test]
fn roundtrip_opacity_zero_element() {
    check_roundtrip("opacity_zero", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 100, height = 100, background = "#000000" }
  elements::
    { type = "rect", x = 0, y = 0, width = 100, height = 100,
      style = { fill = "#ffffff", stroke = "none", stroke_width = 0, opacity = 0.0 } }
)
"##);
}

#[test]
fn roundtrip_stroke_dasharray() {
    check_roundtrip("stroke_dasharray", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 200, background = "#ffffff" }
  elements::
    { type = "line", x1 = 20, y1 = 100, x2 = 380, y2 = 100,
      style = { fill = "none", stroke = "#000000", stroke_width = 3.0,
                stroke_dasharray = [10, 5, 3, 5], stroke_dashoffset = 2.0, opacity = 1.0 } }
)
"##);
}

#[test]
fn roundtrip_use_element() {
    check_roundtrip("use_element", r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 200, background = "#f0f0f0" }
  defs::
    { type = "linear_gradient", id = "base",
      x1 = 0.0, y1 = 0.0, x2 = 1.0, y2 = 0.0,
      stops = [
        { offset = 0.0, color = "#007bff", opacity = 1.0 }
        { offset = 1.0, color = "#00c9ff", opacity = 1.0 }
      ] }
  elements::
    { type = "rect", id = "tile", x = 0, y = 0, width = 80, height = 80,
      style = { fill = "url(#base)", stroke = "none", stroke_width = 0, opacity = 1.0 } }
    { type = "use", href = "#tile", x = 100, y = 60 }
    { type = "use", href = "#tile", x = 210, y = 60 }
)
"##);
}
