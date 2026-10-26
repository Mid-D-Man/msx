// core/msx-svg-import/src/tests.rs
use super::*;
use msx_ast::{Def, FontWeight, TextAnchor};

fn imp(svg: &str) -> Imported {
    import_svg(svg).unwrap_or_else(|e| panic!("import failed: {e}"))
}

/// An SVG document whose canvas is 100 by 100 with no viewBox.
fn doc(body: &str) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100">{body}</svg>"#)
}

fn rect(e: &Element) -> &Rect {
    match e {
        Element::Rect(r) => r,
        other => panic!("expected a rect, got {other:?}"),
    }
}

fn fill(style: &Style) -> Color {
    match &style.fill {
        Some(Paint::Color(c)) => *c,
        other => panic!("expected a solid fill, got {other:?}"),
    }
}

fn stroke(style: &Style) -> Color {
    match &style.stroke {
        Some(Paint::Color(c)) => *c,
        other => panic!("expected a solid stroke, got {other:?}"),
    }
}

fn has_warning(i: &Imported, needle: &str) -> bool {
    i.warnings.iter().any(|w| w.contains(needle))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ── Shapes and style ─────────────────────────────────────────────────────────

#[test]
fn basic_shapes_get_explicit_styles() {
    let i = imp(&doc(
        r##"<rect x="1" y="2" width="30" height="40" fill="#ff0000" stroke="#0000ff" stroke-width="2"/>
            <circle cx="50" cy="50" r="10"/>
            <ellipse cx="20" cy="20" rx="8" ry="4" fill="none"/>
            <line x1="0" y1="0" x2="10" y2="10" stroke="#000000" fill="#ff0000"/>"##,
    ));
    let els = &i.scene.elements;
    assert_eq!(els.len(), 4);
    let r = rect(&els[0]);
    assert_eq!((r.x, r.y, r.width, r.height), (1.0, 2.0, 30.0, 40.0));
    assert_eq!(fill(&r.style), Color::rgb(255, 0, 0));
    assert_eq!(stroke(&r.style), Color::rgb(0, 0, 255));
    assert_eq!(r.style.stroke_width, Some(2.0));
    let Element::Circle(c) = &els[1] else { panic!("circle") };
    assert_eq!(fill(&c.style), Color::BLACK, "SVG default fill is black");
    assert_eq!(c.style.stroke, Some(Paint::None));
    let Element::Ellipse(e) = &els[2] else { panic!("ellipse") };
    assert_eq!(e.style.fill, Some(Paint::None));
    let Element::Line(l) = &els[3] else { panic!("line") };
    assert_eq!(l.style.fill, Some(Paint::None), "a line never has a fill");
}

#[test]
fn style_is_inherited_through_groups_and_plain_groups_vanish() {
    let i = imp(&doc(
        r##"<g fill="#ff0000" stroke="#00ff00" stroke-width="3">
              <rect width="10" height="10"/>
              <g stroke="#0000ff"><circle r="5"/></g>
            </g>"##,
    ));
    let els = &i.scene.elements;
    assert_eq!(els.len(), 2, "groups with no transform, id or opacity are flattened");
    let r = rect(&els[0]);
    assert_eq!((fill(&r.style), stroke(&r.style), r.style.stroke_width), (Color::rgb(255, 0, 0), Color::rgb(0, 255, 0), Some(3.0)));
    let Element::Circle(c) = &els[1] else { panic!("circle") };
    assert_eq!((fill(&c.style), stroke(&c.style), c.style.stroke_width), (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255), Some(3.0)));
}

#[test]
fn group_transform_and_id_survive_and_group_opacity_becomes_a_layer() {
    let i = imp(&doc(
        r#"<g id="a" transform="translate(5 6)"><rect width="1" height="1"/></g>
           <g opacity="0.5"><rect width="1" height="1"/><rect width="2" height="2"/></g>
           <rect width="3" height="3" opacity="0.25"/>"#,
    ));
    let els = &i.scene.elements;
    let Element::Group(g) = &els[0] else { panic!("group") };
    assert_eq!(g.id.as_deref(), Some("a"));
    assert!(matches!(g.transform, Some(Transform::Translate { x, y }) if x == 5.0 && y == 6.0));
    let Element::Layer(l) = &els[1] else { panic!("a group with opacity is an isolated layer") };
    assert_eq!((l.opacity, l.children.len()), (0.5, 2));
    assert_eq!(rect(&els[2]).style.opacity, Some(0.25), "a shape's own opacity stays on the shape");
}

#[test]
fn css_cascade_order() {
    let i = imp(&doc(
        r##"<style>.a{fill:#00ff00} #x{fill:#0000ff} rect{fill:#ffff00} .imp{fill:#123456 !important}</style>
            <rect id="x" class="a" fill="#ff0000" width="1" height="1"/>
            <rect class="a" fill="#ff0000" width="1" height="1"/>
            <rect class="a" style="fill:#ff00ff" width="1" height="1"/>
            <rect class="imp" style="fill:#ff00ff" width="1" height="1"/>
            <rect fill="#ff0000" width="1" height="1"/>"##,
    ));
    let f: Vec<Color> = i.scene.elements.iter().map(|e| fill(&rect(e).style)).collect();
    assert_eq!(f[0], Color::rgb(0, 0, 255), "id rule beats class rule and attribute");
    assert_eq!(f[1], Color::rgb(0, 255, 0), "class rule beats the presentation attribute");
    assert_eq!(f[2], Color::rgb(255, 0, 255), "inline style beats rules");
    assert_eq!(f[3], Color::rgb(0x12, 0x34, 0x56), "!important beats inline style");
    assert_eq!(f[4], Color::rgb(255, 255, 0), "a type rule beats the presentation attribute");
}

#[test]
fn style_element_may_be_cdata() {
    let i = imp(&doc("<style type=\"text/css\"><![CDATA[ .k { fill: #010203; } ]]></style><rect class=\"k\" width=\"1\" height=\"1\"/>"));
    assert_eq!(fill(&rect(&i.scene.elements[0]).style), Color::rgb(1, 2, 3));
}

#[test]
fn unsupported_selectors_are_reported_not_misapplied() {
    let i = imp(&doc("<style>rect:hover { fill: #ff0000 } g + g { fill: #ff0000 }</style><g><rect width=\"1\" height=\"1\"/></g>"));
    assert!(has_warning(&i, "rect:hover") && has_warning(&i, "g + g"), "{:?}", i.warnings);
    assert_eq!(fill(&rect(&i.scene.elements[0]).style), Color::BLACK);
}

