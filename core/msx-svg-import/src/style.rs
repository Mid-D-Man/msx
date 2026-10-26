// core/msx-svg-import/src/style.rs
//! The inherited part of the SVG style cascade.
//!
//! MSX groups do not pass style down to their children on the CPU and GPU
//! renderers, so the importer resolves inheritance itself and writes
//! explicit values on every leaf. [`Inh`] holds the computed values of the
//! inherited properties at one point of the tree, [`apply`] updates it from
//! one element's specified properties.

use std::collections::HashMap;

use msx_ast::{Color, FillRule, FontWeight, LineCap, LineJoin, TextAnchor};

use crate::parse::{parse_color, parse_length, parse_number};

/// Specified property values of one element, after the cascade of
/// presentation attributes, stylesheet rules and the `style` attribute.
pub type Props = HashMap<String, String>;

/// Attribute names that are presentation properties.
pub const PRESENTATION: &[&str] = &[
    "fill", "fill-opacity", "fill-rule", "clip-rule", "overflow", "mask-type", "stroke", "stroke-width", "stroke-opacity", "stroke-linecap", "stroke-linejoin",
    "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset", "opacity", "display", "visibility", "color", "font-size",
    "font-family", "font-weight", "text-anchor", "dominant-baseline", "stop-color", "stop-opacity", "clip-path", "mask",
    "filter", "paint-order", "transform-origin", "transform-box", "marker-start", "marker-mid",
    "marker-end", "text-decoration", "letter-spacing", "word-spacing", "font-style", "writing-mode", "vector-effect",
];

#[derive(Debug, Clone, PartialEq)]
pub enum PaintSpec {
    None,
    /// `currentColor`, resolved against the element's `color` when used.
    Current,
    Color(Color),
    /// `context-fill` and `context-stroke`: the paint of the element that a
    /// `use` or a marker is drawn for.
    ContextFill,
    ContextStroke,
    /// `url(#id)` with an optional fallback: `Some(None)` is a fallback of
    /// `none`. `fallback_current` is a fallback of `currentColor`.
    Url { id: String, fallback: Option<Option<Color>>, fallback_current: bool },
}

/// Computed values of the inherited properties.
#[derive(Debug, Clone)]
pub struct Inh {
    pub fill: PaintSpec,
    pub fill_opacity: f64,
    pub fill_rule: FillRule,
    /// `clip-rule`, read instead of `fill-rule` inside a clip path.
    pub clip_rule: FillRule,
    pub stroke: PaintSpec,
    pub stroke_opacity: f64,
    pub stroke_width: f64,
    pub linecap: LineCap,
    pub linejoin: LineJoin,
    pub miterlimit: f64,
    pub dasharray: Vec<f64>,
    pub dashoffset: f64,
    pub font_size: f64,
    pub font_family: Option<String>,
    pub font_weight: Option<FontWeight>,
    pub text_anchor: Option<TextAnchor>,
    pub dominant_baseline: Option<String>,
    pub hidden: bool,
    pub color: Color,
    /// `paint-order` puts the stroke before the fill.
    pub stroke_first: bool,
    /// `marker-start`, `marker-mid` and `marker-end` as written (`url(#id)`),
    /// `None` for `none`. They are inherited, and the `marker` shorthand in CSS
    /// sets all three.
    pub marker_start: Option<String>,
    pub marker_mid: Option<String>,
    pub marker_end: Option<String>,
}

impl Default for Inh {
    /// The SVG initial values.
    fn default() -> Self {
        Inh {
            fill: PaintSpec::Color(Color::BLACK),
            fill_opacity: 1.0,
            fill_rule: FillRule::NonZero,
            clip_rule: FillRule::NonZero,
            stroke: PaintSpec::None,
            stroke_opacity: 1.0,
            stroke_width: 1.0,
            linecap: LineCap::Butt,
            linejoin: LineJoin::Miter,
            miterlimit: 4.0,
            dasharray: Vec::new(),
            dashoffset: 0.0,
            font_size: 16.0,
            font_family: None,
            font_weight: None,
            text_anchor: None,
            dominant_baseline: None,
            hidden: false,
            color: Color::BLACK,
            stroke_first: false,
            marker_start: None,
            marker_mid: None,
            marker_end: None,
        }
    }
}

