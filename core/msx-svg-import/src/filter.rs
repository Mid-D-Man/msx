// core/msx-svg-import/src/filter.rs
//! Filters by mapping onto layer effects.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Filters". MSX has no filter
//! graph. A layer can carry blur, shadow and color matrix effects, so the
//! importer reads the filters that are a chain of `feGaussianBlur`,
//! `feColorMatrix`, `feDropShadow` and `feOffset` primitives (and the CSS
//! functions `blur()`, `drop-shadow()`, `grayscale()`, `sepia()`,
//! `saturate()`, `hue-rotate()`, `brightness()`, `contrast()`, `invert()` and
//! `opacity()`) and turns
//! each filter of the `filter` list into one layer: the effects, a clip to the
//! filter region, and the content inside. Everything else is reported and the
//! element is drawn without that filter.
//!
//! The layer sits inside the layer or group that carries the element
//! transform, so effect distances are in the element's own units and every
//! renderer scales them with the transform.

use msx_ast::{Color, Effect, Element, Group, Layer, Transform};
use roxmltree::Node;

use crate::color_matrix as cm;
use crate::geom::BBox;
use crate::parse::{number_list, parse_color, parse_length, Unit};
use crate::style::Props;
use crate::{obb_fraction, parse_url_id, plain_rect, Importer, XLINK_NS};

/// One entry of a `filter` list, ready to wrap content.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub effects: Vec<Effect>,
    /// The sum of the `feOffset` primitives. The content moves by this before
    /// the effects run, which gives the same picture as moving it in between.
    pub offset: (f64, f64),
    /// The filter region as a clip, `None` when the filter has no region.
    pub region: Option<Element>,
}

impl Step {
    /// `kids` inside one layer with this filter applied.
    pub fn wrap(self, kids: Vec<Element>) -> Element {
        let mut kids = kids;
        if self.offset != (0.0, 0.0) {
            let transform = Some(Transform::Translate { x: self.offset.0, y: self.offset.1 });
            kids = vec![Element::Group(Group { children: kids, id: None, transform, style: None })];
        }
        let mut layer = Layer::new(kids);
        layer.effects = self.effects;
        layer.clip_path = self.region.map(|r| vec![r]);
        Element::Layer(layer)
    }
}

/// Why a `url()` of a `filter` list gave no filter.
#[derive(Debug)]
enum Bad {
    /// The element is not drawn, and the warning is already given.
    Hidden,
    /// The link points at nothing or at something that is not a filter. The
    /// text finishes "<element> ...". Such a link is skipped when other
    /// filters of the list work, as resvg does, and hides the element when it
    /// is all there is.
    Link(String),
}

/// What the `filter` property of one element comes to.
#[derive(Debug, Default)]
pub(crate) struct Resolved {
    /// The filters to apply, the first one innermost.
    pub steps: Vec<Step>,
    /// The element is not drawn at all (a filter that does not exist, an empty
    /// filter, a region without area).
    pub hidden: bool,
}

/// Wraps `kids` in the layers of `steps`, the first step innermost.
pub(crate) fn wrap_all(steps: Vec<Step>, kids: Vec<Element>) -> Vec<Element> {
    let mut kids = kids;
    for step in steps {
        kids = vec![step.wrap(kids)];
    }
    kids
}

/// How one blur primitive reads its `stdDeviation`.
#[derive(Debug, PartialEq)]
enum Deviation {
    /// Nothing to blur: empty, negative, zero, or more than two values. The
    /// input passes through.
    Skip,
    Even(f64),
    /// Different amounts per axis (x, y), one of them possibly 0.
    Uneven(f64, f64),
}

fn deviation(raw: Option<&str>, default: Option<f64>, scale: (f64, f64)) -> Deviation {
    let values = match raw {
        Some(s) => number_list(s),
        None => match default {
            Some(d) => vec![d],
            None => return Deviation::Skip,
        },
    };
    let (sx, sy) = match values.as_slice() {
        [v] => (*v, *v),
        [x, y] => (*x, *y),
        _ => return Deviation::Skip,
    };
    if sx < 0.0 || sy < 0.0 || (sx == 0.0 && sy == 0.0) {
        return Deviation::Skip;
    }
    let (sx, sy) = (sx * scale.0, sy * scale.1);
    if (sx - sy).abs() < 1e-9 {
        Deviation::Even(sx)
    } else {
        Deviation::Uneven(sx, sy)
    }
}

