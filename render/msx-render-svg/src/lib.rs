// render/msx-render-svg/src/lib.rs
//! MSX → SVG export.
//!
//! Pixel-perfect for every SVG-native primitive (`rect`, `circle`, `ellipse`,
//! `line`, `polyline`/`polygon`, `path`, `text`, `group`, `use`, gradients).
//! `Sdf`/`Splat`/`Layer` are v0.2 primitives without a clean SVG 1.1
//! equivalent — see `sdf.rs` / `splat.rs` / `layer.rs` for what each one
//! degrades to. SVG is an export convenience here, not the primary output
//! target (that's `msx-render-cpu` / `msx-render-gpu`), so "good visual
//! approximation" is the bar for those three, not bit-for-bit fidelity.

mod layer;
mod sdf;
mod shapes;
mod splat;

use msx_ast::transform::Transform;
use msx_ast::{Def, Element, Scene, Style};

pub(crate) struct Ctx {
    pub(crate) out: String,
    pub(crate) extra_defs: String,
    counter: u32,
}

impl Ctx {
    pub(crate) fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// Generates a fresh, collision-free id for synthesized `<defs>` content
    /// (splat gradients, layer filters) — `"{prefix}{n}"`, `n` starting at 1.
    pub(crate) fn fresh_id(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{}{}", prefix, self.counter)
    }
}

pub fn render(scene: &Scene) -> String {
    let mut ctx = Ctx { out: String::new(), extra_defs: String::new(), counter: 0 };

    for el in &scene.elements {
        render_element(&mut ctx, el, &scene.defs);
    }

    let mut svg = String::new();
    let viewbox = scene
        .canvas
        .viewbox
        .map(|v| v.to_svg_attr())
        .unwrap_or_else(|| {
            format!("0 0 {} {}", msx_ast::fmt_f64(scene.canvas.width), msx_ast::fmt_f64(scene.canvas.height))
        });

    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{vb}">"#,
        w = msx_ast::fmt_f64(scene.canvas.width),
        h = msx_ast::fmt_f64(scene.canvas.height),
        vb = viewbox,
    ));

    svg.push_str(&format!(
        r#"<rect width="{w}" height="{h}" fill="{bg}"/>"#,
        w = msx_ast::fmt_f64(scene.canvas.width),
        h = msx_ast::fmt_f64(scene.canvas.height),
        bg = scene.canvas.background.to_svg_hex(),
    ));

    if !scene.defs.is_empty() || !ctx.extra_defs.is_empty() {
        svg.push_str("<defs>");
        for def in &scene.defs {
            svg.push_str(&def.to_svg());
        }
        svg.push_str(&ctx.extra_defs);
        svg.push_str("</defs>");
    }

    svg.push_str(&ctx.out);
    svg.push_str("</svg>");
    svg
}

pub(crate) fn render_element(ctx: &mut Ctx, element: &Element, defs: &[Def]) {
    match element {
        Element::Rect(e)     => shapes::render_rect(ctx, e),
        Element::Circle(e)   => shapes::render_circle(ctx, e),
        Element::Ellipse(e)  => shapes::render_ellipse(ctx, e),
        Element::Line(e)     => shapes::render_line(ctx, e),
        Element::Polyline(e) => shapes::render_polyline(ctx, e),
        Element::Polygon(e)  => shapes::render_polyline(ctx, e),
        Element::Path(e)     => shapes::render_path(ctx, e),
        Element::Text(e)     => shapes::render_text(ctx, e),
        Element::Group(e)    => shapes::render_group(ctx, e, defs),
        Element::Use(e)      => shapes::render_use(ctx, e),
        Element::Sdf(e)      => sdf::render_sdf(ctx, e),
        Element::Splat(e)    => splat::render_splat(ctx, e, defs),
        Element::Layer(e)    => layer::render_layer(ctx, e, defs),
    }
}

// ── Shared attribute-writing helpers ──────────────────────────────────────

pub(crate) fn write_attr(ctx: &mut Ctx, name: &str, value: impl AsRef<str>) {
    ctx.out.push(' ');
    ctx.out.push_str(name);
    ctx.out.push_str("=\"");
    ctx.out.push_str(value.as_ref());
    ctx.out.push('"');
}

pub(crate) fn write_id(ctx: &mut Ctx, id: Option<&str>) {
    if let Some(id) = id {
        write_attr(ctx, "id", escape_attr(id));
    }
}

pub(crate) fn write_transform(ctx: &mut Ctx, transform: Option<&Transform>) {
    if let Some(t) = transform {
        if !t.is_none() {
            write_attr(ctx, "transform", t.to_svg_attr());
        }
    }
}