pub fn parse_paint(v: &str) -> Option<PaintSpec> {
    let t = v.trim();
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "none" => return Some(PaintSpec::None),
        "currentcolor" => return Some(PaintSpec::Current),
        "context-fill" => return Some(PaintSpec::ContextFill),
        "context-stroke" => return Some(PaintSpec::ContextStroke),
        _ => {}
    }
    if lower.starts_with("url(") {
        let close = t.find(')')?;
        let inner = t[4..close].trim().trim_matches(|c| c == '"' || c == '\'').trim();
        let id = inner.strip_prefix('#')?.to_string();
        let rest = t[close + 1..].trim();
        let current = rest.eq_ignore_ascii_case("currentcolor");
        let fallback = if rest.is_empty() || current {
            None
        } else if rest.eq_ignore_ascii_case("none") {
            Some(None)
        } else {
            // Anything else after the reference must be a color, or the
            // whole value is invalid (an icc-color hint is not read).
            Some(Some(parse_color(rest)?))
        };
        return Some(PaintSpec::Url { id, fallback, fallback_current: current });
    }
    parse_color(t).map(PaintSpec::Color)
}

fn parse_opacity(v: &str) -> Option<f64> {
    let t = v.trim();
    let n = match t.strip_suffix('%') {
        Some(p) => parse_number(p)? / 100.0,
        None => parse_number(t)?,
    };
    Some(n.clamp(0.0, 1.0))
}

fn parse_font_size(v: &str, parent: f64, _vp: (f64, f64)) -> Option<f64> {
    let kw = match v.trim() {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "larger" => Some(parent * 1.2),
        "smaller" => Some(parent / 1.2),
        _ => None,
    };
    if kw.is_some() {
        return kw;
    }
    let l = parse_length(v)?;
    // em, ex and % are relative to the parent's font size.
    let px = l.to_px(parent, parent);
    (px >= 0.0).then_some(px)
}

fn parse_weight(v: &str) -> Option<FontWeight> {
    match v.trim() {
        "normal" => Some(FontWeight::Normal),
        "bold" | "bolder" => Some(FontWeight::Bold),
        "lighter" => Some(FontWeight::Normal),
        other => {
            let n = parse_number(other)?;
            if !(1.0..=1000.0).contains(&n) {
                return None;
            }
            let n = n as u16;
            Some(match n {
                400 => FontWeight::Normal,
                700 => FontWeight::Bold,
                _ => FontWeight::Numeric(n),
            })
        }
    }
}

/// Normalized viewport diagonal, the reference for plain `%` lengths.
pub fn diagonal(vp: (f64, f64)) -> f64 {
    ((vp.0 * vp.0 + vp.1 * vp.1) / 2.0).sqrt()
}

fn parse_dasharray(v: &str, font_size: f64, vp: (f64, f64)) -> Option<Vec<f64>> {
    let t = v.trim();
    if t.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for tok in t.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()) {
        let l = parse_length(tok)?;
        let px = l.to_px(font_size, diagonal(vp));
        if px < 0.0 {
            return None;
        }
        out.push(px);
    }
    if out.is_empty() || out.iter().sum::<f64>() <= 0.0 {
        return Some(Vec::new());
    }
    if out.len() % 2 == 1 {
        let again = out.clone();
        out.extend(again);
    }
    Some(out)
}