/// Splits on white space outside parentheses.
fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            c if c.is_whitespace() && depth <= 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `name(args)` as the lower case name and the text between the parentheses.
fn function(item: &str) -> Option<(String, &str)> {
    let open = item.find('(')?;
    let inner = item[open + 1..].strip_suffix(')')?;
    Some((item[..open].trim().to_ascii_lowercase(), inner))
}

fn split_color_opacity(color: Color, opacity: f64) -> (Color, f64) {
    let alpha = f64::from(color.a) / 255.0;
    (Color { a: 255, ..color }, (opacity * alpha).clamp(0.0, 1.0))
}

fn flood_opacity(v: Option<&String>) -> f64 {
    match v.map(|s| s.trim()) {
        Some(s) => obb_fraction(s).map(|o| o.clamp(0.0, 1.0)).unwrap_or(1.0),
        None => 1.0,
    }
}

impl<'a, 'i> Importer<'a, 'i> {
    /// The filters that apply to `node`, from its `filter` property.
    pub(crate) fn resolve_filter(&mut self, node: &Node<'a, 'i>, props: &Props, bbox: Option<BBox>) -> Resolved {
        let Some(value) = props.get("filter").map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && v != "none") else {
            return Resolved::default();
        };
        let label = Self::label(node);
        let mut steps = Vec::new();
        // Filters that resolved, drawable here or not, and links that did not.
        let mut valid = 0usize;
        let mut bad_links = Vec::new();
        for item in split_top_level(&value) {
            if let Some(id) = parse_url_id(&item) {
                match self.filter_by_url(&label, &id, bbox) {
                    Ok(Some(step)) => {
                        valid += 1;
                        steps.push(step);
                    }
                    Ok(None) => valid += 1,
                    Err(Bad::Hidden) => return Resolved { steps: Vec::new(), hidden: true },
                    Err(Bad::Link(why)) => bad_links.push(why),
                }
            } else if let Some((name, args)) = function(&item) {
                match self.filter_function(node, &name, args) {
                    Ok(Some(step)) => {
                        valid += 1;
                        steps.push(step);
                    }
                    Ok(None) => valid += 1,
                    Err(why) => {
                        self.warn(format!("{label} uses filter: {item}, which is not supported ({why}); the filter property is ignored"));
                        return Resolved::default();
                    }
                }
            } else {
                self.warn(format!("{label} uses filter: {item}, which is not supported; the filter property is ignored"));
                return Resolved::default();
            }
        }
        if !bad_links.is_empty() && valid == 0 {
            for why in bad_links {
                self.warn(format!("{label} {why}; the element is not drawn"));
            }
            return Resolved { steps: Vec::new(), hidden: true };
        }
        for why in bad_links {
            self.warn(format!("{label} {why}; that entry of the list is skipped"));
        }
        Resolved { steps, hidden: false }
    }

    // ── CSS filter functions ─────────────────────────────────────────────────

    fn filter_function(&mut self, node: &Node<'a, 'i>, name: &str, args: &str) -> Result<Option<Step>, String> {
        // `em` in a filter function is the font size of the element.
        let font_size = self
            .inherited_prop(*node, "font-size")
            .and_then(|v| parse_length(&v))
            .map(|l| l.to_px(16.0, 16.0))
            .filter(|v| *v > 0.0)
            .unwrap_or(16.0);
        match name {
            "blur" => {
                let tokens = split_top_level(args);
                let radius = match tokens.as_slice() {
                    [] => 0.0,
                    [one] => length_px(one, font_size).ok_or("the radius must be a length")?,
                    _ => return Err("blur takes one length".into()),
                };
                if radius < 0.0 {
                    return Err("the radius must not be negative".into());
                }
                if radius == 0.0 {
                    return Ok(None);
                }
                Ok(Some(Step { effects: vec![Effect::Blur { radius }], offset: (0.0, 0.0), region: None }))
            }
            "drop-shadow" => {
                if has_top_level_comma(args) {
                    return Err("the values must not be separated by commas".into());
                }
                let mut lengths = Vec::new();
                let mut color = None;
                for tok in split_top_level(args) {
                    if let Some(l) = parse_length(&tok).filter(|l| l.unit != Unit::Percent) {
                        lengths.push(l.to_px(font_size, 0.0));
                    } else if tok.eq_ignore_ascii_case("currentcolor") && color.is_none() {
                        color = Some(self.color_of(*node));
                    } else if let Some(c) = parse_color(&tok).filter(|_| color.is_none()) {
                        color = Some(c);
                    } else {
                        return Err(format!("cannot read {tok}"));
                    }
                }
                if !(2..=3).contains(&lengths.len()) {
                    return Err("drop-shadow needs two or three lengths".into());
                }
                let blur = lengths.get(2).copied().unwrap_or(0.0);
                if blur < 0.0 {
                    return Err("the blur must not be negative".into());
                }
                let (color, opacity) = split_color_opacity(color.unwrap_or_else(|| self.color_of(*node)), 1.0);
                let effect = Effect::DropShadow { offset_x: lengths[0], offset_y: lengths[1], blur_radius: blur, color, opacity };
                Ok(Some(Step { effects: vec![effect], offset: (0.0, 0.0), region: None }))
            }
            "grayscale" | "sepia" | "saturate" | "brightness" | "contrast" | "invert" | "opacity" => {
                let amount = color_amount(args)?;
                let values = match name {
                    "grayscale" => cm::grayscale(amount),
                    "sepia" => cm::sepia(amount),
                    "saturate" => cm::saturate(amount),
                    "brightness" => cm::brightness(amount),
                    "contrast" => cm::contrast(amount),
                    "invert" => cm::invert(amount),
                    _ => cm::opacity(amount),
                };
                Ok(color_step(values))
            }
            "hue-rotate" => Ok(color_step(cm::hue_rotate(hue_angle(args)?))),
            other => Err(format!("{other}() has no layer effect")),
        }
    }

    /// Whether the primitives of a filter work in linear light, which is the
    /// default. `color-interpolation-filters` is inherited, so it can sit on
    /// the primitive, the filter or an ancestor.
    fn filters_in_linear_light(&self, prim: Node<'a, 'i>) -> bool {
        let mut cur = Some(prim);
        while let Some(n) = cur {
            // The attribute is not in the table of presentation attributes the
            // style code reads, so it is taken from the node, and a style
            // rule or a `style` attribute still wins over it.
            let from_style = self.props_for(n).get("color-interpolation-filters").cloned();
            let value = from_style.or_else(|| n.attribute("color-interpolation-filters").map(str::to_string));
            match value.as_deref().map(str::trim) {
                Some("sRGB") => return false,
                Some("linearRGB") => return true,
                _ => {}
            }
            cur = n.parent().filter(|p| p.is_element());
        }
        true
    }

    // ── Filter elements ──────────────────────────────────────────────────────

    /// `Ok(None)` when the filter cannot be used and the element is drawn
    /// without it, `Err` when the element is not drawn.
    fn filter_by_url(&mut self, label: &str, id: &str, bbox: Option<BBox>) -> Result<Option<Step>, Bad> {
        let Some(start) = self.ids.get(id).copied() else {
            return Err(Bad::Link(format!("uses the filter #{id}, which does not exist")));
        };
        if start.tag_name().name() != "filter" {
            return Err(Bad::Link(format!("uses #{id} as a filter, but it is a <{}>", start.tag_name().name())));
        }
        let chain = self.filter_chain(start);
        let attr = |name: &str| chain.iter().find_map(|n| n.attribute(name)).map(str::trim);

        let user_space = attr("filterUnits") == Some("userSpaceOnUse");
        let region = if user_space {
            let (vw, vh) = self.vp;
            let get = |name: &str, base: f64, default: f64| attr(name).and_then(parse_length).map(|l| l.to_px(16.0, base)).unwrap_or(default);
            (get("x", vw, -0.1 * vw), get("y", vh, -0.1 * vh), get("width", vw, 1.2 * vw), get("height", vh, 1.2 * vh))
        } else {
            let Some(b) = bbox.filter(|b| b.w > 0.0 && b.h > 0.0) else {
                self.warn(format!("{label} uses the filter #{id} on something without area; the element is not drawn"));
                return Err(Bad::Hidden);
            };
            let f = |name: &str, default: f64| attr(name).and_then(obb_fraction).unwrap_or(default);
            (b.x + f("x", -0.1) * b.w, b.y + f("y", -0.1) * b.h, f("width", 1.2) * b.w, f("height", 1.2) * b.h)
        };
        if region.2 <= 0.0 || region.3 <= 0.0 {
            self.warn(format!("{label} uses the filter #{id}, whose region has no area; the element is not drawn"));
            return Err(Bad::Hidden);
        }

        let prims: Vec<Node<'a, 'i>> = chain
            .iter()
            .map(|n| n.children().filter(|c| c.is_element() && c.tag_name().name().starts_with("fe")).collect::<Vec<_>>())
            .find(|kids| !kids.is_empty())
            .unwrap_or_default();
        if prims.is_empty() {
            self.warn(format!("{label} uses the filter #{id}, which has no primitives; the element is not drawn"));
            return Err(Bad::Hidden);
        }

        let primitive_scale = if attr("primitiveUnits") == Some("objectBoundingBox") {
            match bbox.filter(|b| b.w > 0.0 && b.h > 0.0) {
                Some(b) => (b.w, b.h),
                None => return Err(Bad::Hidden),
            }
        } else {
            (1.0, 1.0)
        };

        match self.filter_primitives(&prims, primitive_scale) {
            Ok((effects, offset)) => {
                let (x, y, w, h) = region;
                Ok(Some(Step { effects, offset, region: Some(plain_rect(x, y, x + w, y + h)) }))
            }
            Err(why) => {
                self.warn(format!("{label} uses the filter #{id}, which is not supported ({why}); drawn without it"));
                Ok(None)
            }
        }
    }

    /// The filter element and the filters it refers to through `href`, nearest first.
    fn filter_chain(&self, start: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
        let mut chain = vec![start];
        let mut cur = start;
        while chain.len() < 16 {
            let Some(next) = cur
                .attribute("href")
                .or_else(|| cur.attribute((XLINK_NS, "href")))
                .and_then(|h| h.trim().strip_prefix('#'))
                .and_then(|id| self.ids.get(id).copied())
                .filter(|n| n.tag_name().name() == "filter")
            else {
                break;
            };
            if chain.iter().any(|n| n.id() == next.id()) {
                break;
            }
            chain.push(next);
            cur = next;
        }
        chain
    }

    /// The effects and the offset of a primitive chain, or why it cannot be one.
    fn filter_primitives(&mut self, prims: &[Node<'a, 'i>], scale: (f64, f64)) -> Result<(Vec<Effect>, (f64, f64)), String> {
        let mut effects = Vec::new();
        let mut offset = (0.0, 0.0);
        let mut previous: Option<String> = None;
        for (index, prim) in prims.iter().enumerate() {
            let tag = prim.tag_name().name();
            let input = prim.attribute("in").map(str::trim);
            let from_source = index == 0 && matches!(input, None | Some("SourceGraphic"));
            let from_previous = index > 0 && (input.is_none() || (input.is_some() && input == previous.as_deref()));
            if !from_source && !from_previous {
                return Err(format!("{tag} reads {}", input.unwrap_or("an earlier result")));
            }
            if ["x", "y", "width", "height"].iter().any(|a| prim.attribute(*a).is_some()) {
                return Err(format!("{tag} has its own region"));
            }
            match tag {
                "feGaussianBlur" => match deviation(prim.attribute("stdDeviation"), None, scale) {
                    Deviation::Skip => {}
                    Deviation::Even(radius) => effects.push(Effect::Blur { radius }),
                    Deviation::Uneven(rx, ry) => effects.push(Effect::BlurXY { rx, ry }),
                },
                "feColorMatrix" => {
                    let values = prim.attribute("values").map(number_list);
                    let values = cm::from_primitive(prim.attribute("type"), values.as_deref());
                    if !cm::is_identity(&values) {
                        effects.push(Effect::ColorMatrix { values: cm::tidy(values), linear: self.filters_in_linear_light(*prim) });
                    }
                }
                "feOffset" => {
                    offset.0 += number_attr(prim, "dx").unwrap_or(0.0) * scale.0;
                    offset.1 += number_attr(prim, "dy").unwrap_or(0.0) * scale.1;
                }
                "feDropShadow" => {
                    let blur = match deviation(prim.attribute("stdDeviation"), Some(2.0), scale) {
                        Deviation::Skip => 0.0,
                        Deviation::Even(radius) => radius,
                        Deviation::Uneven(..) => return Err("a shadow with a different blur for each axis".into()),
                    };
                    let props = self.props_for(*prim);
                    let color = match props.get("flood-color").map(|s| s.trim()) {
                        Some(s) if s.eq_ignore_ascii_case("currentcolor") => self.color_of(*prim),
                        Some(s) => parse_color(s).unwrap_or(Color::BLACK),
                        None => Color::BLACK,
                    };
                    let (color, opacity) = split_color_opacity(color, flood_opacity(props.get("flood-opacity")));
                    effects.push(Effect::DropShadow {
                        offset_x: number_attr(prim, "dx").unwrap_or(2.0) * scale.0,
                        offset_y: number_attr(prim, "dy").unwrap_or(2.0) * scale.1,
                        blur_radius: blur,
                        color,
                        opacity,
                    });
                }
                other => return Err(format!("{other} has no layer effect")),
            }
            previous = prim.attribute("result").map(|r| r.trim().to_string());
        }
        Ok((effects, offset))
    }
}