pub(crate) fn write_style(ctx: &mut Ctx, style: &Style) {
    for (k, v) in style.to_svg_attrs() {
        write_attr(ctx, k, v);
    }
}

pub(crate) fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{
        BlendMode, Canvas, Circle, Color, Def, Effect, Element, GaussianSplat, Layer,
        LinearGradient, Paint, Rect, Scene, Stop, Style,
    };

    fn style_solid(color: Color) -> Style {
        Style {
            fill: Some(Paint::Color(color)),
            stroke: Some(Paint::None),
            stroke_width: Some(0.0),
            opacity: Some(1.0),
            ..Default::default()
        }
    }

    #[test]
    fn renders_basic_shapes_with_expected_attrs() {
        let mut scene = Scene::new(Canvas::new(100.0, 100.0, Color::WHITE));
        scene.elements.push(Element::Rect(Rect {
            x: 0.0, y: 0.0, width: 50.0, height: 20.0, rx: None, ry: None,
            id: None, transform: None, style: style_solid(Color::rgb(255, 0, 0)),
        }));
        scene.elements.push(Element::Circle(Circle {
            cx: 10.0, cy: 10.0, r: 5.0,
            id: None, transform: None, style: style_solid(Color::rgb(0, 255, 0)),
        }));

        let svg = render(&scene);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains(r#"width="50""#));
        // BUGFIX: was `r#"fill="#ff0000""#` — the embedded `"#` is the close
        // delimiter for a single-hash raw string, so this never compiled.
        // Bumped to double-hash; verified against rustc directly.
        assert!(svg.contains(r##"fill="#ff0000""##));
        assert!(svg.contains("<circle"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn renders_gradient_defs() {
        let mut scene = Scene::new(Canvas::new(10.0, 10.0, Color::WHITE));
        scene.defs.push(Def::LinearGradient(LinearGradient::new(
            "g1".into(), 0.0, 0.0, 1.0, 0.0,
            vec![Stop::new(0.0, Color::rgb(255, 0, 0)), Stop::new(1.0, Color::rgb(0, 0, 255))],
        )));

        let svg = render(&scene);
        assert!(svg.contains("<defs>"));
        assert!(svg.contains(r#"id="g1""#));
        assert!(svg.contains("linearGradient"));
    }

    #[test]
    fn polygon_and_polyline_share_closed_flag_dispatch() {
        use msx_ast::{Point, Polyline};
        let mut scene = Scene::new(Canvas::new(10.0, 10.0, Color::WHITE));
        let pts = vec![Point::new(0.0, 0.0), Point::new(5.0, 5.0), Point::new(0.0, 5.0)];
        scene.elements.push(Element::Polygon(Polyline {
            points: pts.clone(), closed: true, id: None, transform: None,
            style: style_solid(Color::BLACK),
        }));
        scene.elements.push(Element::Polyline(Polyline {
            points: pts, closed: false, id: None, transform: None,
            style: style_solid(Color::BLACK),
        }));

        let svg = render(&scene);
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("<polyline"));
    }

    #[test]
    fn layer_emits_blend_mode_css_and_filter() {
        let mut scene = Scene::new(Canvas::new(50.0, 50.0, Color::WHITE));
        let mut layer = Layer::new(vec![Element::Circle(Circle {
            cx: 10.0, cy: 10.0, r: 5.0,
            id: None, transform: None, style: style_solid(Color::rgb(10, 20, 30)),
        })]);
        layer.blend_mode = BlendMode::Multiply;
        layer.opacity = 0.5;
        layer.effects.push(Effect::Blur { radius: 4.0 });
        scene.elements.push(Element::Layer(layer));

        let svg = render(&scene);
        assert!(svg.contains("mix-blend-mode:multiply"));
        assert!(svg.contains(r#"opacity="0.5""#));
        assert!(svg.contains("<filter"));
        assert!(svg.contains("feGaussianBlur"));
    }

    #[test]
    fn splat_generates_radial_gradient_fallback() {
        let mut scene = Scene::new(Canvas::new(50.0, 50.0, Color::BLACK));
        scene.elements.push(Element::Splat(GaussianSplat::circle(25.0, 25.0, 8.0, Color::WHITE, 0.6)));

        let svg = render(&scene);
        assert!(svg.contains("radialGradient"));
        assert!(svg.contains("<ellipse"));
    }

    /// `fill`, when set, must change the synthetic gradient's stop color —
    /// not be silently ignored in favor of the plain `color` field. Uses
    /// an opaque-black → opaque-white gradient so its average
    /// (`#808080`-ish grey) is unambiguously distinct from the splat's own
    /// `color` (pure red) if `fill` were mistakenly skipped.
    #[test]
    fn splat_fill_overrides_color_in_the_svg_fallback_gradient() {
        let mut scene = Scene::new(Canvas::new(50.0, 50.0, Color::BLACK));
        scene.defs.push(Def::LinearGradient(LinearGradient::new(
            "g".to_string(), 0.0, 0.0, 10.0, 0.0,
            vec![Stop::new(0.0, Color::rgb(0, 0, 0)), Stop::new(1.0, Color::rgb(255, 255, 255))],
        )));
        let mut splat = GaussianSplat::circle(25.0, 25.0, 8.0, Color::rgb(255, 0, 0), 0.6);
        splat.fill = Some(Paint::Ref("url(#g)".to_string()));
        scene.elements.push(Element::Splat(splat));

        let svg = render(&scene);
        assert!(svg.contains("#7f7f7f"), "expected the gradient's averaged grey, got: {svg}");
        assert!(!svg.contains("#ff0000"), "the plain `color` field (red) must not appear when `fill` is set");
    }

    /// Same fill-resolution behavior must hold for a splat nested inside a
    /// `Group` — `defs` is threaded through `render_group`'s recursion
    /// specifically so this doesn't silently stop working the moment a
    /// splat isn't at the top level.
    #[test]
    fn splat_fill_resolves_correctly_even_nested_inside_a_group() {
        use msx_ast::Group;

        let mut scene = Scene::new(Canvas::new(50.0, 50.0, Color::BLACK));
        scene.defs.push(Def::LinearGradient(LinearGradient::new(
            "g".to_string(), 0.0, 0.0, 10.0, 0.0,
            vec![Stop::new(0.0, Color::rgb(0, 0, 0)), Stop::new(1.0, Color::rgb(255, 255, 255))],
        )));
        let mut splat = GaussianSplat::circle(25.0, 25.0, 8.0, Color::rgb(255, 0, 0), 0.6);
        splat.fill = Some(Paint::Ref("url(#g)".to_string()));
        let group = Group::new(vec![Element::Splat(splat)]);
        scene.elements.push(Element::Group(group));

        let svg = render(&scene);
        assert!(svg.contains("#7f7f7f"), "expected the gradient's averaged grey even nested in a group, got: {svg}");
    }

    #[test]
    fn sdf_falls_back_to_comment() {
        use msx_ast::{Paint, SdfNode, SdfTree};
        let mut scene = Scene::new(Canvas::new(50.0, 50.0, Color::BLACK));
        scene.elements.push(Element::Sdf(SdfNode::new(
            SdfTree::Circle { cx: 0.0, cy: 0.0, r: 10.0 },
            Paint::Color(Color::rgb(200, 50, 50)),
        )));

        let svg = render(&scene);
        assert!(svg.contains("<!-- sdf node 'circle'"));
    }

    #[test]
    fn shader_def_emits_a_documented_comment_not_a_paint_server() {
        use msx_ast::ShaderDef;
        let mut scene = Scene::new(Canvas::new(10.0, 10.0, Color::WHITE));
        scene.defs.push(Def::Shader(ShaderDef::new(
            "plasma_1", "shaders/plasma.wgsl", Color::rgb(107, 70, 255),
        )));

        let svg = render(&scene);
        assert!(svg.contains("<defs>"));
        assert!(svg.contains("<!-- shader 'plasma_1'"));
        // No linearGradient/radialGradient-style paint-server element for
        // it — SVG has no WGSL equivalent, so there's nothing to actually
        // resolve `url(#plasma_1)` against in a browser. That's expected
        // and documented in the comment itself, not a bug in this export.
        assert!(!svg.contains("<linearGradient") || !svg.contains(r#"id="plasma_1""#));
    }

    #[test]
    fn shape_referencing_a_shader_def_still_gets_its_url_fill_attribute() {
        use msx_ast::ShaderDef;
        let mut scene = Scene::new(Canvas::new(10.0, 10.0, Color::WHITE));
        scene.defs.push(Def::Shader(ShaderDef::new(
            "plasma_1", "shaders/plasma.wgsl", Color::rgb(107, 70, 255),
        )));
        let style = Style { fill: Some(Paint::Ref("url(#plasma_1)".to_string())), stroke: Some(Paint::None), ..Default::default() };
        scene.elements.push(Element::Rect(Rect::new(0.0, 0.0, 5.0, 5.0, style)));

        let svg = render(&scene);
        // The shape's own fill attribute is untouched by what kind of def
        // it points at — Paint::to_svg_value() just echoes the raw
        // "url(#id)" string the same way it would for a gradient. Whether
        // that resolves to anything visible is entirely down to what's
        // inside <defs>, which the previous test covers.
        assert!(svg.contains(r#"fill="url(#plasma_1)""#));
    }
                                                    }
