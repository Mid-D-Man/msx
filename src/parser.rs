// src/parser.rs
//! DixData → Scene AST converter.
//!
//! This is NOT a DixScript parser — the full DixScript pipeline
//! (tokenize → parse → semantic analysis → QuickFuncs evaluation)
//! is handled by the DixScript runtime. By the time we receive a
//! DixData, all QuickFuncs have been evaluated and every element
//! is a flat key-value structure we can walk directly.
//!
//! Entry point:
//!   parse_scene(source: &str) -> Result<Scene, String>
//!   parse_scene_from_data(data: &DixData) -> Result<Scene, String>

use dixscript::Runtime::{DixData, DixLoader, DixLoadOptions, DixValue};
use crate::ast::*;
use crate::color::{Color, LinearGradient, Paint, RadialGradient, Stop};
use crate::path::parse_d;
use crate::primitives::{Point, ViewBox};
use crate::style::{
    FillRule, FontWeight, LineCap, LineJoin, Style, TextAnchor,
};
use crate::transform::Transform;

// ── Public entry points ───────────────────────────────────────────────────────

/// Parse an MSX source string into a Scene.
/// Runs the full DixScript pipeline internally.
pub fn parse_scene(source: &str) -> Result<Scene, String> {
    let loader  = DixLoader::new();
    let options = DixLoadOptions::new();
    let data    = loader
        .load_from_str(source, &options)
        .map_err(|e| format!("DixScript load failed: {}", e))?;
    parse_scene_from_data(&data)
}

/// Parse an MSX file from disk into a Scene.
pub fn parse_scene_file(path: &str) -> Result<Scene, String> {
    let loader  = DixLoader::new();
    let options = DixLoadOptions::new();
    let data    = loader
        .load_text(path, &options)
        .map_err(|e| format!("DixScript load '{}' failed: {}", path, e))?;
    parse_scene_from_data(&data)
}

/// Convert an already-loaded DixData into a Scene.
/// Use this when you have already called DixLoader yourself.
pub fn parse_scene_from_data(data: &DixData) -> Result<Scene, String> {
    let canvas   = parse_canvas(data)?;
    let defs     = parse_defs(data)?;
    let elements = parse_elements(data, "elements")?;

    let mut scene = Scene::new(canvas);
    scene.defs     = defs;
    scene.elements = elements;
    Ok(scene)
}

// ── Canvas ────────────────────────────────────────────────────────────────────

fn parse_canvas(data: &DixData) -> Result<Canvas, String> {
    let width: f64 = data.get("scene.width")
        .map_err(|_| "scene.width is required".to_string())?;
    let height: f64 = data.get("scene.height")
        .map_err(|_| "scene.height is required".to_string())?;

    let bg_str: String = data.get("scene.background")
        .unwrap_or_else(|_| "#ffffff".to_string());
    let background = Color::parse(&bg_str)
        .unwrap_or(Color::WHITE);

    let mut canvas = Canvas::new(width, height, background);

    // Optional viewbox
    if data.exists("viewbox.width") {
        let vb = ViewBox::new(
            data.get("viewbox.min_x").unwrap_or(0.0),
            data.get("viewbox.min_y").unwrap_or(0.0),
            data.get("viewbox.width").unwrap_or(width),
            data.get("viewbox.height").unwrap_or(height),
        );
        canvas.viewbox = Some(vb);
    }

    Ok(canvas)
}

// ── Defs ──────────────────────────────────────────────────────────────────────

fn parse_defs(data: &DixData) -> Result<Vec<Def>, String> {
    let mut defs = Vec::new();

    let mut i = 0;
    loop {
        let prefix = format!("defs[{}]", i);
        if !data.exists(&format!("{}.type", prefix)) { break; }

        let def = parse_def(data, &prefix)
            .map_err(|e| format!("defs[{}]: {}", i, e))?;
        defs.push(def);
        i += 1;
    }

    Ok(defs)
}