/// Update `inh` from one element's specified properties. `inherit` keeps
/// the parent's value, and an invalid value is ignored, as in CSS.
pub fn apply(inh: &mut Inh, props: &Props, vp: (f64, f64)) {
    let get = |k: &str| props.get(k).map(|v| v.trim()).filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("inherit"));

    // Color and font size first: other values can be relative to them.
    if let Some(c) = get("color").and_then(parse_color) {
        inh.color = c;
    }
    if let Some(px) = get("font-size").and_then(|v| parse_font_size(v, inh.font_size, vp)) {
        inh.font_size = px;
    }
    if let Some(v) = get("font-family") {
        inh.font_family = Some(v.to_string());
    }
    if let Some(w) = get("font-weight").and_then(parse_weight) {
        inh.font_weight = Some(w);
    }
    match get("text-anchor") {
        Some("start") => inh.text_anchor = Some(TextAnchor::Start),
        Some("middle") => inh.text_anchor = Some(TextAnchor::Middle),
        Some("end") => inh.text_anchor = Some(TextAnchor::End),
        _ => {}
    }
    if let Some(v) = get("dominant-baseline") {
        inh.dominant_baseline = (v != "auto" && v != "alphabetic").then(|| v.to_string());
    }

    if let Some(p) = get("fill").and_then(parse_paint) {
        inh.fill = p;
    }
    if let Some(o) = get("fill-opacity").and_then(parse_opacity) {
        inh.fill_opacity = o;
    }
    match get("fill-rule") {
        Some("evenodd") => inh.fill_rule = FillRule::EvenOdd,
        Some("nonzero") => inh.fill_rule = FillRule::NonZero,
        _ => {}
    }
    let marker = |v: &str| (!v.trim().is_empty() && v.trim() != "none").then(|| v.trim().to_string());
    if let Some(v) = get("marker") {
        inh.marker_start = marker(v);
        inh.marker_mid = marker(v);
        inh.marker_end = marker(v);
    }
    if let Some(v) = get("marker-start") {
        inh.marker_start = marker(v);
    }
    if let Some(v) = get("marker-mid") {
        inh.marker_mid = marker(v);
    }
    if let Some(v) = get("marker-end") {
        inh.marker_end = marker(v);
    }
    match get("clip-rule") {
        Some("evenodd") => inh.clip_rule = FillRule::EvenOdd,
        Some("nonzero") => inh.clip_rule = FillRule::NonZero,
        _ => {}
    }

    if let Some(p) = get("stroke").and_then(parse_paint) {
        inh.stroke = p;
    }
    if let Some(o) = get("stroke-opacity").and_then(parse_opacity) {
        inh.stroke_opacity = o;
    }
    if let Some(l) = get("stroke-width").and_then(parse_length) {
        let px = l.to_px(inh.font_size, diagonal(vp));
        if px >= 0.0 {
            inh.stroke_width = px;
        }
    }
    match get("stroke-linecap") {
        Some("butt") => inh.linecap = LineCap::Butt,
        Some("round") => inh.linecap = LineCap::Round,
        Some("square") => inh.linecap = LineCap::Square,
        _ => {}
    }
    match get("stroke-linejoin") {
        Some("miter") | Some("miter-clip") => inh.linejoin = LineJoin::Miter,
        Some("round") => inh.linejoin = LineJoin::Round,
        Some("bevel") => inh.linejoin = LineJoin::Bevel,
        _ => {}
    }
    if let Some(m) = get("stroke-miterlimit").and_then(parse_number) {
        if m >= 1.0 {
            inh.miterlimit = m;
        }
    }
    if let Some(d) = get("stroke-dasharray").and_then(|v| parse_dasharray(v, inh.font_size, vp)) {
        inh.dasharray = d;
    }
    if let Some(l) = get("stroke-dashoffset").and_then(parse_length) {
        inh.dashoffset = l.to_px(inh.font_size, diagonal(vp));
    }
    if let Some(v) = get("paint-order") {
        if let Some(stroke_first) = parse_paint_order(v) {
            inh.stroke_first = stroke_first;
        }
    }
    match get("visibility") {
        Some("hidden") | Some("collapse") => inh.hidden = true,
        Some("visible") => inh.hidden = false,
        _ => {}
    }
}

