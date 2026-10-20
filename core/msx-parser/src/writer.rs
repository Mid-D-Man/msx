// core/msx-parser/src/writer.rs
//! Scene to `.msx` source text, the inverse of `parse_scene`.
//!
//! The output is plain DixScript that `parse_scene` reads back into the
//! same scene. Numbers print at full precision (the shortest text that
//! parses back to the same `f64`), so nothing is rounded on the way out.
//!
//! Supported: canvas and `viewbox`, linear, radial and conic gradients,
//! and the elements rect, circle, ellipse, line, polyline, polygon, path,
//! text, group, use and layer (without effects). Anything else returns
//! [`WriteError::Unsupported`] instead of writing a file that would lose
//! content: SDF nodes, splats, images, shader and audio defs, layer
//! effects and animations.

use std::fmt::{self, Write as _};

use msx_ast::{
    BlendMode, Color, Def, Element, FontWeight, Group, Layer, Paint, PathCommand, Point, Scene, Stop, Style, Transform,
};

/// Why a scene could not be written.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteError {
    /// The scene holds something the writer cannot express yet.
    Unsupported(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Unsupported(what) => write!(f, "cannot write {what} to .msx source yet"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Write `scene` as `.msx` source text.
pub fn scene_to_msx(scene: &Scene) -> Result<String, WriteError> {
    if !scene.animations.is_empty() || scene.duration != 0.0 {
        return Err(WriteError::Unsupported("animations".into()));
    }

    let mut out = String::new();
    out.push_str("@CONFIG( version -> \"1.0.0\" )\n@DATA(\n");

    let c = &scene.canvas;
    let _ = writeln!(
        out,
        "  scene = {{ width = {}, height = {}, background = {} }}",
        num(c.width),
        num(c.height),
        color_value(c.background)
    );
    if let Some(vb) = &c.viewbox {
        let _ = writeln!(
            out,
            "  viewbox = {{ min_x = {}, min_y = {}, width = {}, height = {} }}",
            num(vb.min_x),
            num(vb.min_y),
            num(vb.width),
            num(vb.height)
        );
    }

    if !scene.defs.is_empty() {
        out.push_str("  defs::\n");
        for def in &scene.defs {
            let _ = writeln!(out, "    {}", def_object(def)?);
        }
    }

    if !scene.elements.is_empty() {
        out.push_str("  elements::\n");
        for el in &scene.elements {
            write_element(&mut out, el, 4, false)?;
            out.push('\n');
        }
    }

    out.push_str(")\n");
    Ok(out)
}

// ── Scalars ──────────────────────────────────────────────────────────────────

/// Shortest text that parses back to the same value. Whole numbers print
/// without a fraction. Non-finite values cannot appear in source, so they
/// print as 0.
fn num(v: f64) -> String {
    if !v.is_finite() || v == 0.0 {
        return "0".into();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    format!("{v}")
}

/// Same as [`num`], but always with a decimal point, so an array of
/// numbers is read as floats and never as a mix of integers and floats.
fn float(v: f64) -> String {
    let s = num(v);
    if s.contains('.') { s } else { format!("{s}.0") }
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Opaque colors print as a bare hex literal, like the hand-written
/// examples. Colors with alpha print as a quoted 8-digit hex string.
fn color_value(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("\"#{:02x}{:02x}{:02x}{:02x}\"", c.r, c.g, c.b, c.a)
    }
}

fn paint_value(p: &Paint) -> String {
    match p {
        Paint::None => quote("none"),
        Paint::Color(c) => color_value(*c),
        Paint::Ref(r) => quote(r),
        Paint::CurrentColor => quote("currentColor"),
    }
}

fn transform_text(t: &Transform) -> Option<String> {
    Some(match t {
        Transform::None => return None,
        Transform::Translate { x, y } => format!("translate({} {})", num(*x), num(*y)),
        Transform::Scale { x, y } => format!("scale({} {})", num(*x), num(*y)),
        Transform::Rotate { angle, cx: Some(cx), cy: Some(cy) } => format!("rotate({} {} {})", num(*angle), num(*cx), num(*cy)),
        Transform::Rotate { angle, .. } => format!("rotate({})", num(*angle)),
        Transform::SkewX(a) => format!("skewX({})", num(*a)),
        Transform::SkewY(a) => format!("skewY({})", num(*a)),
        Transform::Matrix(m) => format!("matrix({} {} {} {} {} {})", num(m.a), num(m.b), num(m.c), num(m.d), num(m.e), num(m.f)),
        Transform::Multiple(list) => {
            let parts: Vec<String> = list.iter().filter_map(transform_text).collect();
            if parts.is_empty() {
                return None;
            }
            parts.join(" ")
        }
    })
}

// ── Style ────────────────────────────────────────────────────────────────────

/// Style fields at the parser's defaults are left out, because the parser
/// starts from those defaults anyway.
fn style_object(s: &Style) -> Option<String> {
    let default = Style::default();
    let mut f: Vec<String> = Vec::new();

    if let Some(p) = &s.fill {
        if Some(p) != default.fill.as_ref() {
            f.push(format!("fill = {}", paint_value(p)));
        }
    }
    if let Some(p) = &s.stroke {
        if Some(p) != default.stroke.as_ref() {
            f.push(format!("stroke = {}", paint_value(p)));
        }
    }
    if let Some(w) = s.stroke_width {
        if Some(w) != default.stroke_width {
            f.push(format!("stroke_width = {}", num(w)));
        }
    }
    if let Some(o) = s.opacity {
        if Some(o) != default.opacity {
            f.push(format!("opacity = {}", num(o)));
        }
    }
    if let Some(o) = s.fill_opacity {
        f.push(format!("fill_opacity = {}", num(o)));
    }
    if let Some(o) = s.stroke_opacity {
        f.push(format!("stroke_opacity = {}", num(o)));
    }
    if let Some(r) = s.fill_rule {
        f.push(format!("fill_rule = {}", quote(r.to_svg())));
    }
    if let Some(c) = s.stroke_linecap {
        f.push(format!("stroke_linecap = {}", quote(c.to_svg())));
    }
    if let Some(j) = s.stroke_linejoin {
        f.push(format!("stroke_linejoin = {}", quote(j.to_svg())));
    }
    if let Some(m) = s.stroke_miterlimit {
        f.push(format!("stroke_miterlimit = {}", num(m)));
    }
    if let Some(d) = &s.stroke_dasharray {
        let items: Vec<String> = d.iter().map(|v| float(*v)).collect();
        f.push(format!("stroke_dasharray = [{}]", items.join(", ")));
    }
    if let Some(o) = s.stroke_dashoffset {
        f.push(format!("stroke_dashoffset = {}", num(o)));
    }
    if let Some(v) = s.font_size {
        f.push(format!("font_size = {}", num(v)));
    }
    if let Some(v) = &s.font_family {
        f.push(format!("font_family = {}", quote(v)));
    }
    if let Some(w) = &s.font_weight {
        f.push(format!("font_weight = {}", quote(&font_weight_text(w))));
    }
    if let Some(a) = s.text_anchor {
        f.push(format!("text_anchor = {}", quote(a.to_svg())));
    }
    if let Some(b) = &s.dominant_baseline {
        f.push(format!("dominant_baseline = {}", quote(b)));
    }
    if s.visibility_hidden {
        f.push("visibility = \"hidden\"".into());
    }
    if s.display_none {
        f.push("display = \"none\"".into());
    }

    if f.is_empty() { None } else { Some(format!("{{ {} }}", f.join(", "))) }
}

fn font_weight_text(w: &FontWeight) -> String {
    w.to_svg()
}

// ── Defs ─────────────────────────────────────────────────────────────────────

fn stops_array(stops: &[Stop]) -> String {
    let items: Vec<String> = stops
        .iter()
        .map(|s| {
            let opaque = Color::rgb(s.color.r, s.color.g, s.color.b);
            if s.color.a == 255 {
                format!("{{ offset = {}, color = {} }}", num(s.offset), color_value(opaque))
            } else {
                format!(
                    "{{ offset = {}, color = {}, opacity = {} }}",
                    num(s.offset),
                    color_value(opaque),
                    num(s.color.a as f64 / 255.0)
                )
            }
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn def_object(def: &Def) -> Result<String, WriteError> {
    Ok(match def {
        Def::LinearGradient(g) => format!(
            "{{ type = \"linear_gradient\", id = {}, x1 = {}, y1 = {}, x2 = {}, y2 = {}, stops = {} }}",
            quote(&g.id),
            num(g.x1),
            num(g.y1),
            num(g.x2),
            num(g.y2),
            stops_array(&g.stops)
        ),
        Def::RadialGradient(g) => format!(
            "{{ type = \"radial_gradient\", id = {}, cx = {}, cy = {}, r = {}, fx = {}, fy = {}, stops = {} }}",
            quote(&g.id),
            num(g.cx),
            num(g.cy),
            num(g.r),
            num(g.fx),
            num(g.fy),
            stops_array(&g.stops)
        ),
        Def::ConicGradient(g) => format!(
            "{{ type = \"conic_gradient\", id = {}, cx = {}, cy = {}, angle = {}, stops = {} }}",
            quote(&g.id),
            num(g.cx),
            num(g.cy),
            num(g.angle),
            stops_array(&g.stops)
        ),
        Def::Shader(_) => return Err(WriteError::Unsupported("shader defs".into())),
        Def::Audio(_) => return Err(WriteError::Unsupported("audio defs".into())),
    })
}

// ── Elements ─────────────────────────────────────────────────────────────────

fn blend_mode_text(b: BlendMode) -> &'static str {
    match b {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Add => "add",
        BlendMode::SoftLight => "soft_light",
        BlendMode::HardLight => "hard_light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::Subtract => "subtract",
        BlendMode::Divide => "divide",
    }
}

fn path_data(cmds: &[PathCommand]) -> String {
    let p = |pt: &Point| format!("{} {}", num(pt.x), num(pt.y));
    cmds.iter()
        .map(|c| match c {
            PathCommand::MoveTo(a) => format!("M {}", p(a)),
            PathCommand::LineTo(a) => format!("L {}", p(a)),
            PathCommand::HLineTo(x) => format!("H {}", num(*x)),
            PathCommand::VLineTo(y) => format!("V {}", num(*y)),
            PathCommand::CubicBezier { c1, c2, to } => format!("C {} {} {}", p(c1), p(c2), p(to)),
            PathCommand::SmoothCubic { c2, to } => format!("S {} {}", p(c2), p(to)),
            PathCommand::QuadBezier { c, to } => format!("Q {} {}", p(c), p(to)),
            PathCommand::SmoothQuad { to } => format!("T {}", p(to)),
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } => {
                format!("A {} {} {} {} {} {}", num(*rx), num(*ry), num(*x_rotation), *large_arc as u8, *sweep as u8, p(to))
            }
            PathCommand::RelMoveTo(a) => format!("m {}", p(a)),
            PathCommand::RelLineTo(a) => format!("l {}", p(a)),
            PathCommand::RelHLineTo(x) => format!("h {}", num(*x)),
            PathCommand::RelVLineTo(y) => format!("v {}", num(*y)),
            PathCommand::RelCubicBezier { c1, c2, to } => format!("c {} {} {}", p(c1), p(c2), p(to)),
            PathCommand::RelSmoothCubic { c2, to } => format!("s {} {}", p(c2), p(to)),
            PathCommand::RelQuadBezier { c, to } => format!("q {} {}", p(c), p(to)),
            PathCommand::RelSmoothQuad { to } => format!("t {}", p(to)),
            PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
                format!("a {} {} {} {} {} {}", num(*rx), num(*ry), num(*x_rotation), *large_arc as u8, *sweep as u8, p(to))
            }
            PathCommand::ClosePath => "Z".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `id`, `transform` and `style` tails shared by every element.
fn common(id: &Option<String>, transform: &Option<Transform>, style: Option<&Style>) -> String {
    let mut s = String::new();
    if let Some(id) = id {
        let _ = write!(s, ", id = {}", quote(id));
    }
    if let Some(t) = transform.as_ref().and_then(transform_text) {
        let _ = write!(s, ", transform = {}", quote(&t));
    }
    if let Some(style) = style.and_then(style_object) {
        let _ = write!(s, ", style = {style}");
    }
    s
}

fn pad(n: usize) -> String {
    " ".repeat(n)
}

/// Write one element at `indent` spaces. `comma` adds a trailing comma, for
/// entries inside a `[...]` array.
fn write_element(out: &mut String, el: &Element, indent: usize, comma: bool) -> Result<(), WriteError> {
    let tail = if comma { "," } else { "" };
    match el {
        Element::Rect(e) => {
            let mut s = format!("{{ type = \"rect\", x = {}, y = {}, width = {}, height = {}", num(e.x), num(e.y), num(e.width), num(e.height));
            if let Some(rx) = e.rx {
                let _ = write!(s, ", rx = {}", num(rx));
            }
            if let Some(ry) = e.ry {
                let _ = write!(s, ", ry = {}", num(ry));
            }
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, Some(&e.style)));
        }
        Element::Circle(e) => {
            let s = format!("{{ type = \"circle\", cx = {}, cy = {}, r = {}", num(e.cx), num(e.cy), num(e.r));
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, Some(&e.style)));
        }
        Element::Ellipse(e) => {
            let s = format!("{{ type = \"ellipse\", cx = {}, cy = {}, rx = {}, ry = {}", num(e.cx), num(e.cy), num(e.rx), num(e.ry));
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, Some(&e.style)));
        }
        Element::Line(e) => {
            let s = format!("{{ type = \"line\", x1 = {}, y1 = {}, x2 = {}, y2 = {}", num(e.x1), num(e.y1), num(e.x2), num(e.y2));
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, Some(&e.style)));
        }
        Element::Polyline(e) | Element::Polygon(e) => {
            let kind = if e.closed { "polygon" } else { "polyline" };
            let pts: Vec<String> = e.points.iter().map(|p| format!("[{}, {}]", float(p.x), float(p.y))).collect();
            let s = format!("{{ type = \"{kind}\", points = [{}]", pts.join(", "));
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, Some(&e.style)));
        }
        Element::Path(e) => {
            let d = if e.d_raw.is_empty() { path_data(&e.commands) } else { e.d_raw.clone() };
            let s = format!("{{ type = \"path\", d = {}", quote(&d));
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, Some(&e.style)));
        }
        Element::Text(e) => {
            let s = format!("{{ type = \"text\", x = {}, y = {}, content = {}", num(e.x), num(e.y), quote(&e.content));
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, Some(&e.style)));
        }
        Element::Use(e) => {
            let s = format!("{{ type = \"use\", href = {}, x = {}, y = {}", quote(&e.href), num(e.x), num(e.y));
            let _ = write!(out, "{}{}{} }}{tail}", pad(indent), s, common(&e.id, &e.transform, None));
        }
        Element::Group(g) => write_group(out, g, indent, tail)?,
        Element::Layer(l) => write_layer(out, l, indent, tail)?,
        Element::Sdf(_) => return Err(WriteError::Unsupported("sdf elements".into())),
        Element::Splat(_) => return Err(WriteError::Unsupported("splat elements".into())),
        Element::Image(_) => return Err(WriteError::Unsupported("image elements".into())),
    }
    Ok(())
}

