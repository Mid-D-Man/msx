// src/renderer.rs
//! Scene → SVG string.
//! Produces pixel-perfect, standards-compliant SVG 1.1.

use crate::ast::*;
use crate::color::Paint;
use crate::path::commands_to_d;
use crate::primitives::fmt_f64;
use crate::transform::Transform;

// ── Public API ────────────────────────────────────────────────────────────────

/// Render a Scene to an SVG string.
pub fn render(scene: &Scene) -> String {
    let mut svg = String::with_capacity(estimate_capacity(scene));
    render_to(&mut svg, scene);
    svg
}

pub fn render_to(out: &mut String, scene: &Scene) {
    let w = fmt_f64(scene.canvas.width);
    let h = fmt_f64(scene.canvas.height);

    let viewbox_attr = if let Some(ref vb) = scene.canvas.viewbox {
        format!(r#" viewBox="{}""#, vb.to_svg_attr())
    } else {
        format!(r#" viewBox="0 0 {} {}""#, w, h)
    };

    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"{}>
"#,
        w, h, viewbox_attr
    ));

    // Background rect
    let bg = scene.canvas.background.to_svg_hex();
    out.push_str(&format!(
        r#"<rect width="{}" height="{}" fill="{}"/>
"#,
        w, h, bg
    ));

    // Defs section
    if !scene.defs.is_empty() {
        out.push_str("<defs>\n");
        for def in &scene.defs {
            out.push_str(&def.to_svg());
            out.push('\n');
        }
        out.push_str("</defs>\n");
    }

    // Elements
    for elem in &scene.elements {
        render_element(out, elem, 0);
    }

    out.push_str("</svg>");
}

// ── Element rendering ─────────────────────────────────────────────────────────