#[test]
fn descendant_child_and_attribute_selectors_apply() {
    let i = imp(&doc(r##"<style>g rect { fill: #ff0000 } svg > rect { fill: #00ff00 } [data-k] { stroke: #0000ff } g > rect:first-child { opacity: .5 }</style>
        <g><rect width="1" height="1"/></g><rect width="1" height="1" data-k="x"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let first = rect(&i.scene.elements[0]);
    assert_eq!(first.style.opacity, Some(0.5), ":first-child applied");
    assert_eq!(fill(&first.style), Color::rgb(255, 0, 0), "descendant selector");
    let second = rect(&i.scene.elements[1]);
    assert_eq!(fill(&second.style), Color::rgb(0, 255, 0), "child selector");
    assert_eq!(second.style.stroke, Some(Paint::Color(Color::rgb(0, 0, 255))), "attribute selector");
}

#[test]
fn fill_and_stroke_opacity_are_folded_into_alpha() {
    let i = imp(&doc(r##"<rect width="1" height="1" fill="#ff0000" fill-opacity="0.5" stroke="#0000ff" stroke-opacity="0.25"/>"##));
    let s = &rect(&i.scene.elements[0]).style;
    assert_eq!(fill(s), Color::rgba(255, 0, 0, 128));
    assert_eq!(stroke(s), Color::rgba(0, 0, 255, 64));
    assert!(s.fill_opacity.is_none() && s.stroke_opacity.is_none(), "the raster renderers ignore these keys, so none are written");
}

#[test]
fn current_color_uses_the_color_property() {
    let i = imp(&doc(r##"<g color="#00ff00"><rect width="1" height="1" fill="currentColor" stroke="currentColor"/></g>"##));
    let s = &rect(&i.scene.elements[0]).style;
    assert_eq!((fill(s), stroke(s)), (Color::rgb(0, 255, 0), Color::rgb(0, 255, 0)));
}

#[test]
fn stroke_details_are_written_only_when_not_default() {
    let i = imp(&doc(
        r##"<rect width="1" height="1" stroke="#000" stroke-linecap="round" stroke-linejoin="bevel" stroke-miterlimit="2" stroke-dasharray="4 2" stroke-dashoffset="1" fill-rule="evenodd"/>
            <rect width="1" height="1" stroke="#000"/>"##,
    ));
    let s = &rect(&i.scene.elements[0]).style;
    assert_eq!(s.stroke_linecap, Some(msx_ast::LineCap::Round));
    assert_eq!(s.stroke_linejoin, Some(msx_ast::LineJoin::Bevel));
    assert_eq!(s.stroke_miterlimit, Some(2.0));
    assert_eq!(s.stroke_dasharray, Some(vec![4.0, 2.0]));
    assert_eq!(s.stroke_dashoffset, Some(1.0));
    assert_eq!(s.fill_rule, Some(msx_ast::FillRule::EvenOdd));
    let plain = &rect(&i.scene.elements[1]).style;
    assert!(plain.stroke_linecap.is_none() && plain.stroke_linejoin.is_none() && plain.stroke_dasharray.is_none() && plain.fill_rule.is_none());
}

#[test]
fn rect_corner_radius_rules() {
    let i = imp(&doc(
        r#"<rect width="20" height="10" rx="3"/>
           <rect width="20" height="10" ry="4"/>
           <rect width="20" height="10" rx="50"/>
           <rect width="20" height="10"/>
           <rect width="0" height="10"/>"#,
    ));
    assert_eq!(i.scene.elements.len(), 4, "a zero-width rect is not drawn");
    let r = |n: usize| (rect(&i.scene.elements[n]).rx, rect(&i.scene.elements[n]).ry);
    assert_eq!(r(0), (Some(3.0), Some(3.0)));
    assert_eq!(r(1), (Some(4.0), Some(4.0)));
    assert_eq!(r(2), (Some(10.0), Some(5.0)), "radii clamp to half the size");
    assert_eq!(r(3), (None, None));
}

#[test]
fn polylines_polygons_and_odd_coordinates() {
    let i = imp(&doc(r#"<polygon points="0,0 10,0 5,10"/><polyline points="0 0 10 10 5"/><polyline points="1 1"/>"#));
    assert_eq!(i.scene.elements.len(), 2, "a one-point polyline is not drawn");
    let Element::Polygon(p) = &i.scene.elements[0] else { panic!("polygon") };
    assert!(p.closed && p.points.len() == 3);
    let Element::Polyline(l) = &i.scene.elements[1] else { panic!("polyline") };
    assert!(!l.closed && l.points.len() == 2);
    assert!(has_warning(&i, "odd number of coordinates"));
}

#[test]
fn paths_are_normalized_and_broken_paths_draw_up_to_the_error() {
    let i = imp(&doc(r#"<path d="M0.5.5L10 10"/><path d="M 0 0 L 10 10 L 5"/><path d="nonsense"/>"#));
    assert_eq!(i.scene.elements.len(), 2);
    let Element::Path(p) = &i.scene.elements[0] else { panic!("path") };
    assert_eq!(p.d_raw, "M 0.5 0.5 L 10 10");
    assert_eq!(p.commands.len(), 2);
    let Element::Path(q) = &i.scene.elements[1] else { panic!("path") };
    assert_eq!(q.commands.len(), 2);
    assert!(has_warning(&i, "path data error"));
}

// ── Visibility ───────────────────────────────────────────────────────────────

#[test]
fn display_none_and_visibility_hidden() {
    let i = imp(&doc(
        r#"<rect display="none" width="1" height="1"/>
           <g style="display:none"><rect width="1" height="1"/></g>
           <rect visibility="hidden" width="1" height="1"/>
           <g visibility="hidden"><rect id="shown" visibility="visible" width="1" height="1"/></g>"#,
    ));
    assert_eq!(i.scene.elements.len(), 1);
    assert_eq!(rect(&i.scene.elements[0]).id.as_deref(), Some("shown"));
}

#[test]
fn foreign_namespaces_and_metadata_are_ignored() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" width="10" height="10">
        <sodipodi:namedview inkscape:zoom="2"/><metadata><x/></metadata><title>t</title><desc>d</desc>
        <rect width="1" height="1"/></svg>"##;
    let i = imp(svg);
    assert_eq!(i.scene.elements.len(), 1);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
}

#[test]
fn unsupported_features_are_reported() {
    let i = imp(&doc(
        r##"<rect width="1" height="1" clip-path="url(#c)"/>
            <rect width="1" height="1" filter="url(#f)"/>
            <rect width="1" height="1" mask="url(#m)"/>
            <foreignObject width="5" height="5"/>
            <pattern id="p"/><rect width="1" height="1" fill="url(#p)"/>"##,
    ));
    for needle in ["clip path", "filter", "mask", "<foreignObject", "pattern"] {
        assert!(has_warning(&i, needle), "missing a warning about {needle}: {:?}", i.warnings);
    }
    assert_eq!(i.scene.elements.len(), 4, "shapes are still imported, the foreignObject is not");
}

// ── use, symbol, nested svg ──────────────────────────────────────────────────

#[test]
fn use_is_expanded_with_the_style_of_the_use() {
    let i = imp(&doc(
        r##"<defs><rect id="r" width="10" height="10"/></defs>
            <use href="#r" x="20" fill="#ff0000"/>
            <rect id="r2" width="1" height="1"/><use xlink:href="#r2" y="5"/>"##,
    ));
    let els = &i.scene.elements;
    let Element::Group(g) = &els[0] else { panic!("an expanded use is a group") };
    assert!(matches!(g.transform, Some(Transform::Translate { x, y }) if x == 20.0 && y == 0.0));
    let inner = rect(&g.children[0]);
    assert_eq!(fill(&inner.style), Color::rgb(255, 0, 0), "the instance inherits from the use, not from where it was defined");
    assert!(inner.id.is_none(), "ids are not copied onto instances");
    assert_eq!(rect(&els[1]).id.as_deref(), Some("r2"), "the original keeps its id");
    let Element::Group(g2) = &els[2] else { panic!("group") };
    assert!(rect(&g2.children[0]).id.is_none());
}

#[test]
fn use_of_a_symbol_applies_its_viewbox() {
    let i = imp(&doc(r##"<symbol id="s" viewBox="0 0 10 10"><rect width="10" height="10"/></symbol><use href="#s" x="5" width="50" height="50"/>"##));
    let Element::Group(outer) = &i.scene.elements[0] else { panic!("group") };
    assert!(matches!(outer.transform, Some(Transform::Translate { x, .. }) if x == 5.0));
    let Element::Group(inner) = &outer.children[0] else { panic!("the symbol viewBox group") };
    assert!(matches!(inner.transform, Some(Transform::Scale { x, y }) if x == 5.0 && y == 5.0));
}

#[test]
fn a_use_that_refers_to_itself_is_skipped_and_the_rest_still_draws() {
    let i = imp(&doc(r##"<g id="g"><use href="#g"/><rect width="1" height="1"/></g>"##));
    assert!(has_warning(&i, "refers back to itself"));
    assert_eq!(i.scene.elements.len(), 1, "the rect is drawn once, not once per level of recursion");
}

#[test]
fn indirectly_recursive_uses_terminate() {
    let i = imp(&doc(
        r##"<g id="a"><use href="#b"/><rect width="1" height="1"/></g>
            <g id="b"><use href="#a"/><use href="#a"/><rect width="2" height="2"/></g>
            <use href="#a"/>"##,
    ));
    assert!(has_warning(&i, "refers back to itself"));
    assert!(i.scene.elements.len() < 50, "got {}", i.scene.elements.len());
}

#[test]
fn an_exponential_chain_of_uses_is_cut_off() {
    // 12 uses per level over 6 levels would be about 3 million shapes.
    let mut body = String::from(r#"<g id="l0"><rect width="1" height="1"/></g>"#);
    for level in 1..=6 {
        let uses: String = (0..12).map(|_| format!(r##"<use href="#l{}"/>"##, level - 1)).collect();
        body.push_str(&format!(r#"<g id="l{level}">{uses}</g>"#));
    }
    body.push_str(r##"<use href="#l6"/>"##);
    let started = std::time::Instant::now();
    let i = imp(&doc(&body));
    assert!(has_warning(&i, "more than"), "{:?}", i.warnings);
    assert!(started.elapsed().as_secs() < 20, "the cutoff must keep this fast, took {:?}", started.elapsed());
    fn count(els: &[Element]) -> usize {
        els.iter()
            .map(|e| match e {
                Element::Group(g) => count(&g.children),
                Element::Layer(l) => count(&l.children),
                _ => 1,
            })
            .sum()
    }
    assert!(count(&i.scene.elements) <= 200_001);
}

#[test]
fn a_use_of_a_missing_target_is_reported() {
    let i = imp(&doc(r##"<use href="#nothing"/>"##));
    assert!(i.scene.elements.is_empty());
    assert!(has_warning(&i, "#nothing"));
}

#[test]
fn nested_svg_and_switch() {
    let i = imp(&doc(
        r#"<svg x="10" y="20" width="50" height="50" viewBox="0 0 5 5"><rect width="5" height="5"/></svg>
           <switch><rect id="a" width="1" height="1"/><rect id="b" width="1" height="1"/></switch>"#,
    ));
    let Element::Group(g) = &i.scene.elements[0] else { panic!("nested svg group") };
    let Some(Transform::Multiple(t)) = &g.transform else { panic!("translate then scale") };
    assert!(matches!(t[0], Transform::Translate { x, y } if x == 10.0 && y == 20.0));
    assert!(matches!(t[1], Transform::Scale { x, y } if x == 10.0 && y == 10.0));
    assert!(!has_warning(&i, "clipping"), "content that fits needs no clip: {:?}", i.warnings);
    assert_eq!(i.scene.elements.len(), 2, "switch draws only its first child");
    assert_eq!(rect(&i.scene.elements[1]).id.as_deref(), Some("a"));
}

// ── Canvas and viewBox ───────────────────────────────────────────────────────

fn root(attrs: &str) -> Imported {
    imp(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attrs}><rect width="1" height="1"/></svg>"#))
}

#[test]
fn canvas_size_rules() {
    let c = |i: &Imported| (i.scene.canvas.width, i.scene.canvas.height);
    assert_eq!(c(&root(r#"width="48" height="48" viewBox="0 0 24 24""#)), (48.0, 48.0));
    assert_eq!(c(&root(r#"viewBox="0 0 24 12""#)), (24.0, 12.0));
    assert_eq!(c(&root(r#"width="48" viewBox="0 0 24 12""#)), (48.0, 24.0), "a missing height follows the viewBox aspect");
    assert_eq!(c(&root(r#"height="10" viewBox="0 0 20 10""#)), (20.0, 10.0));
    assert_eq!(c(&root(r#"width="1in" height="72pt""#)), (96.0, 96.0));
    let none = root("");
    assert_eq!(c(&none), (300.0, 150.0));
    assert!(has_warning(&none, "300 by 150"));
    assert_eq!(root(r#"width="100%" height="100%" viewBox="0 0 8 8""#).scene.canvas.width, 8.0);
}

#[test]
fn background_is_transparent_and_no_viewbox_block_is_written() {
    let i = root(r#"width="10" height="10" viewBox="0 0 5 5""#);
    assert_eq!(i.scene.canvas.background, Color::rgba(0, 0, 0, 0));
    assert!(i.scene.canvas.viewbox.is_none(), "the mapping is baked into a transform instead");
}

#[test]
fn viewbox_is_baked_into_one_wrapper_transform() {
    let i = root(r#"width="48" height="48" viewBox="0 0 24 24""#);
    let Element::Group(g) = &i.scene.elements[0] else { panic!("wrapper group") };
    assert!(matches!(g.transform, Some(Transform::Scale { x, y }) if x == 2.0 && y == 2.0));
    assert_eq!(root(r#"width="24" height="24" viewBox="0 0 24 24""#).scene.elements.len(), 1);
    assert!(matches!(root(r#"width="24" height="24" viewBox="0 0 24 24""#).scene.elements[0], Element::Rect(_)), "an identity mapping adds no wrapper");
}

#[test]
fn preserve_aspect_ratio_alignment_and_slice() {
    // 10x10 box into 100x50: meet scales by 5 and centers horizontally.
    let meet = root(r#"width="100" height="50" viewBox="0 0 10 10""#);
    let Element::Group(g) = &meet.scene.elements[0] else { panic!("group") };
    let Some(Transform::Multiple(t)) = &g.transform else { panic!("translate and scale") };
    assert!(matches!(t[0], Transform::Translate { x, y } if close(x, 25.0) && close(y, 0.0)));
    assert!(matches!(t[1], Transform::Scale { x, .. } if close(x, 5.0)));

    let slice = root(r#"width="100" height="50" viewBox="0 0 10 10" preserveAspectRatio="xMinYMin slice""#);
    let Element::Group(g) = &slice.scene.elements[0] else { panic!("group") };
    assert!(matches!(g.transform, Some(Transform::Scale { x, .. }) if close(x, 10.0)));

    let none = root(r#"width="100" height="50" viewBox="0 0 10 10" preserveAspectRatio="none""#);
    let Element::Group(g) = &none.scene.elements[0] else { panic!("group") };
    assert!(matches!(g.transform, Some(Transform::Scale { x, y }) if close(x, 10.0) && close(y, 5.0)));

    let offset = root(r#"width="10" height="10" viewBox="10 20 10 10""#);
    let Element::Group(g) = &offset.scene.elements[0] else { panic!("group") };
    assert!(matches!(g.transform, Some(Transform::Translate { x, y }) if close(x, -10.0) && close(y, -20.0)));
}

#[test]
fn percent_lengths_use_the_viewbox() {
    let i = imp(r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 50 50"><rect width="50%" height="20%"/></svg>"#);
    let Element::Group(g) = &i.scene.elements[0] else { panic!("wrapper") };
    let r = rect(&g.children[0]);
    assert_eq!((r.width, r.height), (25.0, 10.0));
}

// ── Gradients ────────────────────────────────────────────────────────────────

fn linear(i: &Imported, n: usize) -> msx_ast::LinearGradient {
    match &i.scene.defs[n] {
        Def::LinearGradient(g) => g.clone(),
        other => panic!("expected a linear gradient, got {other:?}"),
    }
}

#[test]
fn bounding_box_gradient_is_kept_as_is() {
    let i = imp(&doc(
        r##"<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#f00"/><stop offset="100%" stop-color="#00f"/></linearGradient></defs>
            <rect width="10" height="10" fill="url(#g)"/>"##,
    ));
    assert_eq!(i.scene.defs.len(), 1);
    let g = linear(&i, 0);
    assert_eq!(g.id, "g");
    assert_eq!((g.x1, g.y1, g.x2, g.y2), (0.0, 0.0, 1.0, 0.0));
    assert_eq!(g.stops.len(), 2);
    assert_eq!(g.stops[1].offset, 1.0);
    assert_eq!(rect(&i.scene.elements[0]).style.fill, Some(Paint::Ref("url(#g)".into())));
}

#[test]
fn user_space_gradient_is_converted_to_the_shapes_unit_square() {
    // t = x / 100 in user space. The rect spans x = 50..150, so across the
    // rect t runs 0.5 to 1.5, which in unit coordinates is u + 0.5.
    let i = imp(&doc(
        r##"<defs><linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100" y2="0"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <rect x="50" y="0" width="100" height="50" fill="url(#g)"/>"##,
    ));
    let g = linear(&i, 0);
    assert!(close(g.x1, -0.5) && close(g.y1, 0.0) && close(g.x2, 0.5) && close(g.y2, 0.0), "got {g:?}");
}

#[test]
fn gradient_transform_is_applied() {
    // rotate(90) turns an x-direction gradient into a y-direction one.
    let i = imp(&doc(
        r##"<defs><linearGradient id="g" gradientTransform="rotate(90)" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <rect width="10" height="10" fill="url(#g)"/>"##,
    ));
    let g = linear(&i, 0);
    assert!(close(g.x1, 0.0) && close(g.y1, 0.0) && close(g.x2, 0.0) && close(g.y2, 1.0), "got {g:?}");
}

#[test]
fn gradient_inherits_stops_and_attributes_through_href() {
    let i = imp(&doc(
        r##"<defs>
              <linearGradient id="base"><stop offset="0" style="stop-color:#f00;stop-opacity:.5"/><stop offset="1" stop-color="#00f"/></linearGradient>
              <linearGradient id="g" href="#base" x2="0" y2="1"/>
            </defs>
            <rect width="10" height="10" fill="url(#g)"/>"##,
    ));
    let g = linear(&i, 0);
    assert_eq!((g.x1, g.y1, g.x2, g.y2), (0.0, 0.0, 0.0, 1.0));
    assert_eq!(g.stops[0].color, Color::rgba(255, 0, 0, 128));
}

#[test]
fn fill_opacity_is_folded_into_gradient_stops_and_variants_get_their_own_def() {
    let i = imp(&doc(
        r##"<defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <rect width="10" height="10" fill="url(#g)"/>
            <rect width="10" height="10" fill="url(#g)" fill-opacity="0.5"/>"##,
    ));
    assert_eq!(i.scene.defs.len(), 2);
    let (a, b) = (linear(&i, 0), linear(&i, 1));
    assert_ne!(a.id, b.id);
    assert_eq!(a.stops[0].color.a, 255);
    assert_eq!(b.stops[0].color.a, 128);
}

#[test]
fn shapes_that_share_a_gradient_and_box_share_one_def() {
    let i = imp(&doc(
        r##"<defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <rect width="10" height="10" fill="url(#g)"/><rect x="50" width="10" height="10" fill="url(#g)"/>"##,
    ));
    assert_eq!(i.scene.defs.len(), 2, "different boxes need different user-space conversions");
    let i = imp(&doc(
        r##"<defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <rect width="10" height="10" fill="url(#g)"/><rect width="10" height="10" fill="url(#g)" stroke="url(#g)"/>"##,
    ));
    assert_eq!(i.scene.defs.len(), 1, "same gradient, same opacity, same box: one def");
}

#[test]
fn gradient_with_one_stop_is_a_solid_color_and_with_none_paints_nothing() {
    let i = imp(&doc(
        r##"<defs><linearGradient id="one"><stop offset="0" stop-color="#0f0"/></linearGradient><linearGradient id="zero"/></defs>
            <rect width="1" height="1" fill="url(#one)"/><rect width="1" height="1" fill="url(#zero)"/>"##,
    ));
    assert!(i.scene.defs.is_empty());
    assert_eq!(fill(&rect(&i.scene.elements[0]).style), Color::rgb(0, 255, 0));
    assert_eq!(rect(&i.scene.elements[1]).style.fill, Some(Paint::None));
}

#[test]
fn missing_gradient_uses_the_fallback_color_or_nothing() {
    let i = imp(&doc(r##"<rect width="1" height="1" fill="url(#nope) #123456"/><rect width="1" height="1" fill="url(#nope)"/>"##));
    assert_eq!(fill(&rect(&i.scene.elements[0]).style), Color::rgb(0x12, 0x34, 0x56));
    assert_eq!(rect(&i.scene.elements[1]).style.fill, Some(Paint::None));
    assert!(has_warning(&i, "#nope"));
}

#[test]
fn a_current_color_fallback_uses_the_color_property() {
    let i = imp(&doc(r##"<rect width="1" height="1" color="#008000" fill="url(#nothing) currentColor"/>"##));
    assert_eq!(fill(&rect(&i.scene.elements[0]).style), Color::rgb(0, 128, 0));
}

#[test]
fn radial_gradient_and_spread_method_warning() {
    let i = imp(&doc(
        r##"<defs><radialGradient id="r" cx="0.5" cy="0.5" r="0.5" fx="0.3" fy="0.3" spreadMethod="reflect"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient></defs>
            <circle cx="10" cy="10" r="5" fill="url(#r)"/>"##,
    ));
    let Def::RadialGradient(g) = &i.scene.defs[0] else { panic!("radial") };
    assert_eq!((g.cx, g.cy, g.r, g.fx, g.fy), (0.5, 0.5, 0.5, 0.3, 0.3));
    assert!(has_warning(&i, "spreadMethod"));
}

#[test]
fn user_space_radial_under_uniform_scale_is_exact_and_stretched_is_flagged() {
    // A circle of radius 25 around (50, 50) on a 100x100 box is the unit
    // circle of radius 0.25 around (0.5, 0.5).
    let exact = imp(&doc(
        r##"<defs><radialGradient id="r" gradientUnits="userSpaceOnUse" cx="50" cy="50" r="25"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient></defs>
            <rect width="100" height="100" fill="url(#r)"/>"##,
    ));
    let Def::RadialGradient(g) = &exact.scene.defs[0] else { panic!("radial") };
    assert!(close(g.cx, 0.5) && close(g.cy, 0.5) && close(g.r, 0.25), "got {g:?}");
    assert!(exact.warnings.is_empty());

    // The same gradient on a 100x50 box is an ellipse in unit space.
    let stretched = imp(&doc(
        r##"<defs><radialGradient id="r" gradientUnits="userSpaceOnUse" cx="50" cy="25" r="25"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient></defs>
            <rect width="100" height="50" fill="url(#r)"/>"##,
    ));
    assert!(has_warning(&stretched, "approximated"));
}

#[test]
fn user_space_gradient_on_a_straight_line_becomes_its_average_color() {
    let i = imp(&doc(
        r##"<defs><linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" x2="100"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient></defs>
            <line x1="0" y1="10" x2="100" y2="10" stroke="url(#g)" stroke-width="4"/>"##,
    ));
    assert!(i.scene.defs.is_empty());
    assert!(has_warning(&i, "empty bounding box"));
    let Element::Line(l) = &i.scene.elements[0] else { panic!("line") };
    assert_eq!(stroke(&l.style), Color::rgb(128, 128, 128));
}

// ── Text ─────────────────────────────────────────────────────────────────────

fn text(e: &Element) -> &Text {
    match e {
        Element::Text(t) => t,
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn text_collapses_whitespace_and_carries_font_properties() {
    let i = imp(&doc(
        "<text x=\"10\" y=\"20\" font-size=\"12\" font-weight=\"bold\" font-family=\"Arial, sans-serif\" text-anchor=\"middle\" fill=\"#112233\">  Hello   <tspan>wide</tspan>\n world </text>",
    ));
    let t = text(&i.scene.elements[0]);
    assert_eq!(t.content, "Hello wide world");
    assert_eq!((t.x, t.y), (10.0, 20.0));
    assert_eq!(t.style.font_size, Some(12.0));
    assert_eq!(t.style.font_weight, Some(FontWeight::Bold));
    assert_eq!(t.style.text_anchor, Some(TextAnchor::Middle));
    assert_eq!(t.style.font_family.as_deref(), Some("Arial, sans-serif"));
    assert_eq!(fill(&t.style), Color::rgb(0x11, 0x22, 0x33));
}

#[test]
fn positioned_tspans_become_separate_text_elements() {
    let i = imp(&doc(r#"<text font-size="10"><tspan x="5" y="10">Line one</tspan><tspan x="5" y="22">Line two</tspan></text>"#));
    assert_eq!(i.scene.elements.len(), 2);
    let (a, b) = (text(&i.scene.elements[0]), text(&i.scene.elements[1]));
    assert_eq!((a.content.as_str(), a.x, a.y), ("Line one", 5.0, 10.0));
    assert_eq!((b.content.as_str(), b.x, b.y), ("Line two", 5.0, 22.0));
}

#[test]
fn text_dx_dy_and_empty_text() {
    let i = imp(&doc(r#"<text x="10" y="20" dx="2" dy="-3">Hi</text><text x="1" y="1">   </text>"#));
    assert_eq!(i.scene.elements.len(), 1);
    let t = text(&i.scene.elements[0]);
    assert_eq!((t.x, t.y), (12.0, 17.0));
}

#[test]
fn text_with_a_gradient_gets_the_text_box_as_its_bounding_box() {
    let i = imp(&doc(
        r##"<defs><linearGradient id="g" gradientUnits="userSpaceOnUse" x1="10" y1="0" x2="110" y2="0"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <text x="10" y="50" font-size="20" fill="url(#g)">Gradient</text>"##,
    ));
    let g = linear(&i, 0);
    // The text starts at x = 10, so its box starts at the gradient start: t = 0 at u = 0.
    assert!(close(g.x1, 0.0), "gradient start should sit at the left edge of the text box, got {}", g.x1);
    assert!(g.x2 > 0.0 && g.y1 == g.y2);
}

#[test]
fn text_path_is_reported() {
    let i = imp(&doc(r##"<text><textPath href="#p">Curvy</textPath></text>"##));
    assert!(has_warning(&i, "text on a path"));
    assert!(i.scene.elements.is_empty());
}

// ── Errors and round trip ────────────────────────────────────────────────────

#[test]
fn errors_for_bad_input() {
    assert!(matches!(import_svg("not xml at all"), Err(ImportError::Xml(_))));
    assert!(matches!(import_svg("<html/>"), Err(ImportError::NotSvg(_))));
    assert!(import_svg("<svg xmlns=\"http://www.w3.org/2000/svg\"/>").is_ok());
}

#[test]
fn a_byte_order_mark_and_leading_blank_lines_are_accepted() {
    let svg = "\u{feff}\n\n  <?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"5\" height=\"5\"><rect width=\"1\" height=\"1\"/></svg>";
    assert_eq!(imp(svg).scene.elements.len(), 1);
}

#[test]
fn a_doctype_is_accepted() {
    let svg = "<?xml version=\"1.0\"?><!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\"><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"5\" height=\"5\"><rect width=\"1\" height=\"1\"/></svg>";
    assert_eq!(imp(svg).scene.elements.len(), 1);
}

#[test]
fn imported_scenes_survive_write_and_parse() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="120" viewBox="0 0 100 60">
        <style>.t { font-family: "Fira Code", monospace; fill: #223344 }</style>
        <defs>
          <linearGradient id="lg" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100" y2="60" gradientTransform="rotate(10)"><stop offset="0" stop-color="#f00"/><stop offset=".5" stop-color="#0f0" stop-opacity=".5"/><stop offset="1" stop-color="#00f"/></linearGradient>
          <radialGradient id="rg"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient>
          <path id="shape" d="M0.5.5l10 0 0 10z"/>
        </defs>
        <g opacity=".8" transform="translate(5 5)">
          <rect x="1" y="1" width="30" height="20" rx="3" fill="url(#lg)" stroke="#123" stroke-width="2" stroke-dasharray="4 2" stroke-linecap="round"/>
          <circle cx="50" cy="30" r="10" fill="url(#rg)" fill-opacity=".6"/>
          <use href="#shape" x="60" y="5" fill="#f80"/>
        </g>
        <text class="t" x="50" y="55" font-size="8" text-anchor="middle">Quote " backslash \ brace {x} // ok</text>
        <polygon points="0,0 10,0 5,8" fill-rule="evenodd" fill="#abc"/>
      </svg>"##;
    let i = imp(svg);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let text = msx_parser::scene_to_msx(&i.scene).expect("an imported scene is writable");
    let back = msx_parser::parse_scene(&text).unwrap_or_else(|e| panic!("written source must parse: {e}\n{text}"));
    assert_eq!(format!("{:?}", i.scene.canvas), format!("{:?}", back.canvas));
    assert_eq!(format!("{:?}", i.scene.defs), format!("{:?}", back.defs));
    assert_eq!(format!("{:?}", i.scene.elements), format!("{:?}", back.elements), "{text}");
}

// ── Conditional processing ───────────────────────────────────────────────────

#[test]
fn switch_picks_the_first_child_whose_conditions_hold() {
    let i = imp(&doc(
        r#"<switch>
             <rect id="ext" width="1" height="1" requiredExtensions="http://example.org/bogus"/>
             <rect id="ru" width="1" height="1" systemLanguage="ru"/>
             <rect id="en" width="1" height="1" systemLanguage="de, en-US"/>
             <rect id="last" width="1" height="1"/>
           </switch>"#,
    ));
    assert_eq!(i.scene.elements.len(), 1);
    assert_eq!(rect(&i.scene.elements[0]).id.as_deref(), Some("en"));
}

#[test]
fn system_language_applies_outside_switch_too() {
    let i = imp(&doc(
        r#"<rect id="a" width="1" height="1" systemLanguage="fr"/><rect id="b" width="1" height="1" systemLanguage="en"/>
           <g systemLanguage="ja"><rect id="c" width="1" height="1"/></g>"#,
    ));
    let ids: Vec<_> = i.scene.elements.iter().map(|e| rect(e).id.clone().unwrap()).collect();
    assert_eq!(ids, vec!["b".to_string()]);
}

// ── Stop colors ──────────────────────────────────────────────────────────────

#[test]
fn stop_color_current_color_and_inherit_follow_the_tree() {
    let i = imp(&doc(
        r##"<defs>
              <linearGradient id="g" color="#00ff00" stop-color="#ff0000">
                <stop offset="0" stop-color="currentColor"/>
                <stop offset="0.5" stop-color="inherit"/>
                <stop offset="1" stop-color="#0000ff" stop-opacity="inherit"/>
              </linearGradient>
            </defs>
            <rect width="10" height="10" fill="url(#g)"/>"##,
    ));
    let g = linear(&i, 0);
    assert_eq!(g.stops[0].color, Color::rgb(0, 255, 0), "currentColor uses the color set on the gradient");
    assert_eq!(g.stops[1].color, Color::rgb(255, 0, 0), "inherit takes the gradient's stop-color");
}

// ── Painting order and compositing ───────────────────────────────────────────

#[test]
fn paint_order_stroke_draws_the_stroke_first() {
    let i = imp(&doc(
        r##"<g paint-order="stroke"><rect id="r" width="10" height="10" fill="#ff0000" stroke="#0000ff" stroke-width="4"/></g>
            <rect width="10" height="10" fill="#ff0000" stroke="#0000ff" stroke-width="4"/>
            <rect width="10" height="10" fill="#ff0000" paint-order="stroke"/>"##,
    ));
    let els = &i.scene.elements;
    assert_eq!(els.len(), 4, "one shape split in two, then two plain shapes");
    let (first, second) = (rect(&els[0]), rect(&els[1]));
    assert_eq!((first.style.fill.clone(), stroke(&first.style)), (Some(Paint::None), Color::rgb(0, 0, 255)));
    assert_eq!((fill(&second.style), second.style.stroke.clone()), (Color::rgb(255, 0, 0), Some(Paint::None)));
    assert_eq!(first.id.as_deref(), Some("r"));
    assert!(second.id.is_none(), "the id stays on the first half");
}

#[test]
fn mix_blend_mode_becomes_a_layer_blend_and_isolation_an_isolated_layer() {
    let i = imp(&doc(
        r#"<rect width="1" height="1" style="mix-blend-mode:multiply"/>
           <g style="isolation:isolate"><rect width="1" height="1"/></g>
           <rect width="1" height="1" style="mix-blend-mode:hue"/>"#,
    ));
    let Element::Layer(l) = &i.scene.elements[0] else { panic!("a blended shape is wrapped in a layer") };
    assert_eq!(l.blend_mode, msx_ast::BlendMode::Multiply);
    assert_eq!(l.children.len(), 1);
    let Element::Layer(iso) = &i.scene.elements[1] else { panic!("isolation creates a layer") };
    assert_eq!((iso.opacity, iso.blend_mode), (1.0, msx_ast::BlendMode::Normal));
    assert!(matches!(i.scene.elements[2], Element::Rect(_)), "an unsupported blend mode draws normally");
    assert!(has_warning(&i, "mix-blend-mode: hue"));
}

#[test]
fn blend_mode_and_isolation_are_css_only_not_attributes() {
    let i = imp(&doc(r#"<rect width="1" height="1" mix-blend-mode="multiply"/><g isolation="isolate"><rect width="1" height="1"/></g>"#));
    assert!(i.scene.elements.iter().all(|e| matches!(e, Element::Rect(_))), "as plain attributes they have no effect");
}

// ── Transforms ───────────────────────────────────────────────────────────────

#[test]
fn css_transform_and_transform_origin() {
    let i = imp(&doc(
        r#"<rect width="10" height="10" style="transform: rotate(90deg); transform-origin: 50px 20px"/>
           <rect width="10" height="10" transform="scale(2)" style="transform: translate(5px, 6px)"/>
           <rect width="10" height="10" transform="rotate(45)" transform-origin="center"/>"#,
    ));
    let Some(Transform::Multiple(t)) = &rect(&i.scene.elements[0]).transform else { panic!("origin chain") };
    assert!(matches!(t[0], Transform::Translate { x, y } if x == 50.0 && y == 20.0));
    assert!(matches!(t[1], Transform::Rotate { angle, .. } if angle == 90.0));
    assert!(matches!(t[2], Transform::Translate { x, y } if x == -50.0 && y == -20.0));
    assert!(matches!(rect(&i.scene.elements[1]).transform, Some(Transform::Translate { x, y }) if x == 5.0 && y == 6.0), "the CSS property beats the attribute");
    let Some(Transform::Multiple(c)) = &rect(&i.scene.elements[2]).transform else { panic!("centered rotation") };
    assert!(matches!(c[0], Transform::Translate { x, y } if x == 50.0 && y == 50.0), "center of the 100 by 100 viewport");
}

// ── Things that cannot be drawn ──────────────────────────────────────────────

#[test]
fn a_path_must_start_with_a_moveto() {
    let i = imp(&doc(r#"<path d="A 5 5 0 0 1 50 50"/><path d="L 1 1 L 2 2"/>"#));
    assert!(i.scene.elements.is_empty());
    assert!(has_warning(&i, "must start with a moveto"));
}

#[test]
fn unusable_markers_and_context_paint_are_reported() {
    let i = imp(&doc(
        r##"<path d="M0 0 L 5 5" marker-end="url(#m)" stroke="#000"/>
            <path d="M0 0 L 5 5" style="marker: url(#m)" stroke="#000"/>
            <rect width="1" height="1" fill="context-stroke"/>"##,
    ));
    assert!(has_warning(&i, "#m"), "{:?}", i.warnings);
    assert!(has_warning(&i, "does not exist"));
    assert!(has_warning(&i, "context-stroke"));
    assert_eq!(i.scene.elements.len(), 3, "the shapes themselves are still imported");
}

#[test]
fn root_transform_origin_percentages_use_the_viewport() {
    // 200 by 200 canvas over a 100 by 100 viewBox: 25% 75% is (50, 150) in
    // the canvas, so scale(0.5) about that point sends user (20, 20) to (45, 95).
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 100 100" transform="scale(0.5)" transform-origin="25% 75%"><rect x="20" y="20" width="60" height="60"/></svg>"#;
    let i = imp(svg);
    let Element::Group(g) = &i.scene.elements[0] else { panic!("the root wrapper group") };
    let m = g.transform.as_ref().expect("root transform").to_matrix();
    let (x, y) = geom::apply(m, 20.0, 20.0);
    assert!(close(x, 45.0) && close(y, 95.0), "got ({x}, {y})");
}

#[test]
fn ignored_text_and_stroke_properties_are_reported() {
    let i = imp(&doc(r#"<text x="0" y="10" style="text-decoration: underline; letter-spacing: 2px" font-style="italic">hi</text><path d="M0 0L9 9" vector-effect="non-scaling-stroke"/>"#));
    for key in ["text-decoration", "letter-spacing", "font-style", "vector-effect"] {
        assert!(has_warning(&i, key), "no warning for {key}: {:?}", i.warnings);
    }
    let plain = imp(&doc(r#"<text x="0" y="10" style="text-decoration: none; letter-spacing: normal" font-style="normal">hi</text>"#));
    assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);
}

#[test]
fn radial_gradient_features_msx_lacks_are_reported() {
    let i = imp(&doc(
        r##"<defs><radialGradient id="r" cx="0.2" cy="0.2" r="0.2" fx="0.9" fy="0.9" fr="0.1"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient></defs>
            <rect width="10" height="10" fill="url(#r)"/>"##,
    ));
    assert!(has_warning(&i, "fr"));
    assert!(has_warning(&i, "focal point outside"));
}

#[test]
fn bounding_box_gradient_on_an_empty_box_uses_the_fallback_color() {
    let i = imp(&doc(
        r##"<defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <line x1="0" y1="5" x2="10" y2="5" stroke="url(#g) #00ff00" stroke-width="2"/>
            <line x1="0" y1="9" x2="10" y2="9" stroke="url(#g)" stroke-width="2"/>"##,
    ));
    let Element::Line(a) = &i.scene.elements[0] else { panic!("line") };
    assert_eq!(stroke(&a.style), Color::rgb(0, 255, 0), "SVG paints the fallback when the box is empty");
    let Element::Line(b) = &i.scene.elements[1] else { panic!("line") };
    assert_eq!(b.style.stroke, Some(Paint::None), "and nothing when there is none");
    assert!(i.scene.defs.is_empty());
}

// ── clip-path, symbol and nested svg viewports, animation ────────────────────

fn layer(e: &Element) -> &Layer {
    match e {
        Element::Layer(l) => l,
        other => panic!("expected a layer, got {other:?}"),
    }
}

/// The geometry of a clip rect: `(x, y, width, height)`.
fn clip_rect(e: &Element) -> (f64, f64, f64, f64) {
    let r = rect(e);
    (r.x, r.y, r.width, r.height)
}

#[test]
fn clip_path_in_user_space_wraps_the_shape_in_a_clipped_layer() {
    let i = imp(&doc(r##"<defs><clipPath id="c"><rect x="10" y="10" width="30" height="30"/></clipPath></defs>
        <rect width="100" height="100" fill="red" clip-path="url(#c)"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert_eq!(i.scene.elements.len(), 1);
    let l = layer(&i.scene.elements[0]);
    let clip = l.clip_path.as_ref().expect("a clip path");
    assert_eq!(clip.len(), 1);
    assert_eq!(clip_rect(&clip[0]), (10.0, 10.0, 30.0, 30.0));
    assert_eq!(l.children.len(), 1);
    assert_eq!(rect(&l.children[0]).width, 100.0);
}

#[test]
fn clip_path_on_a_transformed_shape_moves_the_transform_to_the_layer() {
    let i = imp(&doc(r##"<defs><clipPath id="c"><rect width="10" height="10"/></clipPath></defs>
        <rect width="20" height="20" transform="translate(5 6)" clip-path="url(#c)"/>"##));
    let l = layer(&i.scene.elements[0]);
    assert!(matches!(l.transform, Some(Transform::Translate { x, y }) if x == 5.0 && y == 6.0), "{:?}", l.transform);
    assert!(rect(&l.children[0]).transform.is_none(), "the shape no longer carries it");
}

#[test]
fn clip_path_on_a_group_clips_the_whole_group() {
    let i = imp(&doc(r##"<defs><clipPath id="c"><circle cx="5" cy="5" r="5"/></clipPath></defs>
        <g clip-path="url(#c)" transform="translate(3 4)"><rect width="9" height="9"/><rect x="20" width="9" height="9"/></g>"##));
    let l = layer(&i.scene.elements[0]);
    assert!(l.clip_path.is_some());
    assert_eq!(l.children.len(), 2);
    assert!(l.transform.is_some());
}

#[test]
fn clip_path_object_bounding_box_units_map_the_unit_square_onto_the_shape() {
    let i = imp(&doc(r##"<defs><clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="0.5" height="1"/></clipPath></defs>
        <rect x="20" y="10" width="40" height="20" clip-path="url(#c)"/>"##));
    let l = layer(&i.scene.elements[0]);
    let clip = l.clip_path.as_ref().unwrap();
    let Element::Group(g) = &clip[0] else { panic!("the unit square group") };
    let m = g.transform.as_ref().expect("bbox transform").to_matrix();
    let (x, y) = geom::apply(m, 0.5, 1.0);
    assert!(close(x, 40.0) && close(y, 30.0), "got ({x}, {y})");
    let (x0, y0) = geom::apply(m, 0.0, 0.0);
    assert!(close(x0, 20.0) && close(y0, 10.0));
}

#[test]
fn clip_path_object_bounding_box_on_an_empty_box_clips_everything() {
    let i = imp(&doc(r##"<defs><clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="1" height="1"/></clipPath></defs>
        <g clip-path="url(#c)"><line x1="0" y1="0" x2="10" y2="0" stroke="red"/></g>"##));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(l.clip_path.as_ref().map(Vec::len), Some(0));
}

#[test]
fn clip_path_uses_clip_rule_and_ignores_paint() {
    let i = imp(&doc(r##"<defs><clipPath id="c" clip-rule="evenodd"><rect width="5" height="5" fill="none" stroke="red" stroke-width="3" opacity=".1" fill-rule="nonzero"/>
            <circle cx="9" cy="9" r="2" clip-rule="nonzero"/></clipPath></defs>
        <rect width="20" height="20" clip-path="url(#c)"/>"##));
    let clip = layer(&i.scene.elements[0]).clip_path.as_ref().unwrap();
    let r = rect(&clip[0]);
    assert_eq!(r.style.fill_rule, Some(msx_ast::FillRule::EvenOdd), "clip-rule is inherited from the clipPath");
    assert_eq!(r.style.stroke, Some(Paint::None));
    assert_eq!(r.style.opacity, Some(1.0));
    let Element::Circle(c) = &clip[1] else { panic!("circle") };
    assert_eq!(c.style.fill_rule, None, "nonzero is the default and stays unwritten");
}

#[test]
fn clip_path_only_takes_shapes_with_an_area_text_and_use_of_those() {
    let i = imp(&doc(r##"<defs><clipPath id="c">
            <line x1="0" y1="0" x2="9" y2="9"/><g><rect width="9" height="9"/></g><image href="a.png" width="3" height="3"/>
            <rect id="ok" width="4" height="4"/><rect width="4" height="4" visibility="hidden"/><rect width="4" height="4" display="none"/>
            <text x="1" y="9">A</text><use href="#ok" x="10"/></clipPath>
          <symbol id="sym"><rect width="2" height="2"/></symbol><clipPath id="d"><use href="#sym"/></clipPath></defs>
        <rect width="20" height="20" clip-path="url(#c)"/><rect width="20" height="20" clip-path="url(#d)"/>"##));
    let c = layer(&i.scene.elements[0]).clip_path.as_ref().unwrap();
    assert_eq!(c.len(), 3, "rect, text and the use of the rect: {c:?}");
    assert!(matches!(c[1], Element::Text(_)));
    let d = layer(&i.scene.elements[1]).clip_path.as_ref().unwrap();
    assert!(d.is_empty(), "a use of a symbol is not a valid child");
}

#[test]
fn clip_path_on_a_clip_path_nests_a_second_layer() {
    let i = imp(&doc(r##"<defs><clipPath id="a" clip-path="url(#b)"><rect width="30" height="30"/></clipPath>
        <clipPath id="b"><rect width="10" height="40"/></clipPath></defs>
        <rect width="50" height="50" clip-path="url(#a)"/>"##));
    let outer = layer(&i.scene.elements[0]);
    assert_eq!(clip_rect(&outer.clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 30.0, 30.0));
    let inner = layer(&outer.children[0]);
    assert_eq!(clip_rect(&inner.clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 10.0, 40.0));
    assert_eq!(rect(&inner.children[0]).width, 50.0);
}

#[test]
fn unusable_clip_references_are_reported_and_draw_unclipped() {
    let i = imp(&doc(r##"<defs><clipPath id="self" clip-path="url(#self)"><rect width="5" height="5"/></clipPath><rect id="notclip" width="1" height="1"/></defs>
        <rect width="9" height="9" clip-path="url(#missing)"/>
        <rect width="9" height="9" clip-path="url(#notclip)"/>
        <rect width="9" height="9" clip-path="circle(5px)"/>
        <rect width="9" height="9" clip-path="url(#self)"/>"##));
    for needle in ["#missing", "#notclip", "circle(5px)", "refers back to itself"] {
        assert!(has_warning(&i, needle), "no warning for {needle}: {:?}", i.warnings);
    }
    assert!(i.scene.elements.iter().any(|e| matches!(e, Element::Rect(_))), "the unclipped rects are still drawn");
}

#[test]
fn symbol_viewport_clips_content_that_overflows_it() {
    // viewBox 0 0 10 10 scaled to the 100 by 100 use: the viewport is (0, 0, 10, 10) in symbol space.
    let i = imp(&doc(r##"<symbol id="s" viewBox="0 0 10 10"><rect width="20" height="20"/></symbol><use href="#s" width="100" height="100"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let l = layer(&i.scene.elements[0]);
    assert_eq!(clip_rect(&l.clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 10.0, 10.0));
    assert!(matches!(l.transform, Some(Transform::Scale { x, y }) if x == 10.0 && y == 10.0), "{:?}", l.transform);
}

#[test]
fn symbol_without_a_viewbox_clips_to_the_use_viewport() {
    let i = imp(&doc(r##"<symbol id="s"><rect x="-20" y="-10" width="120" height="120"/></symbol><use href="#s"/>"##));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(clip_rect(&l.clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 100.0, 100.0));
}

#[test]
fn viewport_clip_is_skipped_when_the_content_fits_or_overflow_is_visible() {
    let fits = imp(&doc(r##"<symbol id="s" viewBox="0 0 24 24"><rect x="2" y="2" width="20" height="20"/></symbol><use href="#s" width="48" height="48"/>"##));
    assert!(matches!(fits.scene.elements[0], Element::Group(_)), "no layer for content that fits: {:?}", fits.scene.elements[0]);
    let visible = imp(&doc(r##"<symbol id="s" overflow="visible" viewBox="0 0 10 10"><rect width="20" height="20"/></symbol><use href="#s" width="100" height="100"/>"##));
    assert!(matches!(visible.scene.elements[0], Element::Group(_)));
    let auto = imp(&doc(r##"<defs><svg id="n" width="10" height="10" style="overflow:auto"><circle cx="5" cy="5" r="30"/></svg></defs><use href="#n"/>"##));
    assert!(!matches!(auto.scene.elements[0], Element::Layer(_)));
}

#[test]
fn stroke_poking_over_the_viewport_edge_is_clipped() {
    let i = imp(&doc(r##"<symbol id="s" viewBox="0 0 10 10"><rect width="10" height="10" fill="none" stroke="red" stroke-width="4"/></symbol><use href="#s" width="10" height="10"/>"##));
    assert!(matches!(i.scene.elements[0], Element::Layer(_)), "the outline reaches 2 units past the edge");
}

#[test]
fn text_inside_a_symbol_is_clipped_because_its_extent_is_unknown() {
    let i = imp(&doc(r##"<symbol id="s" viewBox="0 0 10 10"><text x="1" y="5">Hello there</text></symbol><use href="#s" width="10" height="10"/>"##));
    assert!(matches!(i.scene.elements[0], Element::Layer(_)));
}

#[test]
fn nested_svg_viewport_clips_through_a_use_size() {
    let i = imp(&doc(r##"<defs><svg id="s"><circle cx="10" cy="10" r="30"/></svg></defs><use href="#s" width="20" height="20"/>"##));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(clip_rect(&l.clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 20.0, 20.0));
    let plain = imp(&doc(r##"<svg x="5" y="5" width="30" height="30"><circle cx="10" cy="10" r="5"/></svg>"##));
    assert!(!matches!(plain.scene.elements[0], Element::Layer(_)));
}

#[test]
fn animations_without_a_channel_are_reported_instead_of_dropped_silently() {
    let i = imp(&doc(r##"<style>@keyframes spin { to { fill: red } } .a { animation: spin 2s infinite }</style>
        <rect width="5" height="5"><animate attributeName="fill" from="red" to="blue" dur="1s"/></rect>
        <circle class="a" r="3"/><g><animateTransform attributeName="transform" type="skewX" to="30" dur="1s"/><rect width="1" height="1"/></g>
        <rect width="5" height="5"><set attributeName="fill" to="red"/></rect>"##));
    for needle in ["keyframe property fill", "<animate>", "CSS animation", "<animateTransform>", "<set>"] {
        assert!(has_warning(&i, needle), "no warning for {needle}: {:?}", i.warnings);
    }
    assert!(i.scene.elements.len() >= 3, "the static frame is still drawn");
    assert!(i.scene.animations.is_empty() && i.scene.duration == 0.0);
}

#[test]
fn clipped_imports_survive_write_and_parse() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60">
        <defs><clipPath id="c" clip-rule="evenodd"><path d="M0 0H40V40H0Z M10 10H30V30H10Z"/><circle cx="60" cy="30" r="9"/></clipPath>
        <clipPath id="o" clipPathUnits="objectBoundingBox"><rect width=".5" height="1"/></clipPath></defs>
        <rect width="80" height="60" fill="#f00" clip-path="url(#c)"/>
        <g clip-path="url(#o)" transform="translate(5 5)"><rect width="40" height="20" fill="#00f"/></g></svg>"##;
    let i = imp(svg);
    let text = msx_parser::scene_to_msx(&i.scene).expect("written");
    let back = msx_parser::parse_scene(&text).expect("parsed");
    assert_eq!(format!("{:?}", i.scene.elements), format!("{:?}", back.elements));
}

#[test]
fn use_that_sets_only_a_width_keeps_the_nested_svg_height() {
    let i = imp(&doc(r##"<defs><svg id="s" x="40" y="40" width="80" height="80"><circle cx="100" cy="100" r="120"/></svg></defs><use href="#s" width="100"/>"##));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(clip_rect(&l.clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 100.0, 80.0));
}

#[test]
fn symbol_viewport_follows_a_view_box_that_does_not_start_at_the_origin() {
    // The viewBox covers -100..100, so content past 100 is clipped and 20..100 stays.
    let i = imp(&doc(r##"<symbol id="s" viewBox="-100 -100 200 200"><rect x="20" y="20" width="160" height="160"/></symbol><use href="#s" width="100" height="100"/>"##));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(clip_rect(&l.clip_path.as_ref().unwrap()[0]), (-100.0, -100.0, 200.0, 200.0));
}

#[test]
fn hidden_shapes_still_count_toward_an_object_bounding_box_clip() {
    let svg = r##"<clipPath id="c" clipPathUnits="objectBoundingBox"><circle cx=".5" cy=".5" r=".45"/></clipPath>
        <g clip-path="url(#c)"><rect x="20" y="20" width="160" height="160" fill="red" visibility="hidden"/><rect x="40" y="40" width="120" height="120" fill="green"/></g>"##;
    let i = imp(&doc(svg));
    let l = layer(&i.scene.elements[0]);
    let Element::Group(g) = &l.clip_path.as_ref().unwrap()[0] else { panic!("bbox group") };
    let m = g.transform.as_ref().unwrap().to_matrix();
    let (x1, y1) = geom::apply(m, 1.0, 1.0);
    assert!(close(x1, 180.0) && close(y1, 180.0), "the hidden rect sets the box: ({x1}, {y1})");
    assert_eq!(l.children.len(), 2);
    assert_eq!(rect(&l.children[0]).style.fill, Some(Paint::None), "the hidden rect draws nothing");
    // Without such a clip path, hidden shapes are dropped as before.
    let plain = imp(&doc(r#"<g><rect width="9" height="9" visibility="hidden"/><rect width="5" height="5"/></g>"#));
    assert_eq!(plain.scene.elements.len(), 1);
}

// ── mask ─────────────────────────────────────────────────────────────────────

fn mask_of(e: &Element) -> (&Vec<Element>, MaskType) {
    let l = layer(e);
    (l.mask.as_ref().expect("a mask"), l.mask_type)
}

#[test]
fn mask_wraps_the_shape_in_a_layer_with_the_mask_content() {
    let i = imp(&doc(r##"<defs><mask id="m"><rect x="10" y="10" width="30" height="30" fill="white"/></mask></defs>
        <rect x="10" y="10" width="50" height="50" fill="red" mask="url(#m)"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let (content, kind) = mask_of(&i.scene.elements[0]);
    assert_eq!(kind, MaskType::Luminance);
    assert_eq!(content.len(), 1);
    assert_eq!(rect(&content[0]).width, 30.0, "content inside the region needs no clip layer");
    assert_eq!(rect(&layer(&i.scene.elements[0]).children[0]).width, 50.0);
}

#[test]
fn mask_region_defaults_to_120_percent_of_the_box_and_clips_larger_content() {
    // The box is (10, 10, 50, 50), so the region is 5 to 65.
    let i = imp(&doc(r##"<defs><mask id="m"><rect width="100" height="100" fill="white"/></mask></defs>
        <rect x="10" y="10" width="50" height="50" mask="url(#m)"/>"##));
    let (content, _) = mask_of(&i.scene.elements[0]);
    let clip = layer(&content[0]).clip_path.as_ref().expect("the region clips the content");
    assert_eq!(clip_rect(&clip[0]), (5.0, 5.0, 60.0, 60.0));
}

#[test]
fn mask_user_space_region_and_percent_units() {
    let i = imp(&doc(r##"<defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="40" height="30"><rect width="100" height="100" fill="white"/></mask></defs>
        <rect width="80" height="80" mask="url(#m)"/>"##));
    let (content, _) = mask_of(&i.scene.elements[0]);
    assert_eq!(clip_rect(&layer(&content[0]).clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 40.0, 30.0));
    let pct = imp(&doc(r##"<defs><mask id="m" x="0%" y="0%" width="50%" height="100%"><rect width="100" height="100" fill="white"/></mask></defs>
        <rect x="20" y="0" width="40" height="40" mask="url(#m)"/>"##));
    let (c, _) = mask_of(&pct.scene.elements[0]);
    assert_eq!(clip_rect(&layer(&c[0]).clip_path.as_ref().unwrap()[0]), (20.0, 0.0, 20.0, 40.0));
}

#[test]
fn mask_content_units_object_bounding_box_map_the_unit_square() {
    let i = imp(&doc(r##"<defs><mask id="m" maskContentUnits="objectBoundingBox"><rect width="0.5" height="1" fill="white"/></mask></defs>
        <rect x="20" y="10" width="40" height="20" mask="url(#m)"/>"##));
    let (content, _) = mask_of(&i.scene.elements[0]);
    let Element::Group(g) = &content[0] else { panic!("unit square group") };
    let (x, y) = geom::apply(g.transform.as_ref().unwrap().to_matrix(), 0.5, 1.0);
    assert!(close(x, 40.0) && close(y, 30.0), "({x}, {y})");
}

#[test]
fn mask_type_comes_from_the_attribute_or_the_style() {
    let attr = imp(&doc(r##"<defs><mask id="m" mask-type="alpha"><rect width="9" height="9"/></mask></defs><rect width="9" height="9" mask="url(#m)"/>"##));
    assert_eq!(mask_of(&attr.scene.elements[0]).1, MaskType::Alpha);
    let style = imp(&doc(r##"<defs><mask id="m" style="mask-type:alpha"><rect width="9" height="9"/></mask></defs><rect width="9" height="9" mask="url(#m)"/>"##));
    assert_eq!(mask_of(&style.scene.elements[0]).1, MaskType::Alpha);
    let bad = imp(&doc(r##"<defs><mask id="m" mask-type="nonsense"><rect width="9" height="9"/></mask></defs><rect width="9" height="9" mask="url(#m)"/>"##));
    assert_eq!(mask_of(&bad.scene.elements[0]).1, MaskType::Luminance);
}

#[test]
fn mask_content_keeps_its_own_paint_and_effects() {
    let i = imp(&doc(r##"<defs><linearGradient id="g"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient>
        <mask id="m"><rect x="10" y="10" width="20" height="20" fill="url(#g)" opacity=".5"/></mask></defs>
        <rect x="10" y="10" width="20" height="20" mask="url(#m)"/>"##));
    let (content, _) = mask_of(&i.scene.elements[0]);
    let r = rect(&content[0]);
    assert!(matches!(r.style.fill, Some(Paint::Ref(_))), "{:?}", r.style.fill);
    assert_eq!(r.style.opacity, Some(0.5));
}

#[test]
fn mask_on_a_mask_nests_a_masked_layer_in_the_content() {
    let i = imp(&doc(r##"<defs><mask id="a" mask="url(#b)"><rect x="10" y="10" width="20" height="20" fill="white"/></mask>
        <mask id="b" mask-type="alpha"><rect x="10" y="10" width="10" height="20"/></mask></defs>
        <rect x="10" y="10" width="20" height="20" mask="url(#a)"/>"##));
    let (content, kind) = mask_of(&i.scene.elements[0]);
    assert_eq!(kind, MaskType::Luminance);
    let inner = layer(&content[0]);
    assert_eq!(inner.mask_type, MaskType::Alpha);
    assert_eq!(inner.mask.as_ref().map(Vec::len), Some(1));
    assert_eq!(rect(&inner.children[0]).width, 20.0);
}

#[test]
fn mask_on_a_transformed_shape_or_a_group_keeps_the_space() {
    let i = imp(&doc(r##"<defs><mask id="m"><rect width="9" height="9" fill="white"/></mask></defs>
        <rect width="9" height="9" transform="translate(5 6)" mask="url(#m)"/>
        <g mask="url(#m)" transform="translate(1 2)"><rect width="3" height="3"/><rect x="5" width="3" height="3"/></g>"##));
    let a = layer(&i.scene.elements[0]);
    assert!(matches!(a.transform, Some(Transform::Translate { x, y }) if x == 5.0 && y == 6.0));
    assert!(rect(&a.children[0]).transform.is_none());
    let g = layer(&i.scene.elements[1]);
    assert!(g.mask.is_some() && g.transform.is_some() && g.children.len() == 2);
}

#[test]
fn unusable_masks_are_reported_or_hide_the_element() {
    let i = imp(&doc(r##"<defs><mask id="self"><rect width="5" height="5" fill="white" mask="url(#self)"/></mask><rect id="notmask" width="1" height="1"/>
        <mask id="empty" maskUnits="userSpaceOnUse" x="0" y="0" width="0" height="10"><rect width="5" height="5"/></mask></defs>
        <rect width="9" height="9" mask="url(#missing)"/>
        <rect width="9" height="9" mask="url(#notmask)"/>
        <rect width="9" height="9" mask="url(#self)"/>
        <rect width="9" height="9" mask="url(#empty)"/>"##));
    for needle in ["#missing", "#notmask", "refers back to itself"] {
        assert!(has_warning(&i, needle), "no warning for {needle}: {:?}", i.warnings);
    }
    let hidden = i.scene.elements.iter().filter_map(|e| if let Element::Layer(l) = e { Some(l) } else { None }).find(|l| l.mask.as_ref().is_some_and(Vec::is_empty));
    assert!(hidden.is_some(), "an empty region leaves a mask with nothing in it, which hides the element");
    let line = imp(&doc(r##"<defs><mask id="m"><rect width="9" height="9" fill="white"/></mask></defs><line x1="0" y1="5" x2="20" y2="5" stroke="red" mask="url(#m)"/>"##));
    assert_eq!(layer(&line.scene.elements[0]).mask.as_ref().map(Vec::len), Some(0), "a line has no box, so a box based mask hides it");
}

#[test]
fn masks_inside_a_clip_path_have_no_effect() {
    let i = imp(&doc(r##"<defs><mask id="m"><rect width="1" height="1" fill="black"/></mask><clipPath id="c"><rect width="9" height="9" mask="url(#m)"/></clipPath></defs>
        <rect width="20" height="20" clip-path="url(#c)"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(layer(&i.scene.elements[0]).mask.is_none());
}

#[test]
fn masked_imports_survive_write_and_parse() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60">
        <defs><linearGradient id="g"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient>
        <mask id="m"><rect width="80" height="60" fill="url(#g)"/></mask>
        <mask id="a" mask-type="alpha" maskContentUnits="objectBoundingBox"><circle cx=".5" cy=".5" r=".4"/></mask></defs>
        <rect width="80" height="60" fill="#f00" mask="url(#m)"/>
        <g mask="url(#a)" transform="translate(5 5)"><rect width="40" height="20" fill="#00f"/></g></svg>"##;
    let i = imp(svg);
    let text = msx_parser::scene_to_msx(&i.scene).expect("written");
    let back = msx_parser::parse_scene(&text).expect("parsed");
    assert_eq!(format!("{:?}", i.scene.elements), format!("{:?}", back.elements));
}

// ── images ───────────────────────────────────────────────────────────────────

fn png_uri(w: u32, h: u32) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    format!("data:image/png;base64,{}", STANDARD.encode(raster::tests_support::fake_png(w, h)))
}

fn image_of(e: &Element) -> &msx_ast::Image {
    match e {
        Element::Image(i) => i,
        other => panic!("expected an image, got {other:?}"),
    }
}

fn geometry(i: &msx_ast::Image) -> (f64, f64, f64, f64) {
    (i.x, i.y, i.width, i.height)
}

#[test]
fn a_png_data_uri_becomes_an_embedded_image() {
    let i = imp(&doc(&format!(r#"<image x="5" y="6" width="40" height="20" href="{}"/>"#, png_uri(40, 20))));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let img = image_of(&i.scene.elements[0]);
    assert_eq!(geometry(img), (5.0, 6.0, 40.0, 20.0));
    assert!(matches!(&img.source, msx_ast::MediaSource::Embedded(b) if b.starts_with(&[0x89, b'P'])));
}

#[test]
fn base64_with_line_breaks_and_the_xlink_href_work() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let b64 = STANDARD.encode(raster::tests_support::fake_png(8, 8));
    let broken: String = b64.as_bytes().chunks(12).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("\n    ");
    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="50" height="50"><image width="8" height="8" xlink:href="data:image/png;base64,
    {broken}"/></svg>"#);
    let i = imp(&svg);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert_eq!(image_of(&i.scene.elements[0]).width, 8.0);
}

#[test]
fn a_missing_size_comes_from_the_image() {
    let uri = png_uri(40, 20);
    let both = imp(&doc(&format!(r#"<image x="1" y="2" href="{uri}"/>"#)));
    assert_eq!(geometry(image_of(&both.scene.elements[0])), (1.0, 2.0, 40.0, 20.0));
    let only_w = imp(&doc(&format!(r#"<image width="80" href="{uri}"/>"#)));
    assert_eq!(geometry(image_of(&only_w.scene.elements[0])), (0.0, 0.0, 80.0, 40.0));
    let only_h = imp(&doc(&format!(r#"<image height="10" href="{uri}"/>"#)));
    assert_eq!(geometry(image_of(&only_h.scene.elements[0])), (0.0, 0.0, 20.0, 10.0));
}

#[test]
fn preserve_aspect_ratio_is_baked_into_the_placement() {
    let uri = png_uri(40, 20);
    let place = |par: &str| {
        let i = imp(&doc(&format!(r#"<image x="10" y="20" width="100" height="100" preserveAspectRatio="{par}" href="{uri}"/>"#)));
        geometry(image_of(&i.scene.elements[0]))
    };
    assert_eq!(place("xMidYMid meet"), (10.0, 45.0, 100.0, 50.0));
    assert_eq!(place("xMinYMin meet"), (10.0, 20.0, 100.0, 50.0));
    assert_eq!(place("xMaxYMax meet"), (10.0, 70.0, 100.0, 50.0));
    assert_eq!(place("none"), (10.0, 20.0, 100.0, 100.0));
}

#[test]
fn a_slice_that_spills_is_clipped_to_the_box_and_the_transform_moves_to_the_layer() {
    let uri = png_uri(40, 20);
    let i = imp(&doc(&format!(r#"<image x="10" y="20" width="100" height="100" preserveAspectRatio="xMidYMid slice" transform="translate(3 4)" href="{uri}"/>"#)));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(clip_rect(&l.clip_path.as_ref().unwrap()[0]), (10.0, 20.0, 100.0, 100.0));
    assert!(matches!(l.transform, Some(Transform::Translate { x, y }) if x == 3.0 && y == 4.0));
    let img = image_of(&l.children[0]);
    assert_eq!(geometry(img), (-40.0, 20.0, 200.0, 100.0));
    assert!(img.transform.is_none());
    let fits = imp(&doc(&format!(r#"<image width="40" height="20" preserveAspectRatio="xMidYMid slice" href="{uri}"/>"#)));
    assert!(matches!(fits.scene.elements[0], Element::Image(_)), "a slice that covers exactly needs no clip");
}

#[test]
fn image_opacity_transform_and_jpeg_are_kept() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let jpeg = format!("data:image/jpeg;base64,{}", STANDARD.encode(raster::tests_support::fake_jpeg(300, 200)));
    let i = imp(&doc(&format!(r#"<image width="60" height="40" opacity=".5" transform="rotate(10)" href="{jpeg}"/>"#)));
    let img = image_of(&i.scene.elements[0]);
    assert_eq!(img.style.opacity, Some(0.5));
    assert!(img.transform.is_some());
    // The mime type is not trusted: the bytes decide.
    let no_mime = imp(&doc(&format!(r#"<image width="30" href="data:;base64,{}"/>"#, STANDARD.encode(raster::tests_support::fake_jpeg(300, 200)))));
    assert_eq!(geometry(image_of(&no_mime.scene.elements[0])), (0.0, 0.0, 30.0, 20.0));
}

#[test]
fn images_take_part_in_clip_paths_and_masks() {
    let uri = png_uri(10, 10);
    let i = imp(&doc(&format!(r##"<defs><clipPath id="c"><circle cx="5" cy="5" r="4"/></clipPath></defs>
        <image x="0" y="0" width="10" height="10" transform="translate(7 8)" clip-path="url(#c)" href="{uri}"/>"##)));
    let l = layer(&i.scene.elements[0]);
    assert!(l.clip_path.is_some());
    assert!(matches!(l.transform, Some(Transform::Translate { x, y }) if x == 7.0 && y == 8.0));
    assert!(image_of(&l.children[0]).transform.is_none());
}

#[test]
fn images_msx_cannot_hold_are_reported_and_skipped() {
    let i = imp(&doc(r#"<image width="9" height="9" href="data:image/gif;base64,R0lGODlhAQABAAAAACw="/>
        <image width="9" height="9" href="data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg'/%3E"/>
        <image width="9" height="9" href="https://example.com/a.png"/>
        <image width="9" height="9" href="data:image/png;base64,@@@"/>"#));
    for needle in ["GIF", "SVG", "web address", "bad data URI"] {
        assert!(has_warning(&i, needle), "no warning for {needle}: {:?}", i.warnings);
    }
    assert!(i.scene.elements.is_empty());
}

#[test]
fn empty_or_zero_sized_images_draw_nothing_quietly() {
    let uri = png_uri(10, 10);
    let i = imp(&doc(&format!(r#"<image width="0" height="9" href="{uri}"/><image width="9" height="-1" href="{uri}"/><image width="9" height="9" href=""/><image width="9" height="9"/>"#)));
    assert!(i.scene.elements.is_empty());
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
}

#[test]
fn image_files_are_embedded_with_a_base_directory_and_kept_as_paths_without() {
    let dir = std::env::temp_dir().join(format!("msx-import-images-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pic.png"), raster::tests_support::fake_png(30, 10)).unwrap();
    let svg = doc(r#"<image x="1" y="2" width="60" href="pic.png"/><image width="5" height="5" href="missing.png"/>"#);

    let with = import_svg_with(&svg, &ImportOptions { base_dir: Some(dir.clone()) }).unwrap();
    assert_eq!(geometry(image_of(&with.scene.elements[0])), (1.0, 2.0, 60.0, 20.0), "the height follows the embedded size");
    assert!(matches!(image_of(&with.scene.elements[0]).source, msx_ast::MediaSource::Embedded(_)));
    assert!(with.warnings.iter().any(|w| w.contains("missing.png") && w.contains("could not read")), "{:?}", with.warnings);
    assert_eq!(with.scene.elements.len(), 1, "the unreadable file is skipped");

    let without = imp(&doc(r#"<image x="1" y="2" width="60" height="20" href="pic.png"/>"#));
    assert!(matches!(&image_of(&without.scene.elements[0]).source, msx_ast::MediaSource::FileRef(p) if p == "pic.png"));
    assert!(has_warning(&without, "not embedded"));
    let sizeless = imp(&doc(r#"<image href="pic.png"/>"#));
    assert!(sizeless.scene.elements.is_empty() && has_warning(&sizeless, "size is unknown"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imported_images_survive_write_and_parse() {
    let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80">
        <image x="10" y="10" width="100" height="60" preserveAspectRatio="xMidYMid slice" opacity=".8" href="{}"/>
        <image id="b" x="0" y="0" width="30" height="30" transform="rotate(5)" href="{}"/></svg>"##, png_uri(40, 20), png_uri(8, 8));
    let i = imp(&svg);
    let text = msx_parser::scene_to_msx(&i.scene).expect("written");
    let back = msx_parser::parse_scene(&text).expect("parsed");
    assert_eq!(format!("{:?}", i.scene.elements), format!("{:?}", back.elements));
}

// ── markers ──────────────────────────────────────────────────────────────────

/// Maps the point `(x, y)` in the marker content through a marker copy.
fn place(e: &Element, x: f64, y: f64) -> (f64, f64) {
    let m = |t: &Option<Transform>| t.as_ref().map(|t| t.to_matrix()).unwrap_or_else(msx_ast::Matrix2D::identity);
    match e {
        Element::Group(g) => geom::apply(m(&g.transform), x, y),
        Element::Layer(l) => {
            let (x, y) = match l.children.first() {
                Some(Element::Group(g)) => geom::apply(m(&g.transform), x, y),
                _ => (x, y),
            };
            geom::apply(m(&l.transform), x, y)
        }
        other => panic!("not a marker copy: {other:?}"),
    }
}

fn at(p: (f64, f64)) -> (f64, f64) {
    ((p.0 * 1000.0).round() / 1000.0, (p.1 * 1000.0).round() / 1000.0)
}

const M4: &str = r##"<defs><marker id="m" markerWidth="4" markerHeight="4" refX="2" refY="2"><rect width="4" height="4" fill="red"/></marker></defs>"##;

#[test]
fn a_marker_is_placed_at_the_vertex_and_scaled_by_the_stroke_width() {
    let i = imp(&doc(&format!(r##"{M4}<line x1="0" y1="0" x2="10" y2="0" stroke="#000" stroke-width="2" marker-end="url(#m)"/>"##)));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let l = layer(&i.scene.elements[0]);
    assert_eq!(l.children.len(), 2, "the line and one marker copy");
    assert!(matches!(l.children[0], Element::Line(_)));
    let copy = &l.children[1];
    assert_eq!(at(place(copy, 2.0, 2.0)), (10.0, 0.0), "the reference point lands on the end of the line");
    assert_eq!(at(place(copy, 4.0, 4.0)), (14.0, 4.0), "scaled by the stroke width of 2");
}

#[test]
fn start_mid_and_end_markers_go_on_the_right_vertices_in_that_order() {
    let svg = format!(r##"{M4}<polyline points="0,0 10,0 10,10 20,10" fill="none" stroke="#000" marker-start="url(#m)" marker-mid="url(#m)" marker-end="url(#m)"/>"##);
    let i = imp(&doc(&svg));
    let l = layer(&i.scene.elements[0]);
    let spots: Vec<(f64, f64)> = l.children[1..].iter().map(|c| at(place(c, 2.0, 2.0))).collect();
    assert_eq!(spots, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (20.0, 10.0)]);
}

#[test]
fn only_the_marker_properties_that_are_set_draw() {
    let i = imp(&doc(&format!(r##"{M4}<polyline points="0,0 10,0 10,10" fill="none" stroke="#000" marker-mid="url(#m)"/>"##)));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(l.children.len(), 2);
    assert_eq!(at(place(&l.children[1], 2.0, 2.0)), (10.0, 0.0));
    let none = imp(&doc(&format!(r##"{M4}<polyline points="0,0 10,0" stroke="#000"/>"##)));
    assert!(matches!(none.scene.elements[0], Element::Polyline(_)), "no markers, no layer");
}

#[test]
fn orient_auto_turns_the_marker_along_the_path() {
    let svg = r##"<defs><marker id="m" markerWidth="4" markerHeight="4" refX="0" refY="0" orient="auto" markerUnits="userSpaceOnUse"><rect width="4" height="4"/></marker></defs>
        <line x1="0" y1="0" x2="0" y2="10" stroke="#000" marker-end="url(#m)"/>"##;
    let i = imp(&doc(svg));
    let copy = &layer(&i.scene.elements[0]).children[1];
    assert_eq!(at(place(copy, 1.0, 0.0)), (0.0, 11.0), "the content x axis points down the path");
    let fixed = imp(&doc(&svg.replace(r#"orient="auto""#, r#"orient="90deg""#)));
    assert_eq!(at(place(&layer(&fixed.scene.elements[0]).children[1], 1.0, 0.0)), (0.0, 11.0));
    let reversed = imp(&doc(&svg.replace(r#"orient="auto""#, r#"orient="auto-start-reverse""#).replace("marker-end", "marker-start")));
    assert_eq!(at(place(&layer(&reversed.scene.elements[0]).children[1], 1.0, 0.0)), (0.0, -1.0), "the start marker is turned half a turn");
}

#[test]
fn marker_units_user_space_do_not_scale_with_the_stroke() {
    let svg = r##"<defs><marker id="m" markerWidth="4" markerHeight="4" refX="2" refY="2" markerUnits="userSpaceOnUse"><rect width="4" height="4"/></marker></defs>
        <line x1="0" y1="0" x2="10" y2="0" stroke="#000" stroke-width="5" marker-end="url(#m)"/>"##;
    let i = imp(&doc(svg));
    assert_eq!(at(place(&layer(&i.scene.elements[0]).children[1], 4.0, 4.0)), (12.0, 2.0));
    let zero = imp(&doc(&format!(r##"{M4}<line x1="0" y1="0" x2="10" y2="0" stroke="#000" stroke-width="0" marker-end="url(#m)"/>"##)));
    assert!(matches!(zero.scene.elements[0], Element::Line(_)), "a zero stroke width scales the marker away");
}

#[test]
fn marker_view_box_scales_the_content_into_the_marker_viewport() {
    // A 10 by 10 content box in a 20 by 20 viewport: scale 2, and the reference point is in content units.
    let svg = r##"<defs><marker id="m" markerWidth="20" markerHeight="20" viewBox="0 0 10 10" refX="5" refY="5" markerUnits="userSpaceOnUse"><rect width="10" height="10"/></marker></defs>
        <line x1="0" y1="0" x2="50" y2="0" stroke="#000" marker-end="url(#m)"/>"##;
    let i = imp(&doc(svg));
    let copy = &layer(&i.scene.elements[0]).children[1];
    assert_eq!(at(place(copy, 5.0, 5.0)), (50.0, 0.0));
    assert_eq!(at(place(copy, 10.0, 5.0)), (60.0, 0.0), "5 content units are 10 user units");
}

#[test]
fn marker_content_past_the_viewport_is_clipped_unless_overflow_is_visible() {
    let svg = |extra: &str| format!(r##"<defs><marker id="m" markerWidth="4" markerHeight="4" refX="0" refY="0" markerUnits="userSpaceOnUse" {extra}><rect width="9" height="9"/></marker></defs>
        <line x1="0" y1="0" x2="10" y2="0" stroke="#000" marker-end="url(#m)"/>"##);
    let clipped = imp(&doc(&svg("")));
    let copy = layer(&layer(&clipped.scene.elements[0]).children[1]);
    assert_eq!(clip_rect(&copy.clip_path.as_ref().unwrap()[0]), (0.0, 0.0, 4.0, 4.0));
    let visible = imp(&doc(&svg(r#"overflow="visible""#)));
    assert!(matches!(layer(&visible.scene.elements[0]).children[1], Element::Group(_)));
    let fits = imp(&doc(&format!(r##"{M4}<line x1="0" y1="0" x2="10" y2="0" stroke="#000" marker-end="url(#m)"/>"##)));
    assert!(matches!(layer(&fits.scene.elements[0]).children[1], Element::Group(_)), "content inside the viewport needs no clip");
}

#[test]
fn rects_circles_and_ellipses_get_markers_at_their_outline_vertices() {
    let all = r#"marker-start="url(#m)" marker-mid="url(#m)" marker-end="url(#m)""#;
    let rect = imp(&doc(&format!(r##"{M4}<rect x="20" y="20" width="160" height="160" fill="none" stroke="#000" {all}/>"##)));
    let spots: Vec<(f64, f64)> = layer(&rect.scene.elements[0]).children[1..].iter().map(|c| at(place(c, 2.0, 2.0))).collect();
    assert_eq!(spots, vec![(20.0, 20.0), (180.0, 20.0), (180.0, 180.0), (20.0, 180.0), (20.0, 20.0)]);
    let circle = imp(&doc(&format!(r##"{M4}<circle cx="100" cy="100" r="80" fill="none" stroke="#000" {all}/>"##)));
    assert_eq!(layer(&circle.scene.elements[0]).children.len(), 1 + 6, "the circle, then a start, four mids and an end");
}

#[test]
fn marker_properties_inherit_and_the_marker_attribute_is_ignored() {
    let inherited = imp(&doc(&format!(r##"{M4}<g marker-end="url(#m)"><path d="M0 0 L10 0" stroke="#000"/><path d="M0 5 L10 5" stroke="#000" marker-end="none"/></g>"##)));
    let l = layer(&inherited.scene.elements[0]);
    assert!(matches!(&inherited.scene.elements[1], Element::Path(_)), "the second path switches it off");
    assert_eq!(l.children.len(), 2);
    let css = imp(&doc(&format!(r##"{M4}<path d="M0 0 L10 0" stroke="#000" style="marker: url(#m)"/>"##)));
    assert_eq!(layer(&css.scene.elements[0]).children.len(), 1 + 2, "the CSS shorthand sets all three");
    let attr = imp(&doc(&format!(r##"{M4}<path d="M0 0 L10 0" stroke="#000" marker="url(#m)"/>"##)));
    assert!(matches!(attr.scene.elements[0], Element::Path(_)), "marker is valid only in CSS");
}

#[test]
fn markers_fade_clip_and_transform_with_their_shape() {
    let svg = format!(r##"{M4}<defs><clipPath id="c"><rect width="50" height="50"/></clipPath></defs>
        <line x1="0" y1="0" x2="10" y2="0" stroke="#000" opacity=".5" transform="translate(7 8)" clip-path="url(#c)" marker-end="url(#m)"/>"##);
    let i = imp(&doc(&svg));
    let l = layer(&i.scene.elements[0]);
    assert_eq!(l.opacity, 0.5, "the shape and its markers fade together");
    let Element::Line(line) = &l.children[0] else { panic!("line") };
    assert_eq!(line.style.opacity, Some(1.0));
    assert!(line.transform.is_none(), "the transform moved to the layer");
    assert!(matches!(l.transform, Some(Transform::Translate { x, y }) if x == 7.0 && y == 8.0));
    assert!(l.clip_path.is_some());
}

#[test]
fn unusable_markers_are_reported_and_cycles_stop() {
    let i = imp(&doc(r##"<defs><marker id="zero" markerWidth="0" markerHeight="4"><rect width="1" height="1"/></marker><rect id="notmarker" width="1" height="1"/>
        <marker id="loop" markerWidth="4" markerHeight="4"><path id="inner" d="M0 0 L2 2" stroke="#000" marker-end="url(#loop)"/></marker></defs>
        <path d="M0 0 L10 0" stroke="#000" marker-end="url(#missing)"/>
        <path d="M0 0 L10 0" stroke="#000" marker-end="url(#notmarker)"/>
        <path d="M0 0 L10 0" stroke="#000" marker-end="url(#zero)"/>
        <path d="M0 0 L10 0" stroke="#000" marker-end="url(#loop)"/>"##));
    for needle in ["#missing", "#notmarker", "refers back to itself"] {
        assert!(has_warning(&i, needle), "no warning for {needle}: {:?}", i.warnings);
    }
    assert_eq!(i.scene.elements.iter().filter(|e| matches!(e, Element::Path(_))).count(), 3, "the plain paths stay plain");
}

#[test]
fn a_marker_that_uses_the_shape_it_marks_still_draws_both_ends() {
    // The shape of the recursive tests: the marker content is a use of a path that has markers of its own.
    let svg = r##"<defs><marker id="a" markerWidth="6" markerHeight="6" refX="1" refY="3"><use href="#p"/></marker>
        <marker id="b" markerWidth="6" markerHeight="6" refX="5" refY="3"><use href="#p"/></marker>
        <path id="p" d="M1 1 5 3 1 5" fill="none" stroke="#00f" marker-start="url(#a)" marker-end="url(#b)"/></defs>
        <path d="M10 10 L190 190" fill="none" stroke="#0a0" stroke-width="5" marker-start="url(#a)" marker-end="url(#b)"/>"##;
    let i = imp(&doc(svg));
    let l = layer(&i.scene.elements[0]);
    assert!(l.children.len() >= 3, "the path, a start copy and an end copy: {}", l.children.len());
}

#[test]
fn imported_markers_survive_write_and_parse() {
    let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60">{M4}
        <polyline points="5,5 40,5 40,40 80,40" fill="none" stroke="#06f" stroke-width="2" marker-start="url(#m)" marker-mid="url(#m)" marker-end="url(#m)"/></svg>"##);
    let i = imp(&svg);
    let text = msx_parser::scene_to_msx(&i.scene).expect("written");
    let back = msx_parser::parse_scene(&text).expect("parsed");
    assert_eq!(format!("{:?}", i.scene.elements), format!("{:?}", back.elements));
}

#[test]
fn overflow_inherit_takes_the_parent_value_and_falls_back_to_visible() {
    let content = r##"<marker id="m" overflow="inherit" markerWidth="4" markerHeight="4" markerUnits="userSpaceOnUse"><rect width="9" height="9"/></marker>
        <line x1="0" y1="0" x2="10" y2="0" stroke="#000" marker-end="url(#m)"/>"##;
    let svg = |attrs: &str| format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" {attrs}>{content}</svg>"#);
    let open = imp(&svg(r#"overflow="visible""#));
    assert!(matches!(layer(&open.scene.elements[0]).children[1], Element::Group(_)), "a visible parent leaves the marker unclipped");
    let closed = imp(&svg(r#"overflow="hidden""#));
    assert!(matches!(layer(&closed.scene.elements[0]).children[1], Element::Layer(_)), "a hidden parent clips it");
    let nothing = imp(&svg(""));
    assert!(matches!(layer(&nothing.scene.elements[0]).children[1], Element::Group(_)), "with no value up the tree it is visible");
}

// ── SMIL animation ───────────────────────────────────────────────────────────

fn find_rect(el: &Element, acc: msx_ast::Matrix2D) -> Option<(msx_ast::Matrix2D, &Rect)> {
    let with = |t: &Option<Transform>| acc.concat(t.as_ref().map_or(msx_ast::Matrix2D::identity(), |t| t.to_matrix()));
    match el {
        Element::Group(g) => g.children.iter().find_map(|c| find_rect(c, with(&g.transform))),
        Element::Layer(l) => l.children.iter().find_map(|c| find_rect(c, with(&l.transform))),
        Element::Rect(r) => Some((with(&r.transform), r)),
        _ => None,
    }
}

/// Where the first rect's local point lands in the scene at time `t`.
fn frame_point(i: &Imported, t: f64, x: f64, y: f64) -> (f64, f64) {
    let scene = msx_anim::resolve_at_time(&i.scene, t);
    let (m, _) = scene.elements.iter().find_map(|e| find_rect(e, msx_ast::Matrix2D::identity())).expect("a rect in the frame");
    geom::apply(m, x, y)
}

fn near(p: (f64, f64), q: (f64, f64)) -> bool {
    (p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6
}

fn track(i: &Imported, prop: msx_ast::AnimatedProperty) -> Vec<&msx_ast::AnimationTrack> {
    i.scene.animations.iter().filter(|t| t.property == prop).collect()
}

fn group(e: &Element) -> &Group {
    match e {
        Element::Group(g) => g,
        other => panic!("expected a group, got {other:?}"),
    }
}

const SPINNER: &str = r##"<rect id="r" x="10" y="10" width="20" height="20" fill="#f00">
    <animateTransform attributeName="transform" type="rotate" from="0 20 20" to="360 20 20" dur="2s" repeatCount="indefinite"/></rect>"##;

#[test]
fn a_spinner_becomes_a_looping_rotation_about_its_centre() {
    let i = imp(&doc(SPINNER));
    assert_eq!(i.scene.duration, 2.0);
    assert_eq!(i.scene.loop_mode, msx_ast::LoopMode::Loop);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let tracks = track(&i, msx_ast::AnimatedProperty::Rotate);
    assert_eq!(tracks.len(), 1);
    assert_eq!(i.scene.animations.len(), 1, "only the rotation moves");
    let keys: Vec<(f64, f64)> = tracks[0].keyframes.iter().map(|k| (k.time, k.value)).collect();
    assert_eq!(keys, vec![(0.0, 0.0), (2.0, 360.0)]);
    // translate(centre) > wrapper > translate(-centre) > rect
    let outer = group(&i.scene.elements[0]);
    assert!(outer.id.is_none() && outer.transform.is_some());
    let wrapper = group(&outer.children[0]);
    assert_eq!(wrapper.id.as_deref(), Some(tracks[0].target_id.as_str()));
    assert_eq!(rect(&group(&wrapper.children[0]).children[0]).id.as_deref(), Some("r"));
    // A quarter turn after 0.5s of 2s takes (30,20), 10 right of the centre, to 10 below it.
    assert!(near(frame_point(&i, 0.5, 30.0, 20.0), (20.0, 30.0)));
    assert!(near(frame_point(&i, 1.0, 30.0, 20.0), (10.0, 20.0)));
    assert!(near(frame_point(&i, 0.0, 30.0, 20.0), (30.0, 20.0)));
    // The loop wraps.
    assert!(near(frame_point(&i, 2.5, 30.0, 20.0), (20.0, 30.0)));
}

#[test]
fn an_animation_sits_inside_the_static_transform() {
    let i = imp(&doc(r##"<g transform="translate(5 5)"><animateTransform attributeName="transform" type="translate" values="0 0;10 0;0 0" dur="1s" repeatCount="indefinite" additive="sum"/>
        <rect width="4" height="4"/></g>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert_eq!(i.scene.animations.len(), 1, "translate y never changes");
    let outer = group(&i.scene.elements[0]);
    assert!(matches!(outer.transform, Some(Transform::Translate { x, y }) if x == 5.0 && y == 5.0));
    assert!(near(frame_point(&i, 0.0, 0.0, 0.0), (5.0, 5.0)));
    assert!(near(frame_point(&i, 0.5, 0.0, 0.0), (15.0, 5.0)));
    assert!(near(frame_point(&i, 1.0, 0.0, 0.0), (5.0, 5.0)));
}

#[test]
fn a_rotation_is_applied_inside_a_static_scale() {
    // SMIL adds the animation after the static transform: scale(2 1) rotate(90).
    // (10,0) turns to (0,10) first and the stretch of x leaves it there. With the
    // animation on the outside the point would land on (0,20).
    let i = imp(&doc(r##"<rect width="10" height="10" transform="scale(2 1)" fill="#000">
        <animateTransform attributeName="transform" type="rotate" from="0" to="90" dur="1s" additive="sum" fill="freeze"/></rect>"##));
    assert!(near(frame_point(&i, 1.0, 10.0, 0.0), (0.0, 10.0)), "{:?}", frame_point(&i, 1.0, 10.0, 0.0));
    assert!(near(frame_point(&i, 1.0, 0.0, 10.0), (-20.0, 0.0)), "{:?}", frame_point(&i, 1.0, 0.0, 10.0));
}

#[test]
fn opacity_animation_replaces_the_static_opacity_and_loops() {
    let i = imp(&doc(r##"<circle cx="5" cy="5" r="4" opacity="0.5"><animate attributeName="opacity" values="0.5;1;0.5" dur="2s" repeatCount="indefinite"/></circle>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let Element::Layer(l) = &i.scene.elements[0] else { panic!("expected a layer, got {:?}", i.scene.elements[0]) };
    assert_eq!(l.opacity, 1.0);
    assert!(matches!(l.children[0], Element::Circle(_)), "the static 0.5 is left off the circle");
    let at = |t: f64| match &msx_anim::resolve_at_time(&i.scene, t).elements[0] {
        Element::Layer(l) => l.opacity,
        other => panic!("{other:?}"),
    };
    assert!(close(at(0.0), 0.5) && close(at(1.0), 1.0) && close(at(2.0), 0.5) && close(at(0.5), 0.75));
}

#[test]
fn a_delayed_frozen_animation_plays_once() {
    let i = imp(&doc(r##"<rect width="5" height="5"><animate attributeName="x" from="0" to="20" begin="1s" dur="2s" fill="freeze"/></rect>"##));
    assert_eq!(i.scene.loop_mode, msx_ast::LoopMode::Once);
    assert_eq!(i.scene.duration, 3.0);
    assert!(near(frame_point(&i, 0.5, 0.0, 0.0), (0.0, 0.0)));
    assert!(near(frame_point(&i, 2.0, 0.0, 0.0), (10.0, 0.0)));
    assert!(near(frame_point(&i, 3.0, 0.0, 0.0), (20.0, 0.0)));
    assert!(near(frame_point(&i, 9.0, 0.0, 0.0), (20.0, 0.0)), "frozen");
}

#[test]
fn x_animation_moves_by_the_difference_from_the_attribute() {
    let i = imp(&doc(r##"<rect x="30" y="10" width="5" height="5"><animate attributeName="x" from="30" to="50" dur="1s" fill="freeze"/>
        <animate attributeName="y" by="7" dur="1s" fill="freeze"/></rect>"##));
    assert!(near(frame_point(&i, 1.0, 30.0, 10.0), (50.0, 17.0)), "{:?}", frame_point(&i, 1.0, 30.0, 10.0));
    let c = imp(&doc(r##"<circle cx="10" cy="10" r="3"><animate attributeName="cx" values="10;40;10" dur="2s" repeatCount="indefinite"/></circle>"##));
    assert_eq!(track(&c, msx_ast::AnimatedProperty::TranslateX).len(), 1);
}

#[test]
fn several_animations_on_one_element_nest_in_document_order() {
    let i = imp(&doc(r##"<rect width="10" height="10" fill="#000">
        <animateTransform attributeName="transform" type="rotate" from="0" to="90" dur="1s" fill="freeze" additive="sum"/>
        <animateTransform attributeName="transform" type="translate" from="0 0" to="10 0" dur="1s" fill="freeze" additive="sum"/></rect>"##));
    let outer = group(&i.scene.elements[0]);
    let inner = group(&outer.children[0]);
    let id_of = |g: &Group| g.id.clone().unwrap();
    let prop_of = |id: String| i.scene.animations.iter().find(|t| t.target_id == id).unwrap().property;
    assert_eq!(prop_of(id_of(outer)), msx_ast::AnimatedProperty::Rotate);
    assert_eq!(prop_of(id_of(inner)), msx_ast::AnimatedProperty::TranslateX);
    // rotate(90) then translate(10,0) in local space: (0,0) goes to (10,0), then turns to (0,10).
    assert!(near(frame_point(&i, 1.0, 0.0, 0.0), (0.0, 10.0)), "{:?}", frame_point(&i, 1.0, 0.0, 0.0));
}

#[test]
fn an_animation_can_name_its_target_with_href() {
    let i = imp(&doc(r##"<rect id="r" width="5" height="5"/><animate href="#r" attributeName="opacity" from="1" to="0" dur="1s" fill="freeze"/>
        <animateTransform xlink:href="#r" attributeName="transform" type="translate" to="10 0" dur="1s" fill="freeze"/>"##));
    assert_eq!(i.scene.animations.len(), 2);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
}

#[test]
fn repeating_animations_share_a_loop() {
    let i = imp(&doc(r##"<rect id="a" width="5" height="5"><animateTransform attributeName="transform" type="rotate" to="360" dur="1s" repeatCount="indefinite"/></rect>
        <rect id="b" width="5" height="5"><animateTransform attributeName="transform" type="rotate" to="360" dur="1.5s" repeatCount="indefinite"/></rect>"##));
    assert_eq!(i.scene.duration, 3.0);
    assert_eq!(i.scene.loop_mode, msx_ast::LoopMode::Loop);
    let lens: Vec<usize> = i.scene.animations.iter().map(|t| t.keyframes.len()).collect();
    assert_eq!(lens.len(), 2);
    assert!(lens.contains(&6) && lens.contains(&4), "unrolled to whole iterations: {lens:?}");
}

#[test]
fn key_splines_are_baked_into_keyframes() {
    let i = imp(&doc(r##"<rect width="5" height="5"><animate attributeName="x" values="0;100" calcMode="spline" keySplines="0.42 0 0.58 1" dur="1s" fill="freeze"/></rect>"##));
    let t = &track(&i, msx_ast::AnimatedProperty::TranslateX)[0];
    assert_eq!(t.keyframes.len(), 13);
    assert!((frame_point(&i, 0.5, 0.0, 0.0).0 - 50.0).abs() < 0.5);
    assert!(frame_point(&i, 0.1, 0.0, 0.0).0 < 10.0);
}

#[test]
fn animations_that_cannot_run_are_reported_and_the_static_frame_stays() {
    let i = imp(&doc(r##"<rect width="5" height="5"><animate attributeName="opacity" from="0" to="1" begin="click" dur="1s"/></rect>
        <rect width="5" height="5"><animate attributeName="width" from="1" to="9" dur="1s"/></rect>
        <rect width="5" height="5"><animate attributeName="opacity" from="0" to="1"/></rect>
        <rect width="5" height="5"><animateMotion path="M0 0 L9 9" dur="1s"/></rect>
        <rect width="5" height="5"><animate attributeName="opacity" values="0;1" keyTimes="0;0.5" dur="1s"/></rect>
        <text x="1" y="1"><animate attributeName="x" from="0" to="5" dur="1s"/>hi</text>"##));
    for needle in ["waits for an event", "animating width", "no finite dur", "<animateMotion>", "keyTimes must rise", "x on <text>"] {
        assert!(has_warning(&i, needle), "no warning for {needle}: {:?}", i.warnings);
    }
    assert!(i.scene.animations.is_empty());
    assert_eq!(i.scene.elements.len(), 6);
    assert!(matches!(i.scene.elements[0], Element::Rect(_)), "no wrapper around a rect whose animation was refused");
}

#[test]
fn replacing_a_static_transform_is_noted() {
    let i = imp(&doc(r##"<rect width="5" height="5" transform="translate(5 5)"><animateTransform attributeName="transform" type="rotate" to="90" dur="1s"/></rect>"##));
    assert!(has_warning(&i, "replaces the transform attribute"), "{:?}", i.warnings);
    assert_eq!(i.scene.animations.len(), 1);
}

#[test]
fn animations_inside_a_mask_are_reported_and_left_static() {
    let i = imp(&doc(r##"<defs><mask id="m"><rect width="9" height="9" fill="#fff"><animate attributeName="opacity" from="0" to="1" dur="1s"/></rect></mask></defs>
        <rect width="20" height="20" mask="url(#m)"/>"##));
    assert!(has_warning(&i, "inside a mask or marker"), "{:?}", i.warnings);
    assert!(i.scene.animations.is_empty());
}

#[test]
fn use_copies_animate_independently() {
    let i = imp(&doc(r##"<defs><rect id="d" width="5" height="5"><animateTransform attributeName="transform" type="translate" to="10 0" dur="1s" fill="freeze"/></rect></defs>
        <use href="#d"/><use href="#d" y="20"/>"##));
    assert_eq!(i.scene.animations.len(), 2);
    assert_ne!(i.scene.animations[0].target_id, i.scene.animations[1].target_id);
}

#[test]
fn imported_animations_survive_write_and_parse() {
    let svg = doc(&format!(
        "{SPINNER}<circle cx=\"60\" cy=\"60\" r=\"9\" opacity=\".4\"><animate attributeName=\"opacity\" values=\".4;1;.4\" dur=\"1.5s\" repeatCount=\"indefinite\"/></circle>\
         <g transform=\"translate(3 4)\"><animateTransform attributeName=\"transform\" type=\"scale\" values=\"1;2;1\" calcMode=\"spline\" keySplines=\".4 0 .6 1;.4 0 .6 1\" dur=\"3s\" repeatCount=\"indefinite\" additive=\"sum\"/><rect width=\"9\" height=\"9\"/></g>"
    ));
    let i = imp(&svg);
    assert!(i.scene.animations.len() >= 3);
    let text = msx_parser::scene_to_msx(&i.scene).expect("animated scenes are writable");
    let back = msx_parser::parse_scene(&text).unwrap_or_else(|e| panic!("written source must parse: {e}"));
    assert_eq!(back.duration, i.scene.duration);
    assert_eq!(back.loop_mode, i.scene.loop_mode);
    assert_eq!(format!("{:?}", back.animations), format!("{:?}", i.scene.animations));
    for t in [0.0, 0.4, 1.1, 2.9, 3.7] {
        assert_eq!(format!("{:?}", msx_anim::resolve_at_time(&back, t).elements), format!("{:?}", msx_anim::resolve_at_time(&i.scene, t).elements), "frame at {t}");
    }
}

fn find_shape_matrix(el: &Element, acc: msx_ast::Matrix2D) -> Option<msx_ast::Matrix2D> {
    let with = |t: &Option<Transform>| acc.concat(t.as_ref().map_or(msx_ast::Matrix2D::identity(), |t| t.to_matrix()));
    match el {
        Element::Group(g) => g.children.iter().find_map(|c| find_shape_matrix(c, with(&g.transform))),
        Element::Layer(l) => l.children.iter().find_map(|c| find_shape_matrix(c, with(&l.transform))),
        Element::Circle(c) => Some(with(&c.transform)),
        Element::Rect(r) => Some(with(&r.transform)),
        _ => None,
    }
}

/// Where a local point of the first circle or rect lands at time `t`.
fn shape_point(i: &Imported, t: f64, x: f64, y: f64) -> (f64, f64) {
    let scene = msx_anim::resolve_at_time(&i.scene, t);
    let m = scene.elements.iter().find_map(|e| find_shape_matrix(e, msx_ast::Matrix2D::identity())).expect("a shape in the frame");
    geom::apply(m, x, y)
}

fn layer_opacity_at(i: &Imported, t: f64) -> f64 {
    match &msx_anim::resolve_at_time(&i.scene, t).elements[0] {
        Element::Layer(l) => l.opacity,
        other => panic!("expected a layer, got {other:?}"),
    }
}

#[test]
fn fill_opacity_on_a_filled_shape_fades_the_whole_shape() {
    let i = imp(&doc(r##"<circle cx="5" cy="5" r="4" fill="#f00"><animate attributeName="fill-opacity" values="1;.2;1" dur="2s" repeatCount="indefinite"/></circle>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(close(layer_opacity_at(&i, 0.0), 1.0) && close(layer_opacity_at(&i, 1.0), 0.2) && close(layer_opacity_at(&i, 0.5), 0.6));
    assert_eq!(i.scene.loop_mode, msx_ast::LoopMode::Loop);
}

#[test]
fn fill_opacity_animation_replaces_the_static_value() {
    // Animating from .5 up to 1 is allowed: the static .5 is left off the shape
    // and the layer carries the absolute value.
    let i = imp(&doc(r##"<circle cx="5" cy="5" r="4" fill="#f00" fill-opacity=".5"><animate attributeName="fill-opacity" values=".5;1;.5" dur="2s" repeatCount="indefinite"/></circle>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(close(layer_opacity_at(&i, 0.0), 0.5) && close(layer_opacity_at(&i, 1.0), 1.0) && close(layer_opacity_at(&i, 0.5), 0.75));
    let Element::Layer(l) = &i.scene.elements[0] else { panic!() };
    let Element::Circle(c) = &l.children[0] else { panic!("{:?}", l.children[0]) };
    let Some(Paint::Color(col)) = c.style.fill else { panic!("{:?}", c.style) };
    assert_eq!(col.a, 255, "the shape paints at full alpha, the layer fades it");
}

#[test]
fn a_static_fill_opacity_of_zero_is_a_valid_start() {
    let i = imp(&doc(r##"<circle cx="5" cy="5" r="4" fill-opacity="0"><animate attributeName="fill-opacity" from="0" to="1" dur="1s" fill="freeze"/></circle>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(close(layer_opacity_at(&i, 0.0), 0.0) && close(layer_opacity_at(&i, 1.0), 1.0));
}

#[test]
fn fill_opacity_is_refused_where_it_would_not_be_exact() {
    let both = imp(&doc(r##"<circle cx="5" cy="5" r="4" fill="#f00" stroke="#000" stroke-width="2"><animate attributeName="fill-opacity" values="1;0;1" dur="1s" repeatCount="indefinite"/></circle>"##));
    assert!(has_warning(&both, "also paints its stroke"), "{:?}", both.warnings);
    let inherited = imp(&doc(r##"<g stroke="#000"><circle cx="5" cy="5" r="4"><animate attributeName="fill-opacity" values="1;0;1" dur="1s" repeatCount="indefinite"/></circle></g>"##));
    assert!(has_warning(&inherited, "also paints its stroke"), "a stroke inherited from the group counts: {:?}", inherited.warnings);
    let none = imp(&doc(r##"<circle cx="5" cy="5" r="4" fill="none"><animate attributeName="fill-opacity" values="1;0;1" dur="1s" repeatCount="indefinite"/></circle>"##));
    assert!(has_warning(&none, "no fill to fade"), "{:?}", none.warnings);
    for i in [&both, &inherited, &none] {
        assert!(i.scene.animations.is_empty());
    }
}

#[test]
fn stroke_opacity_works_on_a_shape_that_only_strokes() {
    let i = imp(&doc(r##"<circle cx="5" cy="5" r="4" fill="none" stroke="#000" stroke-width="2"><animate attributeName="stroke-opacity" values="1;0;1" dur="2s" repeatCount="indefinite"/></circle>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(close(layer_opacity_at(&i, 1.0), 0.0));
    let filled = imp(&doc(r##"<circle cx="5" cy="5" r="4" stroke="#000"><animate attributeName="stroke-opacity" values="1;0;1" dur="2s" repeatCount="indefinite"/></circle>"##));
    assert!(has_warning(&filled, "also paints its fill"), "{:?}", filled.warnings);
}

#[test]
fn circle_radius_scales_about_the_centre() {
    let i = imp(&doc(r##"<circle cx="10" cy="10" r="4" fill="#00f"><animate attributeName="r" from="4" to="12" dur="1s" fill="freeze"/></circle>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    // The point on the right edge follows the radius, the centre stays put.
    assert!(near(shape_point(&i, 0.0, 14.0, 10.0), (14.0, 10.0)));
    assert!(near(shape_point(&i, 0.5, 14.0, 10.0), (18.0, 10.0)));
    assert!(near(shape_point(&i, 1.0, 14.0, 10.0), (22.0, 10.0)));
    assert!(near(shape_point(&i, 1.0, 10.0, 10.0), (10.0, 10.0)));
    assert!(near(shape_point(&i, 1.0, 10.0, 14.0), (10.0, 22.0)));
}

#[test]
fn radius_combines_with_a_moving_centre_and_a_static_transform() {
    let i = imp(&doc(r##"<g transform="translate(100 0)"><circle cx="10" cy="10" r="2" fill="#00f">
        <animate attributeName="r" from="2" to="6" dur="1s" fill="freeze"/><animate attributeName="cx" from="10" to="30" dur="1s" fill="freeze"/></circle></g>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    // At the end the centre is (30,10) and the radius 6, all inside translate(100 0).
    assert!(near(shape_point(&i, 1.0, 12.0, 10.0), (136.0, 10.0)), "{:?}", shape_point(&i, 1.0, 12.0, 10.0));
    assert!(near(shape_point(&i, 1.0, 10.0, 10.0), (130.0, 10.0)));
}

#[test]
fn radius_is_refused_where_it_would_not_be_exact() {
    let stroked = imp(&doc(r##"<circle cx="10" cy="10" r="4" fill="#00f" stroke="#000" stroke-width="2"><animate attributeName="r" from="4" to="8" dur="1s"/></circle>"##));
    assert!(has_warning(&stroked, "stroke, which would scale"), "{:?}", stroked.warnings);
    let none = imp(&doc(r##"<circle cx="10" cy="10" r="0" fill="#00f"><animate attributeName="r" from="0" to="8" dur="1s"/></circle>"##));
    assert!(has_warning(&none, "no static r"), "{:?}", none.warnings);
    let ellipse = imp(&doc(r##"<ellipse cx="10" cy="10" rx="4" ry="2" fill="#00f"><animate attributeName="r" from="4" to="8" dur="1s"/></ellipse>"##));
    assert!(has_warning(&ellipse, "r on <ellipse>"), "{:?}", ellipse.warnings);
    for i in [&stroked, &none, &ellipse] {
        assert!(i.scene.animations.is_empty());
    }
}

#[test]
fn geometry_animations_do_not_depend_on_their_order_in_the_file() {
    let r_first = r##"<animate attributeName="r" from="2" to="6" dur="1s" fill="freeze"/><animate attributeName="cx" from="10" to="30" dur="1s" fill="freeze"/>"##;
    let cx_first = r##"<animate attributeName="cx" from="10" to="30" dur="1s" fill="freeze"/><animate attributeName="r" from="2" to="6" dur="1s" fill="freeze"/>"##;
    for body in [r_first, cx_first] {
        let i = imp(&doc(&format!(r##"<circle cx="10" cy="10" r="2" fill="#00f">{body}</circle>"##)));
        assert!(near(shape_point(&i, 1.0, 12.0, 10.0), (36.0, 10.0)), "{:?}", shape_point(&i, 1.0, 12.0, 10.0));
        assert!(near(shape_point(&i, 1.0, 10.0, 10.0), (30.0, 10.0)));
        assert!(near(shape_point(&i, 0.5, 10.0, 10.0), (20.0, 10.0)));
    }
}

#[test]
fn opacity_layers_go_outside_transform_wrappers() {
    let i = imp(&doc(r##"<rect width="4" height="4" fill="#000"><animateTransform attributeName="transform" type="translate" to="5 0" dur="1s" fill="freeze"/>
        <animate attributeName="opacity" from="1" to="0" dur="1s" fill="freeze"/></rect>"##));
    assert!(matches!(i.scene.elements[0], Element::Layer(_)), "the layer is outermost: {:?}", i.scene.elements[0]);
}

// ── CSS animation ────────────────────────────────────────────────────────────

const SPIN_CSS: &str = "<style>@keyframes spin { from { transform: rotate(0deg) } to { transform: rotate(360deg) } }
    .s { animation: spin 2s linear infinite; transform-origin: 20px 20px }</style>";

#[test]
fn a_css_spinner_becomes_a_looping_rotation_about_its_origin() {
    let i = imp(&doc(&format!(r##"{SPIN_CSS}<rect class="s" x="10" y="10" width="20" height="20" fill="#f00"/>"##)));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert_eq!(i.scene.duration, 2.0);
    assert_eq!(i.scene.loop_mode, msx_ast::LoopMode::Loop);
    let t = track(&i, msx_ast::AnimatedProperty::Rotate);
    assert_eq!(t.len(), 1);
    let keys: Vec<(f64, f64)> = t[0].keyframes.iter().map(|k| (k.time, k.value)).collect();
    assert_eq!(keys, vec![(0.0, 0.0), (2.0, 360.0)]);
    assert!(near(frame_point(&i, 0.5, 30.0, 20.0), (20.0, 30.0)));
    assert!(near(frame_point(&i, 1.0, 30.0, 20.0), (10.0, 20.0)));
    assert!(near(frame_point(&i, 2.5, 30.0, 20.0), (20.0, 30.0)), "the loop wraps");
}

#[test]
fn transform_box_fill_box_puts_the_origin_in_the_shape() {
    let css = "<style>@keyframes spin { to { transform: rotate(360deg) } } .s { animation: spin 2s linear infinite; transform-box: fill-box; transform-origin: center }</style>";
    let i = imp(&doc(&format!(r##"{css}<rect class="s" x="10" y="10" width="20" height="20" fill="#f00"/>"##)));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(near(frame_point(&i, 0.5, 30.0, 20.0), (20.0, 30.0)), "{:?}", frame_point(&i, 0.5, 30.0, 20.0));
    let g = imp(&doc(&format!(r##"{css}<g class="s"><rect x="0" y="0" width="10" height="10"/><rect x="30" y="10" width="10" height="10"/></g>"##)));
    // The box of the group is (0,0) to (40,20), so its centre is (20,10).
    assert!(near(frame_point(&g, 0.5, 10.0, 10.0), (20.0, 0.0)), "{:?}", frame_point(&g, 0.5, 10.0, 10.0));
}

#[test]
fn the_default_easing_is_ease_and_linear_is_honored() {
    let ease = imp(&doc(r##"<style>@keyframes k { to { transform: translate(100px, 0) } } .e { animation: k 1s infinite }</style><rect class="e" width="5" height="5"/>"##));
    let x = frame_point(&ease, 0.5, 0.0, 0.0).0;
    assert!(x > 75.0 && x < 85.0, "ease is ahead of linear at the middle: {x}");
    let lin = imp(&doc(r##"<style>@keyframes k { to { transform: translate(100px, 0) } } .e { animation: k 1s linear infinite }</style><rect class="e" width="5" height="5"/>"##));
    assert!(close(frame_point(&lin, 0.5, 0.0, 0.0).0, 50.0));
}

#[test]
fn alternate_runs_forward_then_back() {
    let i = imp(&doc(r##"<style>@keyframes k { from { transform: translate(0, 0) } to { transform: translate(100px, 0) } } .e { animation: k 1s linear infinite alternate }</style><rect class="e" width="5" height="5"/>"##));
    assert_eq!(i.scene.duration, 2.0);
    for (t, want) in [(0.0, 0.0), (0.5, 50.0), (1.0, 100.0), (1.5, 50.0), (2.0, 0.0), (2.5, 50.0)] {
        assert!(close(frame_point(&i, t, 0.0, 0.0).0, want), "t={t}: {:?}", frame_point(&i, t, 0.0, 0.0));
    }
}

#[test]
fn percent_stops_and_opacity_make_a_pulse() {
    let i = imp(&doc(r##"<style>@keyframes pulse { 0%, 100% { opacity: 1 } 50% { opacity: .2 } } .p { animation: pulse 2s linear infinite }</style><circle class="p" cx="5" cy="5" r="4"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(close(layer_opacity_at(&i, 0.0), 1.0) && close(layer_opacity_at(&i, 1.0), 0.2) && close(layer_opacity_at(&i, 0.5), 0.6) && close(layer_opacity_at(&i, 2.0), 1.0));
}

#[test]
fn a_delay_without_fill_shows_the_static_frame_and_forwards_holds_the_end() {
    let css = |fill: &str| format!(r##"<style>@keyframes k {{ from {{ transform: translate(5px, 0) }} to {{ transform: translate(15px, 0) }} }} .e {{ animation: k 1s linear 1s {fill} }}</style><rect class="e" width="5" height="5"/>"##);
    let plain = imp(&doc(&css("")));
    assert_eq!(plain.scene.loop_mode, msx_ast::LoopMode::Once);
    assert!(close(plain.scene.duration, 2.001), "one millisecond longer, so the last frame is the static one: {}", plain.scene.duration);
    assert!(close(frame_point(&plain, 0.5, 0.0, 0.0).0, 0.0), "before the delay: the static frame");
    assert!(close(frame_point(&plain, 1.5, 0.0, 0.0).0, 10.0));
    assert!((frame_point(&plain, 1.999, 0.0, 0.0).0 - 15.0).abs() < 0.02, "still at the end value a moment before");
    assert!(close(frame_point(&plain, 2.001, 0.0, 0.0).0, 0.0), "after the end it goes back");
    assert!(close(frame_point(&plain, 9.0, 0.0, 0.0).0, 0.0), "and stays there");
    let both = imp(&doc(&css("both")));
    assert!(close(frame_point(&both, 0.5, 0.0, 0.0).0, 5.0), "backwards holds the first keyframe: {:?}", frame_point(&both, 0.5, 0.0, 0.0));
    assert!(close(frame_point(&both, 2.0, 0.0, 0.0).0, 15.0), "forwards holds the last");
    assert!(close(frame_point(&both, 9.0, 0.0, 0.0).0, 15.0));
}

#[test]
fn a_negative_delay_starts_part_way_in() {
    let i = imp(&doc(&format!(r##"{}<rect class="s" x="10" y="10" width="20" height="20"/>"##, "<style>@keyframes spin { to { transform: rotate(360deg) } } .s { animation: spin 2s linear -0.5s infinite; transform-origin: 20px 20px }</style>")));
    assert!(near(frame_point(&i, 0.0, 30.0, 20.0), (20.0, 30.0)), "a quarter turn in at time 0");
    assert!(near(frame_point(&i, 0.5, 30.0, 20.0), (10.0, 20.0)));
}

#[test]
fn transform_functions_apply_in_list_order() {
    let tr_rot = imp(&doc(r##"<style>@keyframes k { to { transform: translate(10px, 0) rotate(90deg) } } .e { animation: k 1s linear forwards }</style><rect class="e" width="5" height="5"/>"##));
    assert!(i_near(&tr_rot, 1.0, (1.0, 0.0), (10.0, 1.0)), "translate then rotate: T.R");
    let rot_tr = imp(&doc(r##"<style>@keyframes k { to { transform: rotate(90deg) translate(10px, 0) } } .e { animation: k 1s linear forwards }</style><rect class="e" width="5" height="5"/>"##));
    assert!(i_near(&rot_tr, 1.0, (1.0, 0.0), (0.0, 11.0)), "rotate then translate: R.T");
    // Halfway the translation is 5 and the rotation 45 degrees.
    let (x, y) = frame_point(&tr_rot, 0.5, 0.0, 0.0);
    assert!(close(x, 5.0) && close(y, 0.0));
}

fn i_near(i: &Imported, t: f64, local: (f64, f64), want: (f64, f64)) -> bool {
    near(frame_point(i, t, local.0, local.1), want)
}

#[test]
fn two_css_animations_on_one_element_both_run() {
    let i = imp(&doc(r##"<style>@keyframes spin { to { transform: rotate(360deg) } } @keyframes fade { 50% { opacity: .2 } }
        .e { animation: spin 2s linear infinite, fade 1s linear infinite }</style><rect class="e" width="5" height="5"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert_eq!(i.scene.duration, 2.0);
    assert_eq!(track(&i, msx_ast::AnimatedProperty::Rotate).len(), 1);
    assert_eq!(track(&i, msx_ast::AnimatedProperty::Opacity).len(), 1);
    assert!(matches!(i.scene.elements[0], Element::Layer(_)), "the opacity layer is outermost");
}

#[test]
fn css_fill_opacity_follows_the_single_paint_rule() {
    let css = "<style>@keyframes f { 50% { fill-opacity: .2 } } .c { animation: f 1s linear infinite }</style>";
    let ok = imp(&doc(&format!(r##"{css}<circle class="c" cx="5" cy="5" r="4" fill="#00f"/>"##)));
    assert!(ok.warnings.is_empty(), "{:?}", ok.warnings);
    assert!(close(layer_opacity_at(&ok, 0.5), 0.2) && close(layer_opacity_at(&ok, 0.0), 1.0));
    let stroked = imp(&doc(&format!(r##"{css}<circle class="c" cx="5" cy="5" r="4" fill="#00f" stroke="#000"/>"##)));
    assert!(has_warning(&stroked, "also paints its stroke"), "{:?}", stroked.warnings);
    assert!(stroked.scene.animations.is_empty());
}

#[test]
fn css_animations_that_cannot_run_are_reported_and_left_static() {
    let cases = [
        (r##"@keyframes k { to { transform: skewX(20deg) } } .e { animation: k 1s }"##, "skewX() has no animation channel"),
        (r##"@keyframes k { to { transform: translate(5px, 0) } } .e { animation: k 1s steps(4) }"##, "step easing"),
        (r##"@keyframes k { to { transform: translate(5px, 0) } } .e { animation: ghost 1s }"##, "@keyframes ghost is not defined"),
        (r##"@keyframes k { to { transform: translate(5px, 0) } } .e { animation: k 1s paused }"##, "paused"),
        (r##"@keyframes k { to { transform: translate(5px, 0) } } .e { animation: k 0s }"##, "0 or too short"),
        (r##"@keyframes k { from { transform: rotate(0deg) } to { transform: scale(2) } } .e { animation: k 1s }"##, "different functions"),
        (r##"@keyframes k { to { transform: translate(50%, 0) } } .e { animation: k 1s }"##, "is not a length in px"),
        (r##"@keyframes k { to { fill: red } } .e { animation: k 1s }"##, "keyframe property fill"),
    ];
    for (css, needle) in cases {
        let i = imp(&doc(&format!(r##"<style>{css}</style><rect class="e" width="5" height="5"/>"##)));
        assert!(has_warning(&i, needle), "no warning for {needle}: {:?}", i.warnings);
        assert!(i.scene.animations.is_empty(), "{needle}");
        assert!(matches!(i.scene.elements[0], Element::Rect(_)), "{needle}: the static frame stays");
    }
    let none = imp(&doc(r##"<style>.e { animation: k 1s }</style><rect class="e" width="5" height="5"/>"##));
    assert!(has_warning(&none, "defines no @keyframes"), "{:?}", none.warnings);
}

#[test]
fn css_animation_survives_write_and_parse() {
    let i = imp(&doc(&format!(r##"{SPIN_CSS}<rect class="s" x="10" y="10" width="20" height="20" fill="#f00"/>
        <style>@keyframes b {{ from {{ transform: translate(0, 0) }} 50% {{ transform: translate(30px, 0) }} to {{ transform: translate(0, 0) }} }} .b {{ animation: b 1.5s ease-in-out infinite alternate }}</style>
        <circle class="b" cx="5" cy="50" r="4" fill="#00f"/>"##)));
    assert!(i.scene.animations.len() >= 2, "{:?}", i.warnings);
    let text = msx_parser::scene_to_msx(&i.scene).expect("animated scenes are writable");
    let back = msx_parser::parse_scene(&text).unwrap_or_else(|e| panic!("written source must parse: {e}"));
    assert_eq!(back.duration, i.scene.duration);
    for t in [0.0, 0.7, 1.3, 2.9] {
        assert_eq!(format!("{:?}", msx_anim::resolve_at_time(&back, t).elements), format!("{:?}", msx_anim::resolve_at_time(&i.scene, t).elements), "frame at {t}");
    }
}

// ── context-fill and context-stroke ──────────────────────────────────────────

/// Every circle in the tree, in order, with its fill and stroke.
fn circles(el: &Element, out: &mut Vec<(Option<Paint>, Option<Paint>)>) {
    match el {
        Element::Group(g) => g.children.iter().for_each(|c| circles(c, out)),
        Element::Layer(l) => l.children.iter().for_each(|c| circles(c, out)),
        Element::Circle(c) => out.push((c.style.fill.clone(), c.style.stroke.clone())),
        _ => {}
    }
}

fn all_circles(i: &Imported) -> Vec<(Option<Paint>, Option<Paint>)> {
    let mut v = Vec::new();
    i.scene.elements.iter().for_each(|e| circles(e, &mut v));
    v
}

fn paint_rgb(p: &Option<Paint>) -> (u8, u8, u8) {
    match p {
        Some(Paint::Color(c)) => (c.r, c.g, c.b),
        other => panic!("expected a color, got {other:?}"),
    }
}

#[test]
fn context_paint_in_a_use_takes_the_paint_of_the_use() {
    let i = imp(&doc(r##"<defs><circle id="p" r="3" fill="context-stroke" stroke="context-fill" stroke-width="1"/></defs>
        <use href="#p" fill="#ff0000" stroke="#0000ff"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let c = all_circles(&i);
    assert_eq!(c.len(), 1);
    assert_eq!(paint_rgb(&c[0].0), (0, 0, 255), "fill is the use's stroke");
    assert_eq!(paint_rgb(&c[0].1), (255, 0, 0), "stroke is the use's fill");
}

#[test]
fn each_use_has_its_own_context() {
    let i = imp(&doc(r##"<defs><circle id="p" r="3" fill="context-fill"/></defs>
        <use href="#p" fill="#ff0000"/><use href="#p" fill="#00ff00" x="10"/>"##));
    let c = all_circles(&i);
    assert_eq!((paint_rgb(&c[0].0), paint_rgb(&c[1].0)), ((255, 0, 0), (0, 255, 0)));
}

#[test]
fn a_context_paint_on_a_nested_use_refers_to_the_enclosing_context() {
    let i = imp(&doc(r##"<defs><circle id="p" r="3" fill="context-fill"/><g id="outer"><use href="#p" fill="context-stroke"/></g></defs>
        <use href="#outer" fill="#ff0000" stroke="#00ff00"/>"##));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let c = all_circles(&i);
    assert_eq!(paint_rgb(&c[0].0), (0, 255, 0), "the inner use's fill is the outer use's stroke, and the circle takes that");
}

#[test]
fn context_paint_takes_the_opacity_of_the_context() {
    let i = imp(&doc(r##"<defs><circle id="p" r="3" fill="context-fill"/></defs><use href="#p" fill="#ff0000" fill-opacity=".5"/>"##));
    let Some(Paint::Color(col)) = &all_circles(&i)[0].0 else { panic!() };
    assert!(col.a == 127 || col.a == 128, "{col:?}");
}

const CTX_MARKER: &str = r##"<defs><marker id="m" markerWidth="4" markerHeight="4" refX="2" refY="2" markerUnits="userSpaceOnUse"><circle cx="2" cy="2" r="2" fill="context-stroke"/></marker></defs>"##;

#[test]
fn context_paint_in_a_marker_takes_the_paint_of_the_marked_shape() {
    let i = imp(&doc(&format!(r##"{CTX_MARKER}<path d="M0 0 L 20 0" stroke="#00ff00" marker-end="url(#m)"/>"##)));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let c = all_circles(&i);
    assert_eq!(c.len(), 1);
    assert_eq!(paint_rgb(&c[0].0), (0, 255, 0));
}

#[test]
fn one_marker_is_built_for_each_context_it_is_used_in() {
    let i = imp(&doc(&format!(
        r##"{CTX_MARKER}<path d="M0 0 L 20 0" stroke="#00ff00" marker-end="url(#m)"/><path d="M0 10 L 20 10" stroke="#ff0000" marker-end="url(#m)"/><path d="M0 20 L 20 20" stroke="#00ff00" marker-end="url(#m)"/>"##
    )));
    let c = all_circles(&i);
    let colors: Vec<_> = c.iter().map(|x| paint_rgb(&x.0)).collect();
    assert_eq!(colors, vec![(0, 255, 0), (255, 0, 0), (0, 255, 0)]);
}

#[test]
fn a_marker_without_context_paint_is_still_shared() {
    let i = imp(&doc(
        r##"<defs><marker id="m" markerWidth="4" markerHeight="4" refX="2" refY="2" markerUnits="userSpaceOnUse"><circle cx="2" cy="2" r="2" fill="#123456"/></marker></defs>
        <path d="M0 0 L 20 0" stroke="#00ff00" marker-end="url(#m)"/><path d="M0 10 L 20 10" stroke="#ff0000" marker-end="url(#m)"/>"##,
    ));
    let colors: Vec<_> = all_circles(&i).iter().map(|x| paint_rgb(&x.0)).collect();
    assert_eq!(colors, vec![(0x12, 0x34, 0x56), (0x12, 0x34, 0x56)]);
    assert_eq!(i.warnings.len(), 0);
}

#[test]
fn a_marker_in_a_use_sees_the_marked_shape_not_the_use() {
    let i = imp(&doc(&format!(
        r##"{CTX_MARKER}<defs><path id="p" d="M0 0 L 20 0" stroke="#0000ff" marker-end="url(#m)"/></defs><use href="#p" stroke="#ff0000"/>"##
    )));
    // The path sets its own stroke, which is what the marker is for.
    assert_eq!(paint_rgb(&all_circles(&i)[0].0), (0, 0, 255));
}

#[test]
fn context_paint_without_a_context_is_reported_and_painted_as_none() {
    let i = imp(&doc(r##"<circle r="3" fill="context-fill"/>"##));
    assert!(has_warning(&i, "no context"), "{:?}", i.warnings);
    assert_eq!(all_circles(&i)[0].0, Some(Paint::None));
}

// ── SVG images ───────────────────────────────────────────────────────────────

fn svg_uri(svg: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg))
}

const INNER_50: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><rect width="50" height="50" fill="#f00"/></svg>"##;
const INNER_WIDE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="25"><rect width="50" height="25" fill="#f00"/></svg>"##;

fn image_doc(inner: &str, attrs: &str) -> Imported {
    imp(&doc(&format!(r##"<image {attrs} href="{}"/>"##, svg_uri(inner))))
}

#[test]
fn an_svg_image_is_imported_and_placed_in_its_box() {
    let i = image_doc(INNER_50, r#"x="10" y="20" width="100" height="100""#);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    // The 50 by 50 canvas is scaled to 100 by 100 and moved to (10,20).
    assert!(near(shape_point(&i, 0.0, 0.0, 0.0), (10.0, 20.0)));
    assert!(near(shape_point(&i, 0.0, 50.0, 50.0), (110.0, 120.0)));
    assert!(near(shape_point(&i, 0.0, 25.0, 25.0), (60.0, 70.0)));
}

#[test]
fn a_missing_size_comes_from_the_inner_canvas() {
    let i = image_doc(INNER_WIDE, r#"x="5" y="5""#);
    assert!(near(shape_point(&i, 0.0, 50.0, 25.0), (55.0, 30.0)), "natural size 50 by 25");
    let only_w = image_doc(INNER_WIDE, r#"width="100""#);
    assert!(near(shape_point(&only_w, 0.0, 50.0, 25.0), (100.0, 50.0)), "height follows the ratio");
}

#[test]
fn preserve_aspect_ratio_applies_to_svg_images() {
    // A 50 by 25 canvas in a 100 by 100 box.
    let meet = image_doc(INNER_WIDE, r#"width="100" height="100""#);
    assert!(near(shape_point(&meet, 0.0, 0.0, 0.0), (0.0, 25.0)), "centred: {:?}", shape_point(&meet, 0.0, 0.0, 0.0));
    assert!(near(shape_point(&meet, 0.0, 50.0, 25.0), (100.0, 75.0)));
    let min = image_doc(INNER_WIDE, r#"width="100" height="100" preserveAspectRatio="xMinYMin meet""#);
    assert!(near(shape_point(&min, 0.0, 0.0, 0.0), (0.0, 0.0)));
    let max = image_doc(INNER_WIDE, r#"width="100" height="100" preserveAspectRatio="xMaxYMax meet""#);
    assert!(near(shape_point(&max, 0.0, 0.0, 0.0), (0.0, 50.0)));
    let none = image_doc(INNER_WIDE, r#"width="100" height="100" preserveAspectRatio="none""#);
    assert!(near(shape_point(&none, 0.0, 50.0, 25.0), (100.0, 100.0)), "stretched to the box");
    let slice = image_doc(INNER_WIDE, r#"width="100" height="100" preserveAspectRatio="xMidYMid slice""#);
    assert!(near(shape_point(&slice, 0.0, 0.0, 0.0), (-50.0, 0.0)), "scaled by 4 and centred: {:?}", shape_point(&slice, 0.0, 0.0, 0.0));
    assert!(has_clip(&slice.scene.elements[0]), "the part outside the box is cut off");
    assert!(!has_clip(&meet.scene.elements[0]), "a meet that fits needs no clip");
}

fn has_clip(el: &Element) -> bool {
    match el {
        Element::Layer(l) => l.clip_path.is_some() || l.children.iter().any(has_clip),
        Element::Group(g) => g.children.iter().any(has_clip),
        _ => false,
    }
}

#[test]
fn what_an_svg_image_draws_outside_its_canvas_is_cut_off() {
    let inner = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect x="-20" width="60" height="10" fill="#f00"/></svg>"##;
    let i = image_doc(inner, r#"width="10" height="10""#);
    assert!(has_clip(&i.scene.elements[0]));
    let contained = image_doc(INNER_50, r#"width="50" height="50""#);
    assert!(!has_clip(&contained.scene.elements[0]), "nothing to cut off, so no clip");
}

#[test]
fn opacity_and_transform_on_an_svg_image_apply_to_the_whole_picture() {
    let i = image_doc(INNER_50, r#"width="50" height="50" opacity=".5" transform="translate(100 0)""#);
    let Element::Layer(l) = &i.scene.elements[0] else { panic!("{:?}", i.scene.elements[0]) };
    assert_eq!(l.opacity, 0.5);
    assert!(near(shape_point(&i, 0.0, 0.0, 0.0), (100.0, 0.0)));
}

#[test]
fn an_svg_image_keeps_its_ids_styles_and_gradients_to_itself() {
    let inner = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><style>rect { fill: #00ff00 }</style><defs><linearGradient id="g"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs><rect id="r" width="10" height="10"/><circle id="c" cx="5" cy="5" r="2" fill="url(#g)"/></svg>"##;
    let i = imp(&doc(&format!(
        r##"<style>rect {{ fill: #ff0000 }}</style><linearGradient id="g"><stop offset="0" stop-color="#000000"/><stop offset="1" stop-color="#ffffff"/></linearGradient>
        <image width="10" height="10" href="{}"/><rect id="r" width="5" height="5" style="fill: url(#g)"/>"##,
        svg_uri(inner)
    )));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    // The inner rect and circle sit directly in the scene, then the outer rect.
    let Element::Rect(inner_rect) = &i.scene.elements[0] else { panic!("{:?}", i.scene.elements[0]) };
    assert_eq!(inner_rect.style.fill, Some(Paint::Color(Color::rgb(0, 255, 0))), "the image's own style applies inside it");
    let Element::Circle(inner_circle) = &i.scene.elements[1] else { panic!("{:?}", i.scene.elements[1]) };
    let outer = rect(&i.scene.elements[2]);
    let (Some(Paint::Ref(inner_ref)), Some(Paint::Ref(outer_ref))) = (&inner_circle.style.fill, &outer.style.fill) else { panic!("{:?} {:?}", inner_circle.style.fill, outer.style.fill) };
    assert_ne!(inner_ref, outer_ref, "both documents call their gradient g, so one of them got another id");
    let stops_of = |reference: &str| -> Vec<u8> {
        let id = reference.trim_start_matches("url(#").trim_end_matches(')');
        match i.scene.defs.iter().find(|d| d.id() == id) {
            Some(msx_ast::Def::LinearGradient(g)) => g.stops.iter().map(|s| s.color.r).collect(),
            other => panic!("no gradient {id}: {other:?}"),
        }
    };
    assert_eq!(stops_of(inner_ref), vec![255, 0], "the circle got the inner gradient, red to blue");
    assert_eq!(stops_of(outer_ref), vec![0, 255], "the outer rect still has its own, black to white");
    assert_eq!(i.scene.defs.len(), 2);
    let ids: Vec<&str> = i.scene.defs.iter().map(|d| d.id()).collect();
    assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), ids.len(), "{ids:?}");
}

#[test]
fn warnings_of_an_svg_image_are_reported_with_the_image() {
    let inner = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="5" height="5"><animate attributeName="fill" from="red" to="blue" dur="1s"/></rect></svg>"##;
    let i = image_doc(inner, r#"id="pic" width="10" height="10""#);
    assert!(i.warnings.iter().any(|w| w.contains("<image id=\"pic\">") && w.contains("<animate>")), "{:?}", i.warnings);
}

#[test]
fn an_svg_image_can_hold_a_bitmap() {
    let inner = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><image width="20" height="20" href="{}"/></svg>"##, png_uri(4, 4));
    let i = image_doc(&inner, r#"width="40" height="40""#);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    fn find_image(el: &Element) -> Option<&msx_ast::Image> {
        match el {
            Element::Image(im) => Some(im),
            Element::Group(g) => g.children.iter().find_map(find_image),
            Element::Layer(l) => l.children.iter().find_map(find_image),
            _ => None,
        }
    }
    assert!(find_image(&i.scene.elements[0]).is_some());
}

#[test]
fn a_document_that_cannot_be_read_is_reported_and_skipped() {
    let i = imp(&doc(&format!(r##"<image width="10" height="10" href="{}"/><rect width="5" height="5"/>"##, svg_uri("<svg><oops></svg>"))));
    assert!(has_warning(&i, "could not be read"), "{:?}", i.warnings);
    assert_eq!(i.scene.elements.len(), 1, "only the rect is left");
    let not_svg = imp(&doc(&format!(r##"<image width="10" height="10" href="{}"/>"##, svg_uri("<html xmlns='http://www.w3.org/1999/xhtml'/>"))));
    assert!(not_svg.scene.elements.is_empty());
}

static IMG_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let n = IMG_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("msx-svgimg-{}-{tag}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn an_external_svg_image_is_read_from_the_base_directory_and_reads_its_own_files_from_its_folder() {
    let dir = temp_dir("ext");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/pic.png"), raster::tests_support::fake_png(4, 4)).unwrap();
    std::fs::write(dir.join("sub/inner.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="#f00"/><image width="20" height="20" href="pic.png"/></svg>"##).unwrap();
    let outer = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image x="10" y="10" width="40" height="40" href="sub/inner.svg"/></svg>"##;
    let i = import_svg_with(outer, &ImportOptions { base_dir: Some(dir.clone()) }).unwrap();
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(near(shape_point(&i, 0.0, 20.0, 20.0), (50.0, 50.0)));
    fn embedded(el: &Element) -> bool {
        match el {
            Element::Image(im) => matches!(im.source, msx_ast::MediaSource::Embedded(_)),
            Element::Group(g) => g.children.iter().any(embedded),
            Element::Layer(l) => l.children.iter().any(embedded),
            _ => false,
        }
    }
    assert!(i.scene.elements.iter().any(embedded), "the bitmap next to the inner file was read and embedded");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_svg_image_that_embeds_itself_is_skipped_at_the_first_repeat() {
    let dir = temp_dir("rec");
    std::fs::write(dir.join("a.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/><image width="10" height="10" href="a.svg"/></svg>"##).unwrap();
    let outer = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="10" height="10" href="a.svg"/></svg>"##;
    let i = import_svg_with(outer, &ImportOptions { base_dir: Some(dir.clone()) }).unwrap();
    assert!(i.warnings.iter().any(|w| w.contains("already contains it")), "{:?}", i.warnings);
    assert_eq!(i.scene.elements.len(), 1, "a.svg is drawn once, without the copy of itself");
    // The document that is the top level is part of the chain too.
    std::fs::write(dir.join("top.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="10" height="10" href="b.svg"/></svg>"##).unwrap();
    std::fs::write(dir.join("b.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="4" height="4"/><image width="10" height="10" href="top.svg"/></svg>"##).unwrap();
    let top = std::fs::read_to_string(dir.join("top.svg")).unwrap();
    let j = import_svg_with(&top, &ImportOptions { base_dir: Some(dir.clone()) }).unwrap();
    assert!(j.warnings.iter().any(|w| w.contains("already contains it")), "{:?}", j.warnings);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_long_chain_of_different_svg_images_stops_at_the_depth_limit() {
    let dir = temp_dir("chain");
    for n in 0..8 {
        let next = if n < 7 { format!(r##"<image width="10" height="10" href="f{}.svg"/>"##, n + 1) } else { String::new() };
        std::fs::write(dir.join(format!("f{n}.svg")), format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="{}" height="1"/>{next}</svg>"##, n + 1)).unwrap();
    }
    let outer = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="10" height="10" href="f0.svg"/></svg>"##;
    let i = import_svg_with(outer, &ImportOptions { base_dir: Some(dir.clone()) }).unwrap();
    assert!(i.warnings.iter().any(|w| w.contains("nested") && w.contains("deep")), "{:?}", i.warnings);
    assert_eq!(i.scene.elements.len(), MAX_IMAGE_DEPTH, "{:?}", i.scene.elements.len());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_data_uri_svg_has_no_directory_to_read_files_from() {
    let dir = temp_dir("data");
    std::fs::write(dir.join("pic.png"), raster::tests_support::fake_png(4, 4)).unwrap();
    let inner = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="10" height="10" href="pic.png"/></svg>"##;
    let outer = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="10" height="10" href="{}"/></svg>"##, svg_uri(inner));
    let i = import_svg_with(&outer, &ImportOptions { base_dir: Some(dir.clone()) }).unwrap();
    assert!(i.warnings.iter().any(|w| w.contains("pic.png") && w.contains("no base directory")), "{:?}", i.warnings);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_inner_gradient_is_renamed_when_the_outer_id_is_already_taken() {
    let inner = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><defs><linearGradient id="g"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs><circle cx="5" cy="5" r="4" fill="url(#g)"/></svg>"##;
    let i = imp(&doc(&format!(
        r##"<linearGradient id="g"><stop offset="0" stop-color="#000000"/><stop offset="1" stop-color="#ffffff"/></linearGradient>
        <rect id="r" width="5" height="5" fill="url(#g)"/><image width="10" height="10" href="{}"/>"##,
        svg_uri(inner)
    )));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let outer = rect(&i.scene.elements[0]);
    let Element::Circle(c) = &i.scene.elements[1] else { panic!("{:?}", i.scene.elements[1]) };
    let (Some(Paint::Ref(o)), Some(Paint::Ref(n))) = (&outer.style.fill, &c.style.fill) else { panic!() };
    assert_eq!(o, "url(#g)");
    assert_ne!(n, "url(#g)", "the inner gradient was moved to a free id: {n}");
    let first_stop = |reference: &str| match i.scene.defs.iter().find(|d| d.id() == reference.trim_start_matches("url(#").trim_end_matches(')')) {
        Some(msx_ast::Def::LinearGradient(g)) => g.stops[0].color.r,
        other => panic!("{other:?}"),
    };
    assert_eq!((first_stop(o), first_stop(n)), (0, 255));
}