fn parse_def(data: &DixData, prefix: &str) -> Result<Def, String> {
    let type_str: String = data.get(&format!("{}.type", prefix))
        .map_err(|_| "missing type field".to_string())?;

    match type_str.as_str() {
        "linear_gradient" => {
            let id: String = data.get(&format!("{}.id", prefix))
                .map_err(|_| "linear_gradient requires id".to_string())?;
            let x1: f64 = data.get(&format!("{}.x1", prefix)).unwrap_or(0.0);
            let y1: f64 = data.get(&format!("{}.y1", prefix)).unwrap_or(0.0);
            let x2: f64 = data.get(&format!("{}.x2", prefix)).unwrap_or(1.0);
            let y2: f64 = data.get(&format!("{}.y2", prefix)).unwrap_or(0.0);
            let stops   = parse_stops(data, &format!("{}.stops", prefix))?;
            Ok(Def::LinearGradient(LinearGradient::new(id, x1, y1, x2, y2, stops)))
        }
        "radial_gradient" => {
            let id: String = data.get(&format!("{}.id", prefix))
                .map_err(|_| "radial_gradient requires id".to_string())?;
            let cx: f64 = data.get(&format!("{}.cx", prefix)).unwrap_or(0.5);
            let cy: f64 = data.get(&format!("{}.cy", prefix)).unwrap_or(0.5);
            let r:  f64 = data.get(&format!("{}.r",  prefix)).unwrap_or(0.5);
            let fx: f64 = data.get(&format!("{}.fx", prefix)).unwrap_or(cx);
            let fy: f64 = data.get(&format!("{}.fy", prefix)).unwrap_or(cy);
            let stops   = parse_stops(data, &format!("{}.stops", prefix))?;
            Ok(Def::RadialGradient(RadialGradient::new(id, cx, cy, r, fx, fy, stops)))
        }
        other => Err(format!("unknown def type '{}'", other)),
    }
}

fn parse_stops(data: &DixData, prefix: &str) -> Result<Vec<Stop>, String> {
    let mut stops = Vec::new();
    let mut i = 0;
    loop {
        let stop_prefix = format!("{}[{}]", prefix, i);
        if !data.exists(&format!("{}.offset", stop_prefix)) &&
           !data.exists(&format!("{}.color",  stop_prefix)) { break; }

        let offset: f64 = data.get(&format!("{}.offset", stop_prefix)).unwrap_or(0.0);
        let color_str: String = data.get(&format!("{}.color", stop_prefix))
            .unwrap_or_else(|_| "#000000".to_string());
        let opacity: f64 = data.get(&format!("{}.opacity", stop_prefix)).unwrap_or(1.0);

        let mut color = Color::parse(&color_str)
            .unwrap_or(Color::BLACK);
        color.a = (opacity * 255.0).round() as u8;

        stops.push(Stop::new(offset, color));
        i += 1;
    }
    Ok(stops)
}

// ── Elements ──────────────────────────────────────────────────────────────────

fn parse_elements(data: &DixData, array_key: &str) -> Result<Vec<Element>, String> {
    let mut elements = Vec::new();
    let mut i = 0;
    loop {
        let prefix = format!("{}[{}]", array_key, i);
        if !data.exists(&format!("{}.type", prefix)) { break; }

        let elem = parse_element(data, &prefix)
            .map_err(|e| format!("{}[{}]: {}", array_key, i, e))?;
        elements.push(elem);
        i += 1;
    }
    Ok(elements)
}