/// The one amount of a color function, a number or a percentage, 1 when there
/// is none. A negative amount is an error, as in the Filter Effects
/// specification.
fn color_amount(args: &str) -> Result<f64, String> {
    let tokens = split_top_level(args);
    let value = match tokens.as_slice() {
        [] => return Ok(1.0),
        [one] => match one.strip_suffix('%') {
            Some(p) => p.trim().parse::<f64>().map(|v| v / 100.0),
            None => one.parse::<f64>(),
        },
        _ => return Err("a color function takes one amount".into()),
    };
    match value {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(v),
        Ok(_) => Err("the amount must not be negative".into()),
        Err(_) => Err("the amount must be a number or a percentage".into()),
    }
}

/// The angle of `hue-rotate()` in degrees, 0 when there is none. A number
/// without a unit is degrees.
fn hue_angle(args: &str) -> Result<f64, String> {
    let tokens = split_top_level(args);
    let one = match tokens.as_slice() {
        [] => return Ok(0.0),
        [one] => one.as_str(),
        _ => return Err("hue-rotate takes one angle".into()),
    };
    let (number, scale) = if let Some(n) = one.strip_suffix("deg") {
        (n, 1.0)
    } else if let Some(n) = one.strip_suffix("grad") {
        (n, 0.9)
    } else if let Some(n) = one.strip_suffix("rad") {
        (n, 180.0 / std::f64::consts::PI)
    } else if let Some(n) = one.strip_suffix("turn") {
        (n, 360.0)
    } else {
        (one, 1.0)
    };
    match number.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v * scale),
        _ => Err("the angle must be a number with deg, grad, rad or turn".into()),
    }
}

/// A color function as a step, or nothing when the matrix changes no color.
/// The CSS functions work on sRGB values.
fn color_step(values: cm::Matrix) -> Option<Step> {
    if cm::is_identity(&values) {
        return None;
    }
    Some(Step { effects: vec![Effect::ColorMatrix { values: cm::tidy(values), linear: false }], offset: (0.0, 0.0), region: None })
}

fn has_top_level_comma(s: &str) -> bool {
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth <= 0 => return true,
            _ => {}
        }
    }
    false
}

fn number_attr(node: &Node, name: &str) -> Option<f64> {
    node.attribute(name).and_then(|s| s.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
}

fn length_px(s: &str, font_size: f64) -> Option<f64> {
    parse_length(s).filter(|l| l.unit != Unit::Percent).map(|l| l.to_px(font_size, 0.0))
}