/// Whether `paint-order` draws the stroke before the fill. `None` for an
/// invalid value, which CSS says to ignore.
fn parse_paint_order(v: &str) -> Option<bool> {
    let t = v.trim();
    if t == "normal" {
        return Some(false);
    }
    let mut order: Vec<&str> = Vec::new();
    for tok in t.split_whitespace() {
        if !matches!(tok, "fill" | "stroke" | "markers") || order.contains(&tok) {
            return None;
        }
        order.push(tok);
    }
    for default in ["fill", "stroke", "markers"] {
        if !order.contains(&default) {
            order.push(default);
        }
    }
    let at = |name: &str| order.iter().position(|o| *o == name);
    Some(at("stroke") < at("fill"))
}

/// `opacity` is not inherited, so it is read per element.
pub fn element_opacity(props: &Props) -> f64 {
    props.get("opacity").and_then(|v| parse_opacity(v)).unwrap_or(1.0)
}

/// Scale a color's alpha by `factor` in 0..1.
pub fn with_alpha(c: Color, factor: f64) -> Color {
    if factor >= 1.0 {
        return c;
    }
    Color::rgba(c.r, c.g, c.b, (c.a as f64 * factor.max(0.0)).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> Props {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn paints_parse_every_form() {
        assert_eq!(parse_paint("none"), Some(PaintSpec::None));
        assert_eq!(parse_paint("currentColor"), Some(PaintSpec::Current));
        assert_eq!(parse_paint("#f00"), Some(PaintSpec::Color(Color::rgb(255, 0, 0))));
        assert_eq!(parse_paint("url(#g)"), Some(PaintSpec::Url { id: "g".into(), fallback: None, fallback_current: false }));
        assert_eq!(parse_paint("url('#g') red"), Some(PaintSpec::Url { id: "g".into(), fallback: Some(Some(Color::rgb(255, 0, 0))), fallback_current: false }));
        assert_eq!(parse_paint("url(#g) none"), Some(PaintSpec::Url { id: "g".into(), fallback: Some(None), fallback_current: false }));
        assert_eq!(parse_paint("url(#g) currentColor"), Some(PaintSpec::Url { id: "g".into(), fallback: None, fallback_current: true }));
        assert_eq!(parse_paint("url(nothash)"), None);
        assert_eq!(parse_paint("garbage"), None);
    }

    #[test]
    fn inheritance_keeps_the_parent_value_for_inherit_and_junk() {
        let mut inh = Inh::default();
        apply(&mut inh, &props(&[("fill", "red"), ("stroke", "blue"), ("stroke-width", "3")]), (100.0, 100.0));
        apply(&mut inh, &props(&[("fill", "inherit"), ("stroke", "not-a-paint"), ("stroke-width", "-4")]), (100.0, 100.0));
        assert_eq!(inh.fill, PaintSpec::Color(Color::rgb(255, 0, 0)));
        assert_eq!(inh.stroke, PaintSpec::Color(Color::rgb(0, 0, 255)));
        assert_eq!(inh.stroke_width, 3.0);
    }

    #[test]
    fn font_size_is_relative_to_the_parent() {
        let mut inh = Inh::default();
        apply(&mut inh, &props(&[("font-size", "20px")]), (100.0, 100.0));
        apply(&mut inh, &props(&[("font-size", "1.5em")]), (100.0, 100.0));
        assert_eq!(inh.font_size, 30.0);
        apply(&mut inh, &props(&[("font-size", "50%")]), (100.0, 100.0));
        assert_eq!(inh.font_size, 15.0);
        apply(&mut inh, &props(&[("font-size", "large")]), (100.0, 100.0));
        assert_eq!(inh.font_size, 18.0);
    }

    #[test]
    fn stroke_width_in_em_uses_the_elements_own_font_size() {
        let mut inh = Inh::default();
        apply(&mut inh, &props(&[("font-size", "10px"), ("stroke-width", "0.5em")]), (100.0, 100.0));
        assert_eq!(inh.stroke_width, 5.0);
    }

    #[test]
    fn percent_stroke_width_uses_the_viewport_diagonal() {
        let mut inh = Inh::default();
        apply(&mut inh, &props(&[("stroke-width", "10%")]), (200.0, 200.0));
        assert!((inh.stroke_width - 20.0).abs() < 1e-9);
    }

    #[test]
    fn dash_arrays_are_normalized() {
        let d = |s: &str| {
            let mut inh = Inh::default();
            apply(&mut inh, &props(&[("stroke-dasharray", s)]), (100.0, 100.0));
            inh.dasharray
        };
        assert_eq!(d("4 2"), vec![4.0, 2.0]);
        assert_eq!(d("5,3,1"), vec![5.0, 3.0, 1.0, 5.0, 3.0, 1.0]);
        assert_eq!(d("none"), Vec::<f64>::new());
        assert_eq!(d("0 0"), Vec::<f64>::new());
        assert_eq!(d("4 -2"), Vec::<f64>::new());
    }

    #[test]
    fn weights_map_to_the_ast() {
        let w = |s: &str| {
            let mut inh = Inh::default();
            apply(&mut inh, &props(&[("font-weight", s)]), (100.0, 100.0));
            inh.font_weight
        };
        assert_eq!(w("bold"), Some(FontWeight::Bold));
        assert_eq!(w("700"), Some(FontWeight::Bold));
        assert_eq!(w("400"), Some(FontWeight::Normal));
        assert_eq!(w("600"), Some(FontWeight::Numeric(600)));
        assert_eq!(w("heavy"), None);
    }

    #[test]
    fn visibility_can_be_turned_back_on_by_a_child() {
        let mut inh = Inh::default();
        apply(&mut inh, &props(&[("visibility", "hidden")]), (1.0, 1.0));
        assert!(inh.hidden);
        apply(&mut inh, &props(&[("visibility", "visible")]), (1.0, 1.0));
        assert!(!inh.hidden);
    }

    #[test]
    fn opacity_is_not_inherited_and_clamps() {
        assert_eq!(element_opacity(&props(&[("opacity", "0.25")])), 0.25);
        assert_eq!(element_opacity(&props(&[("opacity", "50%")])), 0.5);
        assert_eq!(element_opacity(&props(&[("opacity", "7")])), 1.0);
        assert_eq!(element_opacity(&props(&[])), 1.0);
    }

    #[test]
    fn paint_order_decides_whether_the_stroke_comes_first() {
        let po = |s: &str| {
            let mut inh = Inh::default();
            apply(&mut inh, &props(&[("paint-order", s)]), (1.0, 1.0));
            inh.stroke_first
        };
        assert!(!po("normal"));
        assert!(po("stroke"));
        assert!(po("stroke fill"));
        assert!(po("stroke markers"));
        assert!(!po("fill"));
        assert!(!po("fill stroke"));
        assert!(!po("markers"));
        assert!(!po("stroke stroke"), "a duplicate makes the value invalid, so the default stays");
        assert!(!po("sideways"));
    }

    #[test]
    fn an_icc_color_hint_makes_the_whole_paint_invalid() {
        // Browsers and resvg do not read icc-color, so the declaration is
        // dropped and the paint stays at its default.
        assert_eq!(parse_paint("url(#g) #CD853F icc-color(Foo, 0.1)"), None);
        assert_eq!(parse_paint("url(#g) junk"), None);
    }

    #[test]
    fn alpha_scaling() {
        assert_eq!(with_alpha(Color::rgb(1, 2, 3), 0.5), Color::rgba(1, 2, 3, 128));
        assert_eq!(with_alpha(Color::rgba(1, 2, 3, 100), 0.5), Color::rgba(1, 2, 3, 50));
        assert_eq!(with_alpha(Color::rgb(1, 2, 3), 1.0), Color::rgb(1, 2, 3));
    }
}