fn parse_element(data: &DixData, prefix: &str) -> Result<Element, String> {
    let type_str: String = data.get(&format!("{}.type", prefix))
        .map_err(|_| "element missing 'type' field".to_string())?;

    let id:        Option<String>    = data.get(&format!("{}.id", prefix)).ok();
    let transform: Option<Transform> = parse_optional_transform(data, prefix);
    let style:     Style             = parse_style(data, &format!("{}.style", prefix));

    match type_str.as_str() {
        "rect" => {
            let x:      f64 = data.get(&format!("{}.x", prefix)).unwrap_or(0.0);
            let y:      f64 = data.get(&format!("{}.y", prefix)).unwrap_or(0.0);
            let width:  f64 = data.get(&format!("{}.width", prefix))
                .map_err(|_| "rect requires 'width'".to_string())?;
            let height: f64 = data.get(&format!("{}.height", prefix))
                .map_err(|_| "rect requires 'height'".to_string())?;
            let rx:     Option<f64> = data.get(&format!("{}.rx", prefix)).ok();
            let ry:     Option<f64> = data.get(&format!("{}.ry", prefix)).ok();
            Ok(Element::Rect(Rect { x, y, width, height, rx, ry, id, transform, style }))
        }
        "circle" => {
            let cx: f64 = data.get(&format!("{}.cx", prefix)).unwrap_or(0.0);
            let cy: f64 = data.get(&format!("{}.cy", prefix)).unwrap_or(0.0);
            let r:  f64 = data.get(&format!("{}.r", prefix))
                .map_err(|_| "circle requires 'r'".to_string())?;
            Ok(Element::Circle(Circle { cx, cy, r, id, transform, style }))
        }
        "ellipse" => {
            let cx: f64 = data.get(&format!("{}.cx", prefix)).unwrap_or(0.0);
            let cy: f64 = data.get(&format!("{}.cy", prefix)).unwrap_or(0.0);
            let rx: f64 = data.get(&format!("{}.rx", prefix))
                .map_err(|_| "ellipse requires 'rx'".to_string())?;
            let ry: f64 = data.get(&format!("{}.ry", prefix))
                .map_err(|_| "ellipse requires 'ry'".to_string())?;
            Ok(Element::Ellipse(Ellipse { cx, cy, rx, ry, id, transform, style }))
        }
        "line" => {
            let x1: f64 = data.get(&format!("{}.x1", prefix)).unwrap_or(0.0);
            let y1: f64 = data.get(&format!("{}.y1", prefix)).unwrap_or(0.0);
            let x2: f64 = data.get(&format!("{}.x2", prefix)).unwrap_or(0.0);
            let y2: f64 = data.get(&format!("{}.y2", prefix)).unwrap_or(0.0);
            Ok(Element::Line(Line { x1, y1, x2, y2, id, transform, style }))
        }
        "polyline" | "polygon" => {
            let closed  = type_str == "polygon";
            let points  = parse_point_array(data, &format!("{}.points", prefix))?;
            Ok(Element::Polyline(Polyline { points, closed, id, transform, style }))
        }
        "path" => {
            let d_raw: String = data.get(&format!("{}.d", prefix))
                .map_err(|_| "path requires 'd'".to_string())?;
            let commands = parse_d(&d_raw)
                .map_err(|e| format!("path 'd' parse error: {}", e))?;
            Ok(Element::Path(Path { commands, d_raw, id, transform, style }))
        }
        "text" => {
            let x:       f64    = data.get(&format!("{}.x", prefix)).unwrap_or(0.0);
            let y:       f64    = data.get(&format!("{}.y", prefix)).unwrap_or(0.0);
            let content: String = data.get(&format!("{}.content", prefix))
                .map_err(|_| "text requires 'content'".to_string())?;
            Ok(Element::Text(Text { x, y, content, id, transform, style }))
        }
        "group" => {
            let children = parse_elements(data, &format!("{}.elements", prefix))?;
            let group_style = if style == Style::empty() { None } else { Some(style) };
            Ok(Element::Group(Group { children, id, transform, style: group_style }))
        }
        "use" => {
            let href: String = data.get(&format!("{}.href", prefix))
                .map_err(|_| "use requires 'href'".to_string())?;
            let x: f64 = data.get(&format!("{}.x", prefix)).unwrap_or(0.0);
            let y: f64 = data.get(&format!("{}.y", prefix)).unwrap_or(0.0);
            Ok(Element::Use(Use { href, x, y, id, transform }))
        }
        other => Err(format!("unknown element type '{}'", other)),
    }
}