fn render_element(out: &mut String, elem: &Element, depth: usize) {
    match elem {
        Element::Rect(e)     => render_rect(out, e, depth),
        Element::Circle(e)   => render_circle(out, e, depth),
        Element::Ellipse(e)  => render_ellipse(out, e, depth),
        Element::Line(e)     => render_line(out, e, depth),
        Element::Polyline(e) => render_polyline(out, e, depth),
        Element::Polygon(e)  => render_polygon(out, e, depth),
        Element::Path(e)     => render_path(out, e, depth),
        Element::Text(e)     => render_text(out, e, depth),
        Element::Group(e)    => render_group(out, e, depth),
        Element::Use(e)      => render_use(out, e, depth),
    }
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

// ── Shared attribute helpers ──────────────────────────────────────────────────

fn id_attr(id: &Option<String>) -> String {
    match id {
        Some(ref s) => format!(r#" id="{}""#, escape_xml(s)),
        None        => String::new(),
    }
}

fn transform_attr(t: &Option<Transform>) -> String {
    match t {
        None    => String::new(),
        Some(t) => {
            let s = t.to_svg_attr();
            if s.is_empty() { String::new() } else { format!(r#" transform="{}""#, s) }
        }
    }
}

fn style_attrs(style: &crate::style::Style) -> String {
    style.to_svg_attrs()
        .iter()
        .map(|(k, v)| format!(r#" {}="{}""#, k, escape_xml(v)))
        .collect()
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('"', "&quot;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
}

// ── Concrete renderers ────────────────────────────────────────────────────────

fn render_rect(out: &mut String, e: &Rect, depth: usize) {
    let ind = indent(depth);
    let mut s = format!(
        r#"{}<rect x="{}" y="{}" width="{}" height="{}""#,
        ind,
        fmt_f64(e.x), fmt_f64(e.y),
        fmt_f64(e.width), fmt_f64(e.height),
    );
    if let Some(rx) = e.rx { s.push_str(&format!(r#" rx="{}""#, fmt_f64(rx))); }
    if let Some(ry) = e.ry { s.push_str(&format!(r#" ry="{}""#, fmt_f64(ry))); }
    s.push_str(&id_attr(&e.id));
    s.push_str(&transform_attr(&e.transform));
    s.push_str(&style_attrs(&e.style));
    s.push_str("/>\n");
    out.push_str(&s);
}

fn render_circle(out: &mut String, e: &Circle, depth: usize) {
    let ind = indent(depth);
    out.push_str(&format!(
        r#"{}<circle cx="{}" cy="{}" r="{}"{}{}{}/>"#,
        ind,
        fmt_f64(e.cx), fmt_f64(e.cy), fmt_f64(e.r),
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_attrs(&e.style),
    ));
    out.push('\n');
}

fn render_ellipse(out: &mut String, e: &Ellipse, depth: usize) {
    let ind = indent(depth);
    out.push_str(&format!(
        r#"{}<ellipse cx="{}" cy="{}" rx="{}" ry="{}"{}{}{}/>"#,
        ind,
        fmt_f64(e.cx), fmt_f64(e.cy),
        fmt_f64(e.rx), fmt_f64(e.ry),
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_attrs(&e.style),
    ));
    out.push('\n');
}

fn render_line(out: &mut String, e: &Line, depth: usize) {
    let ind = indent(depth);
    out.push_str(&format!(
        r#"{}<line x1="{}" y1="{}" x2="{}" y2="{}"{}{}{}/>"#,
        ind,
        fmt_f64(e.x1), fmt_f64(e.y1),
        fmt_f64(e.x2), fmt_f64(e.y2),
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_attrs(&e.style),
    ));
    out.push('\n');
}

fn render_polyline(out: &mut String, e: &Polyline, depth: usize) {
    let ind     = indent(depth);
    let pts_str = points_to_str(&e.points);
    out.push_str(&format!(
        r#"{}<polyline points="{}"{}{}{}/>"#,
        ind, pts_str,
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_attrs(&e.style),
    ));
    out.push('\n');
}

fn render_polygon(out: &mut String, e: &Polyline, depth: usize) {
    let ind     = indent(depth);
    let pts_str = points_to_str(&e.points);
    out.push_str(&format!(
        r#"{}<polygon points="{}"{}{}{}/>"#,
        ind, pts_str,
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_attrs(&e.style),
    ));
    out.push('\n');
}

fn points_to_str(pts: &[crate::primitives::Point]) -> String {
    pts.iter()
        .map(|p| format!("{},{}", fmt_f64(p.x), fmt_f64(p.y)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_path(out: &mut String, e: &Path, depth: usize) {
    let ind = indent(depth);
    let d   = commands_to_d(&e.commands);
    out.push_str(&format!(
        r#"{}<path d="{}"{}{}{}/>"#,
        ind, d,
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_attrs(&e.style),
    ));
    out.push('\n');
}

fn render_text(out: &mut String, e: &Text, depth: usize) {
    let ind = indent(depth);
    out.push_str(&format!(
        r#"{}<text x="{}" y="{}"{}{}{}>{}",
        ind,
        fmt_f64(e.x), fmt_f64(e.y),
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_attrs(&e.style),
        escape_xml(&e.content),
    ));
    out.push_str("</text>\n");
}

fn render_group(out: &mut String, e: &Group, depth: usize) {
    let ind = indent(depth);
    let style_s = match &e.style {
        Some(s) => style_attrs(s),
        None    => String::new(),
    };

    out.push_str(&format!(
        r#"{}<g{}{}{}>
"#,
        ind,
        id_attr(&e.id),
        transform_attr(&e.transform),
        style_s,
    ));

    for child in &e.children {
        render_element(out, child, depth + 1);
    }

    out.push_str(&format!("{}</g>\n", ind));
}

fn render_use(out: &mut String, e: &Use, depth: usize) {
    let ind = indent(depth);
    out.push_str(&format!(
        r#"{}<use href="{}" x="{}" y="{}"{}{}/>"#,
        ind,
        escape_xml(&e.href),
        fmt_f64(e.x), fmt_f64(e.y),
        id_attr(&e.id),
        transform_attr(&e.transform),
    ));
    out.push('\n');
}

// ── Capacity estimation ───────────────────────────────────────────────────────

fn estimate_capacity(scene: &Scene) -> usize {
    // SVG header ~120 bytes + ~80 bytes per top-level element (rough estimate)
    120 + scene.elements.len() * 80 + scene.defs.len() * 150
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::ast::{Canvas, Circle, Scene};
    use crate::style::Style;

    fn circle_scene() -> Scene {
        let mut style = Style::empty();
        style.fill         = Some(Paint::Color(Color::rgb(255, 0, 0)));
        style.stroke       = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity      = Some(1.0);
        let mut scene = Scene::new(Canvas::new(200.0, 200.0, Color::WHITE));
        scene.elements.push(Element::Circle(Circle {
            cx: 100.0, cy: 100.0, r: 50.0,
            id: None, transform: None, style,
        }));
        scene
    }

    #[test]
    fn renders_svg_root_element() {
        let svg = render(&circle_scene());
        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn renders_xmlns() {
        let svg = render(&circle_scene());
        assert!(svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
    }

    #[test]
    fn renders_width_height() {
        let svg = render(&circle_scene());
        assert!(svg.contains(r#"width="200""#));
        assert!(svg.contains(r#"height="200""#));
    }

    #[test]
    fn renders_background_rect() {
        let svg = render(&circle_scene());
        assert!(svg.contains(r#"<rect width="200" height="200" fill="#ffffff"/>"#));
    }

    #[test]
    fn renders_circle_element() {
        let svg = render(&circle_scene());
        assert!(svg.contains("<circle"));
        assert!(svg.contains(r#"cx="100""#));
        assert!(svg.contains(r#"cy="100""#));
        assert!(svg.contains(r#"r="50""#));
    }

    #[test]
    fn renders_circle_fill_color() {
        let svg = render(&circle_scene());
        assert!(svg.contains(r#"fill="#ff0000""#));
    }

    #[test]
    fn renders_defs_section() {
        use crate::color::{LinearGradient, Stop};
        let mut scene = Scene::new(Canvas::new(400.0, 300.0, Color::WHITE));
        scene.defs.push(Def::LinearGradient(LinearGradient::new(
            "grad1".to_string(),
            0.0, 0.0, 1.0, 0.0,
            vec![
                Stop::new(0.0, Color::rgb(255, 0, 0)),
                Stop::new(1.0, Color::rgb(0, 0, 255)),
            ],
        )));
        let svg = render(&scene);
        assert!(svg.contains("<defs>"));
        assert!(svg.contains("linearGradient"));
        assert!(svg.contains(r#"id="grad1""#));
    }

    #[test]
    fn renders_group_with_nesting() {
        use crate::ast::{Rect, Group};
        let mut style = Style::empty();
        style.fill = Some(Paint::Color(Color::rgb(0, 128, 255)));
        style.stroke = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity = Some(1.0);
        let rect = Element::Rect(Rect::new(0.0, 0.0, 50.0, 50.0, style));
        let group = Element::Group(Group {
            children:  vec![rect],
            id:        Some("mygroup".to_string()),
            transform: Some(Transform::Translate { x: 10.0, y: 20.0 }),
            style:     None,
        });
        let mut scene = Scene::new(Canvas::new(200.0, 200.0, Color::WHITE));
        scene.elements.push(group);
        let svg = render(&scene);
        assert!(svg.contains(r#"<g id="mygroup""#));
        assert!(svg.contains(r#"transform="translate(10,20)""#));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("</g>"));
    }

    #[test]
    fn renders_text_element() {
        use crate::ast::Text;
        use crate::style::TextAnchor;
        let mut style = Style::empty();
        style.fill        = Some(Paint::Color(Color::BLACK));
        style.font_size   = Some(16.0);
        style.text_anchor = Some(TextAnchor::Middle);
        style.stroke      = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity      = Some(1.0);
        let text = Element::Text(Text::new(100.0, 50.0, "Hello".to_string(), style));
        let mut scene = Scene::new(Canvas::new(200.0, 100.0, Color::WHITE));
        scene.elements.push(text);
        let svg = render(&scene);
        assert!(svg.contains("<text"));
        assert!(svg.contains("Hello"));
        assert!(svg.contains(r#"text-anchor="middle""#));
    }

    #[test]
    fn xml_escaping_in_text() {
        use crate::ast::Text;
        let mut style = Style::empty();
        style.fill = Some(Paint::Color(Color::BLACK));
        style.stroke = Some(Paint::None);
        style.stroke_width = Some(0.0);
        style.opacity = Some(1.0);
        let text = Element::Text(Text::new(
            10.0, 10.0,
            "5 < 10 & x > 0".to_string(),
            style,
        ));
        let mut scene = Scene::new(Canvas::new(100.0, 100.0, Color::WHITE));
        scene.elements.push(text);
        let svg = render(&scene);
        assert!(svg.contains("5 &lt; 10 &amp; x &gt; 0"));
    }
}