fn write_children(out: &mut String, children: &[Element], indent: usize) -> Result<(), WriteError> {
    write_element_list(out, "elements", children, indent)
}

fn write_element_list(out: &mut String, field: &str, children: &[Element], indent: usize) -> Result<(), WriteError> {
    out.push_str(&format!("{}{} = [\n", pad(indent), field));
    for (i, child) in children.iter().enumerate() {
        write_element(out, child, indent + 2, i + 1 < children.len())?;
        out.push('\n');
    }
    out.push_str(&format!("{}]", pad(indent)));
    Ok(())
}

fn write_group(out: &mut String, g: &Group, indent: usize, tail: &str) -> Result<(), WriteError> {
    let mut head = common(&g.id, &g.transform, g.style.as_ref());
    // A group style that is all defaults still has to be written, or the
    // parser reads the group back without one. Leaves never need this,
    // because the parser always builds a style for them.
    if g.style.as_ref().is_some_and(|s| style_object(s).is_none()) {
        head.push_str(", style = { }");
    }
    let _ = write!(out, "{}{{ type = \"group\"{}", pad(indent), head);
    out.push_str(",\n");
    write_children(out, &g.children, indent + 2)?;
    let _ = write!(out, " }}{tail}");
    Ok(())
}

fn write_layer(out: &mut String, l: &Layer, indent: usize, tail: &str) -> Result<(), WriteError> {
    if !l.effects.is_empty() {
        return Err(WriteError::Unsupported("layer effects".into()));
    }
    let mut head = format!("{}{{ type = \"layer\", blend_mode = {}, opacity = {}", pad(indent), quote(blend_mode_text(l.blend_mode)), num(l.opacity));
    if l.z_index != 0.0 {
        let _ = write!(head, ", z_index = {}", num(l.z_index));
    }
    if l.clip {
        head.push_str(", clip = true");
    }
    out.push_str(&head);
    out.push_str(&common(&l.id, &l.transform, None));
    out.push_str(",\n");
    if let Some(shapes) = &l.clip_path {
        write_element_list(out, "clip_path", shapes, indent + 2)?;
        out.push_str(",\n");
    }
    write_children(out, &l.children, indent + 2)?;
    let _ = write!(out, " }}{tail}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_scene;
    use msx_ast::{
        Canvas, Circle, ConicGradient, Ellipse, FillRule, Group, LineCap, LineJoin, LinearGradient, Matrix2D, Path, Polyline, RadialGradient,
        Rect, Text, TextAnchor, Use, ViewBox,
    };

    fn style() -> Style {
        Style::default()
    }

    fn scene_with(defs: Vec<Def>, elements: Vec<Element>) -> Scene {
        let mut s = Scene::new(Canvas::new(120.0, 80.0, Color::WHITE));
        s.defs = defs;
        s.elements = elements;
        s
    }

    /// Write, parse back, and require the parsed scene to equal the
    /// original. The AST has no `PartialEq`, so compare `Debug` output,
    /// which prints floats as the shortest text that round-trips.
    fn assert_roundtrip(scene: &Scene) {
        let text = scene_to_msx(scene).expect("scene should be writable");
        let back = parse_scene(&text).unwrap_or_else(|e| panic!("written source must parse: {e}\n{text}"));
        assert_eq!(format!("{:?}", scene.canvas), format!("{:?}", back.canvas), "canvas\n{text}");
        assert_eq!(format!("{:?}", scene.defs), format!("{:?}", back.defs), "defs\n{text}");
        assert_eq!(format!("{:?}", scene.elements), format!("{:?}", back.elements), "elements\n{text}");
    }

    fn rect(style: Style) -> Element {
        Element::Rect(Rect { x: 1.5, y: 2.0, width: 30.25, height: 40.0, rx: Some(3.0), ry: None, id: Some("r1".into()), transform: None, style })
    }

    #[test]
    fn every_shape_roundtrips() {
        let mut st = style();
        st.fill = Some(Paint::Color(Color::rgb(233, 69, 96)));
        st.stroke = Some(Paint::Color(Color::rgb(0, 0, 0)));
        st.stroke_width = Some(2.5);
        let elements = vec![
            rect(st.clone()),
            Element::Circle(Circle { cx: 10.0, cy: 20.0, r: 5.5, id: None, transform: None, style: st.clone() }),
            Element::Ellipse(Ellipse { cx: 1.0, cy: 2.0, rx: 3.0, ry: 4.0, id: None, transform: None, style: st.clone() }),
            Element::Line(msx_ast::Line { x1: 0.0, y1: 0.0, x2: 9.0, y2: -9.0, id: None, transform: None, style: st.clone() }),
            Element::Polyline(Polyline { points: vec![Point::new(0.0, 0.0), Point::new(5.0, 5.5), Point::new(9.0, 0.0)], closed: false, id: None, transform: None, style: st.clone() }),
            Element::Polygon(Polyline { points: vec![Point::new(0.0, 0.0), Point::new(5.0, 5.0), Point::new(9.0, 0.0)], closed: true, id: None, transform: None, style: st.clone() }),
        ];
        assert_roundtrip(&scene_with(vec![], elements));
    }

    #[test]
    fn path_text_and_use_roundtrip() {
        let d = "M 10 10 h 20 v -5 c 1 2 3 4 5 6 a 5 5 0 1 0 10 10 Z".to_string();
        let path = Element::Path(Path { commands: msx_ast::path::parse_d(&d).unwrap(), d_raw: d, id: Some("p".into()), transform: None, style: style() });
        let mut ts = style();
        ts.font_size = Some(18.0);
        ts.font_family = Some("DejaVu Sans, sans-serif".into());
        ts.font_weight = Some(FontWeight::Numeric(700));
        ts.text_anchor = Some(TextAnchor::Middle);
        ts.dominant_baseline = Some("middle".into());
        let text = Element::Text(Text { x: 60.0, y: 40.0, content: "Hello".into(), id: None, transform: None, style: ts });
        let u = Element::Use(Use { href: "#p".into(), x: 5.0, y: 6.0, id: None, transform: Some(Transform::Scale { x: 2.0, y: 2.0 }) });
        assert_roundtrip(&scene_with(vec![], vec![path, text, u]));
    }

    #[test]
    fn style_fields_roundtrip() {
        let mut st = style();
        st.fill = Some(Paint::Ref("url(#g)".into()));
        st.stroke = Some(Paint::CurrentColor);
        st.stroke_width = Some(0.0);
        st.opacity = Some(0.5);
        st.fill_opacity = Some(0.25);
        st.stroke_opacity = Some(0.75);
        st.fill_rule = Some(FillRule::EvenOdd);
        st.stroke_linecap = Some(LineCap::Round);
        st.stroke_linejoin = Some(LineJoin::Bevel);
        st.stroke_miterlimit = Some(7.5);
        st.stroke_dasharray = Some(vec![4.0, 2.0, 1.5]);
        st.stroke_dashoffset = Some(3.0);
        st.visibility_hidden = true;
        st.display_none = true;
        assert_roundtrip(&scene_with(vec![], vec![rect(st)]));
    }

    #[test]
    fn colors_with_alpha_roundtrip() {
        let mut st = style();
        st.fill = Some(Paint::Color(Color::rgba(10, 20, 30, 128)));
        st.stroke = Some(Paint::Color(Color::rgba(255, 255, 255, 1)));
        let mut s = scene_with(vec![], vec![rect(st)]);
        s.canvas.background = Color::rgba(1, 2, 3, 4);
        assert_roundtrip(&s);
    }

    #[test]
    fn gradients_roundtrip_including_stop_alpha() {
        let stops = vec![Stop::new(0.0, Color::rgb(255, 0, 0)), Stop::new(0.4, Color::rgba(0, 255, 0, 100)), Stop::new(1.0, Color::rgba(0, 0, 255, 0))];
        let defs = vec![
            Def::LinearGradient(LinearGradient::new("l".into(), 0.1, 0.2, 0.9, 0.8, stops.clone())),
            Def::RadialGradient(RadialGradient::new("r".into(), 0.5, 0.5, 0.4, 0.3, 0.35, stops.clone())),
            Def::ConicGradient(ConicGradient::new("c".into(), 0.5, 0.5, 45.0, stops)),
        ];
        assert_roundtrip(&scene_with(defs, vec![]));
    }

    #[test]
    fn nested_groups_and_layers_roundtrip() {
        let inner = Element::Group(Group { children: vec![rect(style())], id: Some("inner".into()), transform: Some(Transform::Translate { x: 3.0, y: 4.0 }), style: None });
        let mut layer = Layer::new(vec![inner.clone(), rect(style())]);
        layer.opacity = 0.4;
        layer.blend_mode = BlendMode::Multiply;
        layer.z_index = 2.0;
        layer.clip = true;
        layer.id = Some("lay".into());
        let outer = Element::Group(Group { children: vec![inner, Element::Layer(layer)], id: None, transform: None, style: Some(style()) });
        assert_roundtrip(&scene_with(vec![], vec![outer]));
    }

    #[test]
    fn transforms_roundtrip_without_rounding() {
        let list = Transform::Multiple(vec![
            Transform::Translate { x: 10.123456789, y: -0.000123 },
            Transform::Rotate { angle: 33.3333333, cx: Some(5.0), cy: Some(6.0) },
            Transform::Scale { x: 1.5, y: 0.25 },
            Transform::SkewX(10.0),
            Transform::Matrix(Matrix2D { a: 0.9999999, b: 0.0001, c: -0.0001, d: 0.9999999, e: 100.5, f: -7.25 }),
        ]);
        let el = Element::Rect(Rect { x: 0.0, y: 0.0, width: 1.0, height: 1.0, rx: None, ry: None, id: None, transform: Some(list), style: style() });
        assert_roundtrip(&scene_with(vec![], vec![el]));
    }

    #[test]
    fn numbers_keep_full_precision() {
        let el = Element::Circle(Circle { cx: 0.123456789012, cy: 12345.6789, r: 1e-7, id: None, transform: None, style: style() });
        assert_roundtrip(&scene_with(vec![], vec![el]));
    }

    #[test]
    fn awkward_strings_roundtrip() {
        let nasty = "quote \" backslash \\ newline \n tab \t brace {x} slash // hash # dollar $ unicode é ñ 日本 emoji 🎉 end";
        let mut st = style();
        st.font_family = Some("Fira \"Code\", 'Mono'".into());
        let el = Element::Text(Text { x: 1.0, y: 2.0, content: nasty.into(), id: Some("id \"x\"".into()), transform: None, style: st });
        assert_roundtrip(&scene_with(vec![], vec![el]));
    }

    #[test]
    fn viewbox_roundtrips() {
        let mut s = scene_with(vec![], vec![rect(style())]);
        s.canvas.viewbox = Some(ViewBox::new(-5.0, 10.0, 24.0, 24.0));
        assert_roundtrip(&s);
    }

    #[test]
    fn layer_clip_paths_roundtrip_including_the_empty_one() {
        let mut even = style();
        even.fill_rule = Some(FillRule::EvenOdd);
        let shapes = vec![
            rect(style()),
            Element::Circle(Circle { cx: 5.0, cy: 6.0, r: 7.5, id: None, transform: Some(Transform::Translate { x: 1.0, y: 2.0 }), style: even }),
        ];
        let clipped = Layer::new(vec![rect(style())]).with_clip_path(shapes);
        let nothing = Layer::new(vec![rect(style())]).with_clip_path(vec![]);
        let plain = Layer::new(vec![rect(style())]);
        assert_roundtrip(&scene_with(vec![], vec![Element::Layer(clipped), Element::Layer(nothing), Element::Layer(plain)]));
    }

    #[test]
    fn an_empty_clip_path_stays_empty_and_no_clip_path_stays_none() {
        let s = scene_with(vec![], vec![Element::Layer(Layer::new(vec![]).with_clip_path(vec![])), Element::Layer(Layer::new(vec![]))]);
        let back = parse_scene(&scene_to_msx(&s).unwrap()).unwrap();
        let clips: Vec<Option<usize>> = back.elements.iter().map(|e| match e { Element::Layer(l) => l.clip_path.as_ref().map(Vec::len), _ => panic!("layer") }).collect();
        assert_eq!(clips, vec![Some(0), None]);
    }

    #[test]
    fn empty_scene_roundtrips() {
        assert_roundtrip(&scene_with(vec![], vec![]));
    }

    #[test]
    fn unsupported_content_is_refused_not_dropped() {
        let sdf = Element::Sdf(msx_ast::SdfNode::new(msx_ast::SdfTree::Circle { cx: 0.0, cy: 0.0, r: 1.0 }, Paint::None));
        let err = scene_to_msx(&scene_with(vec![], vec![sdf])).unwrap_err();
        assert_eq!(err, WriteError::Unsupported("sdf elements".into()));

        let mut layer = Layer::new(vec![]);
        layer.effects = vec![msx_ast::Effect::Blur { radius: 2.0 }];
        assert!(scene_to_msx(&scene_with(vec![], vec![Element::Layer(layer)])).is_err());

        let mut animated = scene_with(vec![], vec![]);
        animated.duration = 2.0;
        assert!(scene_to_msx(&animated).is_err());
    }
}