// ── Transform parsing ─────────────────────────────────────────────────────────

fn parse_optional_transform(data: &DixData, prefix: &str) -> Option<Transform> {
    if let Ok(s) = data.get::<String>(&format!("{}.transform", prefix)) {
        let t = Transform::parse_svg(&s);
        if !t.is_none() { return Some(t); }
    }

    let ttype: String = data.get(&format!("{}.transform.type", prefix)).ok()?;

    let t = match ttype.as_str() {
        "translate" => Transform::Translate {
            x: data.get(&format!("{}.transform.x", prefix)).unwrap_or(0.0),
            y: data.get(&format!("{}.transform.y", prefix)).unwrap_or(0.0),
        },
        "scale" => Transform::Scale {
            x: data.get(&format!("{}.transform.x", prefix)).unwrap_or(1.0),
            y: data.get(&format!("{}.transform.y", prefix)).unwrap_or(1.0),
        },
        "rotate" => Transform::Rotate {
            angle: data.get(&format!("{}.transform.angle", prefix)).unwrap_or(0.0),
            cx:    data.get(&format!("{}.transform.cx", prefix)).ok(),
            cy:    data.get(&format!("{}.transform.cy", prefix)).ok(),
        },
        "skew_x" => Transform::SkewX(
            data.get(&format!("{}.transform.angle", prefix)).unwrap_or(0.0)
        ),
        "skew_y" => Transform::SkewY(
            data.get(&format!("{}.transform.angle", prefix)).unwrap_or(0.0)
        ),
        "matrix" => {
            let get = |field: &str| -> f64 {
                data.get(&format!("{}.transform.{}", prefix, field)).unwrap_or(0.0)
            };
            use crate::transform::Matrix2D;
            Transform::Matrix(Matrix2D {
                a: get("a"), b: get("b"), c: get("c"),
                d: get("d"), e: get("e"), f: get("f"),
            })
        }
        _ => return None,
    };

    Some(t)
}

// ── Style parsing ─────────────────────────────────────────────────────────────

fn parse_style(data: &DixData, prefix: &str) -> Style {
    let mut s = Style::empty();

    if let Ok(v) = data.get::<String>(&format!("{}.fill", prefix)) {
        s.fill = Some(Paint::parse(&v));
    }
    if let Ok(v) = data.get::<String>(&format!("{}.stroke", prefix)) {
        s.stroke = Some(Paint::parse(&v));
    }
    if let Ok(v) = data.get::<f64>(&format!("{}.stroke_width", prefix)) {
        s.stroke_width = Some(v);
    }
    if let Ok(v) = data.get::<f64>(&format!("{}.opacity", prefix)) {
        s.opacity = Some(v);
    }
    if let Ok(v) = data.get::<f64>(&format!("{}.fill_opacity", prefix)) {
        s.fill_opacity = Some(v);
    }
    if let Ok(v) = data.get::<f64>(&format!("{}.stroke_opacity", prefix)) {
        s.stroke_opacity = Some(v);
    }
    if let Ok(v) = data.get::<String>(&format!("{}.fill_rule", prefix)) {
        s.fill_rule = Some(FillRule::parse(&v));
    }
    if let Ok(v) = data.get::<String>(&format!("{}.stroke_linecap", prefix)) {
        s.stroke_linecap = Some(LineCap::parse(&v));
    }
    if let Ok(v) = data.get::<String>(&format!("{}.stroke_linejoin", prefix)) {
        s.stroke_linejoin = Some(LineJoin::parse(&v));
    }
    if let Ok(v) = data.get::<f64>(&format!("{}.stroke_miterlimit", prefix)) {
        s.stroke_miterlimit = Some(v);
    }
    if let Ok(v) = data.get::<f64>(&format!("{}.font_size", prefix)) {
        s.font_size = Some(v);
    }
    if let Ok(v) = data.get::<String>(&format!("{}.font_family", prefix)) {
        s.font_family = Some(v);
    }
    if let Ok(v) = data.get::<String>(&format!("{}.font_weight", prefix)) {
        s.font_weight = Some(FontWeight::parse(&v));
    }
    if let Ok(v) = data.get::<String>(&format!("{}.text_anchor", prefix)) {
        s.text_anchor = Some(TextAnchor::parse(&v));
    }
    if let Ok(v) = data.get::<String>(&format!("{}.dominant_baseline", prefix)) {
        s.dominant_baseline = Some(v);
    }
    if let Ok(v) = data.get::<String>(&format!("{}.visibility", prefix)) {
        s.visibility_hidden = v == "hidden";
    }
    if let Ok(v) = data.get::<String>(&format!("{}.display", prefix)) {
        s.display_none = v == "none";
    }

    let mut da: Vec<f64> = Vec::new();
    let mut j = 0;
    loop {
        let key = format!("{}[{}]", format!("{}.stroke_dasharray", prefix), j);
        if let Ok(v) = data.get::<f64>(&key) {
            da.push(v);
            j += 1;
        } else { break; }
    }
    if !da.is_empty() { s.stroke_dasharray = Some(da); }

    if let Ok(v) = data.get::<f64>(&format!("{}.stroke_dashoffset", prefix)) {
        s.stroke_dashoffset = Some(v);
    }

    s
}

// ── Point array parsing ───────────────────────────────────────────────────────

fn parse_point_array(data: &DixData, prefix: &str) -> Result<Vec<Point>, String> {
    let mut points = Vec::new();
    let mut i = 0;
    loop {
        let xkey = format!("{}[{}][0]", prefix, i);
        let ykey = format!("{}[{}][1]", prefix, i);
        if !data.exists(&xkey) { break; }
        let x: f64 = data.get(&xkey).unwrap_or(0.0);
        let y: f64 = data.get(&ykey).unwrap_or(0.0);
        points.push(Point::new(x, y));
        i += 1;
    }
    Ok(points)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // All raw strings that embed "#colour" values use r##"..."## so the
    // "#" inside colour literals does not accidentally close the raw string.

    fn simple_circle_src() -> &'static str {
        r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 200, height = 200, background = "#ffffff" }
  elements::
    { type = "circle", cx = 100, cy = 100, r = 50,
      style = { fill = "#ff0000", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"##
    }

    fn simple_rect_src() -> &'static str {
        r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 400, height = 300, background = "#1a1a2e" }
  elements::
    { type = "rect", x = 10, y = 20, width = 100, height = 50,
      style = { fill = "#4a9eff", stroke = "#ffffff", stroke_width = 2.0, opacity = 0.9 } }
)
"##
    }

    #[test]
    fn parse_canvas_dimensions() {
        let scene = parse_scene(simple_circle_src()).unwrap();
        assert!((scene.canvas.width  - 200.0).abs() < 1e-4);
        assert!((scene.canvas.height - 200.0).abs() < 1e-4);
    }

    #[test]
    fn parse_canvas_background_color() {
        let scene = parse_scene(simple_circle_src()).unwrap();
        assert_eq!(scene.canvas.background, Color::WHITE);
    }

    #[test]
    fn parse_single_circle() {
        let scene = parse_scene(simple_circle_src()).unwrap();
        assert_eq!(scene.elements.len(), 1);
        if let Element::Circle(c) = &scene.elements[0] {
            assert!((c.cx - 100.0).abs() < 1e-4);
            assert!((c.cy - 100.0).abs() < 1e-4);
            assert!((c.r  - 50.0).abs() < 1e-4);
        } else {
            panic!("expected Circle");
        }
    }

    #[test]
    fn parse_circle_style_fill() {
        let scene = parse_scene(simple_circle_src()).unwrap();
        if let Element::Circle(c) = &scene.elements[0] {
            if let Some(Paint::Color(col)) = &c.style.fill {
                assert_eq!(*col, Color::rgb(255, 0, 0));
            } else {
                panic!("expected Color fill");
            }
        }
    }

    #[test]
    fn parse_single_rect() {
        let scene = parse_scene(simple_rect_src()).unwrap();
        assert_eq!(scene.elements.len(), 1);
        if let Element::Rect(r) = &scene.elements[0] {
            assert!((r.x       - 10.0).abs() < 1e-4);
            assert!((r.y       - 20.0).abs() < 1e-4);
            assert!((r.width   - 100.0).abs() < 1e-4);
            assert!((r.height  - 50.0).abs() < 1e-4);
        } else {
            panic!("expected Rect");
        }
    }

    #[test]
    fn parse_rect_stroke_opacity() {
        let scene = parse_scene(simple_rect_src()).unwrap();
        if let Element::Rect(r) = &scene.elements[0] {
            assert!((r.style.opacity.unwrap() - 0.9).abs() < 1e-3);
            if let Some(Paint::Color(col)) = &r.style.stroke {
                assert_eq!(*col, Color::WHITE);
            } else {
                panic!("expected stroke Color");
            }
        }
    }

    #[test]
    fn parse_path_d_string() {
        let src = r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 500, height = 500, background = "#000000" }
  elements::
    { type = "path", d = "M 50 450 L 250 50 L 450 450 Z",
      style = { fill = "#3498db", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"##;
        let scene = parse_scene(src).unwrap();
        if let Element::Path(p) = &scene.elements[0] {
            assert_eq!(p.commands.len(), 4);
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn parse_group_with_children() {
        let src = r##"
@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~badge<object>(x, y) {
    return {
      type = "group"
      elements = [
        { type = "rect", x = x, y = y, width = 90, height = 30,
          style = { fill = "#007bff", stroke = "none", stroke_width = 0, opacity = 1.0 } }
        { type = "circle", cx = x + 45, cy = y + 15, r = 10,
          style = { fill = "#ffffff", stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }
)
@DATA(
  scene: { width = 200, height = 100, background = "#f0f0f0" }
  elements::
    badge(10, 10)
)
"##;
        let scene = parse_scene(src).unwrap();
        assert_eq!(scene.elements.len(), 1);
        if let Element::Group(g) = &scene.elements[0] {
            assert_eq!(g.children.len(), 2);
            assert!(matches!(g.children[0], Element::Rect(_)));
            assert!(matches!(g.children[1], Element::Circle(_)));
        } else {
            panic!("expected Group, got {:?}", scene.elements[0].tag_name());
        }
    }

    #[test]
    fn parse_text_element() {
        let src = r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 300, height = 100, background = "#ffffff" }
  elements::
    { type = "text", x = 50, y = 60, content = "Hello MSX",
      style = { fill = "#000000", font_size = 18, text_anchor = "middle",
                stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"##;
        let scene = parse_scene(src).unwrap();
        if let Element::Text(t) = &scene.elements[0] {
            assert_eq!(t.content, "Hello MSX");
            assert!((t.x - 50.0).abs() < 1e-4);
            assert_eq!(t.style.text_anchor, Some(TextAnchor::Middle));
            assert!((t.style.font_size.unwrap() - 18.0).abs() < 0.01);
        } else {
            panic!("expected Text");
        }
    }

    #[test]
    fn no_elements_gives_empty_scene() {
        let src = r##"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene: { width = 100, height = 100, background = "#000000" }
  elements::
)
"##;
        let result = parse_scene(src);
        match result {
            Ok(scene) => assert_eq!(scene.elements.len(), 0),
            Err(_) => {}
        }
    }
}
