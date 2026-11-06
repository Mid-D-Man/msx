// core/msx-svg-import/src/animate.rs
//! SMIL animation import.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Animation". The timing and baking
//! maths lives in `anim.rs`; this file reads the animation elements and builds
//! the scene around them.
//!
//! An MSX track moves the transform or the opacity of one element, found by
//! id, and the delta wraps the static transform from the outside. So each
//! supported animation gets a wrapper of its own around the animated element:
//!
//! - `animateTransform` (translate, scale, rotate) and `animate` of `x`, `y`,
//!   `cx` and `cy` get a group. They sit inside the element's static transform,
//!   which is how SMIL adds them. A rotation about a centre is a group that
//!   moves the centre to the origin inside the wrapper and back outside it.
//! - `animate` of `opacity` gets a layer, because only a layer composites
//!   opacity the same way on every renderer. So do `fill-opacity` and
//!   `stroke-opacity`, when the shape paints only that one of fill and stroke,
//!   where fading the layer is the same picture. Like `opacity`, the animation
//!   replaces the static value, so the shape is walked without its own.
//! - `animate` of `r` on a circle without a stroke is a uniform scale about the
//!   centre (a stroke would scale with it, which SVG does not do).
//!
//! Several animations on one element nest outermost first in this order:
//! opacity layers, `animateTransform` in document order, then motion, then
//! position (`x`, `y`, `cx`, `cy`), then radius.
//! CSS animation is built from the same pieces: the `transform` functions of
//! the keyframes, taken one by one in list order, become wrappers like the ones
//! `animateTransform` makes, and `opacity` becomes a layer. `transform-origin`
//! (with `transform-box: fill-box`) is the pivot of the scale and the rotation.
//! `set` is a constant animation of one value, so it goes through the same arms
//! as `animate` (`opacity`, `fill-opacity`, `stroke-opacity`, `r`, `x`, `y`,
//! `cx`, `cy`). `visibility` and `display`, by `set` or by a discrete `animate`,
//! become an opacity layer that is 1 while the element shows and 0 while it is
//! hidden. An element that starts hidden is built as if visible (`reveals`).
//! `animateMotion` is sampled into translate tracks, and a rotate track for
//! `rotate="auto"`, by `motion.rs`; it sits inside `animateTransform` and
//! outside the geometry attributes.
//! CSS easing, `steps()` included, is turned into staircase keyframes by
//! `anim.rs`.
//! Anything that has no channel (colors, paths, dash offsets, skew) is reported
//! and the static value is drawn.

use std::collections::HashMap;

use msx_ast::path::parse_d;
use msx_ast::{AnimatedProperty, AnimationTrack, Element, Group, Keyframe, Layer, LoopMode, PathCommand, Point, Scene, Transform};
use roxmltree::{Document, Node, NodeId};

use crate::anim::{self, Curve, Ease, Span, Timing};
use crate::cssanim::{self, AnimDecl, Stops, Tf};
use crate::motion;
use crate::parse::parse_origin;
use crate::style::element_opacity;
use crate::{is_foreign, translate, Importer};

const TIER_OPACITY: u8 = 0;
const TIER_TRANSFORM: u8 = 1;
const TIER_MOTION: u8 = 2;
const TIER_POSITION: u8 = 3;
const TIER_RADIUS: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Replaces {
    Nothing,
    Opacity,
    FillOpacity,
    StrokeOpacity,
}

/// Shortest CSS animation duration the importer accepts, in seconds.
const EPS_DUR: f64 = 0.004;

/// Keeps the last entry of each offset: two keyframes at one offset merge, and
/// the later declarations win.
fn dedupe<V>(entries: Vec<(f64, V, Option<Ease>)>) -> Vec<(f64, V, Option<Ease>)> {
    let mut out: Vec<(f64, V, Option<Ease>)> = Vec::new();
    for e in entries {
        match out.last_mut() {
            Some(last) if (last.0 - e.0).abs() < 1e-12 => *last = e,
            _ => out.push(e),
        }
    }
    out
}

/// Most wrappers one SVG may produce before the rest are left static.
const MAX_ANIMATIONS: usize = 4000;

pub(crate) fn is_animation_tag(tag: &str) -> bool {
    matches!(tag, "animate" | "animateTransform" | "animateMotion" | "animateColor" | "set")
}

/// One parsed animation, ready to wrap an element.
#[derive(Debug, Clone)]
pub(crate) struct Spec {
    props: Vec<AnimatedProperty>,
    curve: Curve,
    timing: Timing,
    /// The value of each channel without the animation.
    base: Vec<f64>,
    /// Wrap in a layer (opacity) instead of a group.
    layer: bool,
    /// Which static value of the element the animation replaces, so the walk leaves it off.
    replaces: Replaces,
    /// Centre of a rotation or a scale.
    pivot: Option<(f64, f64)>,
    /// A CSS `transform-origin` that is resolved once the element is built,
    /// because `fill-box` needs the box of what was built.
    origin: Option<Origin>,
    /// Moves the element, so the element's own transform goes outside the wrapper.
    moves: bool,
    /// The element is hidden without the animation (`display: none` or
    /// `visibility: hidden`) and the animation shows it, so the walk builds it as
    /// if it were visible and the layer around it holds it back until then.
    reveals: bool,
    /// Nesting order, outermost first. Opacity layers commute with everything
    /// and go on the outside, then `animateTransform` in document order. The
    /// attributes `x`, `y`, `cx`, `cy` and `r` change the geometry, which SVG
    /// places inside the transform whatever the document order: the
    /// translations sit outside the radius scale, because moving a scaled
    /// circle is a translation of the scaled circle.
    tier: u8,
}

/// A CSS `transform-origin` and the box it is measured in.
#[derive(Debug, Clone)]
pub(crate) struct Origin {
    value: String,
    fill_box: bool,
}

/// A wrapper that was written, waiting for the scene timeline.
#[derive(Debug)]
pub(crate) struct Pending {
    target: String,
    props: Vec<AnimatedProperty>,
    curve: Curve,
    timing: Timing,
    base: Vec<f64>,
}

/// Numbers separated by whitespace or commas, all of them or nothing.
fn strict_numbers(s: &str) -> Option<Vec<f64>> {
    let parts: Vec<&str> = s.split(|c: char| c.is_whitespace() || c == ',').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    parts.iter().map(|p| p.parse::<f64>().ok().filter(|v| v.is_finite())).collect()
}

/// One length in user units: a number, optionally followed by `px`.
fn user_length(s: &str) -> Option<f64> {
    let s = s.trim();
    s.strip_suffix("px").unwrap_or(s).trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The values of one animation, from `values` or from `from`, `to` and `by`.
/// `ident` stands in for the underlying value where `from` is missing.
fn derive_values(a: &Node, ident: &[f64], parse_one: &dyn Fn(&str) -> Option<Vec<f64>>) -> Result<Vec<Vec<f64>>, String> {
    if a.tag_name().name() == "set" {
        let to = a.attribute("to").map(str::trim).filter(|t| !t.is_empty()).ok_or("it has no to")?;
        return parse_one(to).map(|v| vec![v]).ok_or_else(|| format!("to=\"{to}\" cannot be read"));
    }
    if let Some(v) = a.attribute("values") {
        let vals: Option<Vec<Vec<f64>>> = v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(parse_one).collect();
        return vals.filter(|v| !v.is_empty()).ok_or_else(|| format!("values=\"{}\" cannot be read", v.trim()));
    }
    let read = |name: &str| -> Result<Option<Vec<f64>>, String> {
        match a.attribute(name).map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(None),
            Some(s) => parse_one(s).map(Some).ok_or_else(|| format!("{name}=\"{s}\" cannot be read")),
        }
    };
    let (from, to, by) = (read("from")?, read("to")?, read("by")?);
    let add = |x: &[f64], d: &[f64]| -> Vec<f64> { x.iter().zip(d).map(|(p, q)| p + q).collect() };
    match (from, to, by) {
        (Some(f), Some(t), _) => Ok(vec![f, t]),
        (Some(f), None, Some(b)) => {
            let t = add(&f, &b);
            Ok(vec![f, t])
        }
        (None, Some(t), _) => Ok(vec![ident.to_vec(), t]),
        (None, None, Some(b)) => Ok(vec![ident.to_vec(), add(ident, &b)]),
        (Some(_), None, None) => Err("it has a from but no to or by".into()),
        (None, None, None) => Err("it has no values, from, to or by".into()),
    }
}

/// An opacity value: a number, or a percentage, clamped to 0 to 1.
fn parse_alpha(s: &str) -> Option<f64> {
    let s = s.trim();
    let v = match s.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok()? / 100.0,
        None => s.parse::<f64>().ok()?,
    };
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

impl<'a, 'i> Importer<'a, 'i> {
    /// An inherited property as the element sees it: the nearest ancestor or
    /// the element itself that sets it, `inherit` passed over.
    fn computed_prop(&self, node: Node<'a, 'i>, name: &str) -> Option<String> {
        let mut cur = Some(node);
        while let Some(n) = cur {
            if let Some(v) = self.props_for(n).get(name).filter(|v| !v.trim().eq_ignore_ascii_case("inherit")) {
                return Some(v.clone());
            }
            cur = n.parent().filter(|p| p.is_element());
        }
        None
    }

    /// Whether the element paints its fill or its stroke at all. A fill is
    /// black unless set, a stroke is none unless set.
    fn paints(&self, node: Node<'a, 'i>, which: &str) -> bool {
        match self.computed_prop(node, which) {
            Some(v) => !v.trim().eq_ignore_ascii_case("none"),
            None => which == "fill",
        }
    }

    /// Checks that fading the whole shape is the same picture as fading one of
    /// its paints, and returns the static value of that paint's opacity.
    fn single_paint(&self, node: Node<'a, 'i>, attr: &str) -> Result<f64, String> {
        let (this, other) = if attr == "fill-opacity" { ("fill", "stroke") } else { ("stroke", "fill") };
        let node_tag = node.tag_name().name();
        if !matches!(node_tag, "rect" | "circle" | "ellipse" | "path" | "polygon" | "polyline") {
            return Err(format!("{attr} on <{node_tag}> has no animation channel"));
        }
        if self.paints(node, other) {
            return Err(format!("the shape also paints its {other}, which a fade of the whole shape would change too"));
        }
        if !self.paints(node, this) {
            return Err(format!("the shape has no {this} to fade"));
        }
        Ok(self.computed_prop(node, attr).and_then(|v| parse_alpha(&v)).unwrap_or(1.0))
    }

    /// Whether the element names a CSS animation whose `@keyframes` exist.
    fn css_animated(&self, node: Node<'a, 'i>) -> bool {
        if self.sheet.keyframes.is_empty() {
            return false;
        }
        let props = self.props_for(node);
        ["animation", "animation-name"].iter().any(|k| props.get(*k).is_some_and(|v| !v.trim().is_empty() && v.trim() != "none"))
    }

    /// The CSS animations of `node` that can be imported. The others are reported.
    fn css_specs(&mut self, node: Node<'a, 'i>) -> Vec<Spec> {
        if self.sheet.keyframes.is_empty() {
            return Vec::new();
        }
        let props = self.props_for(node);
        let label = Self::label(&node);
        let mut specs = Vec::new();
        for decl in cssanim::parse_animations(|k| props.get(k).map(String::as_str)) {
            match decl {
                Err(why) => self.warn(format!("{label} has a CSS animation that is not supported ({why}); the static frame is drawn")),
                Ok(d) => {
                    let mut notes = Vec::new();
                    match self.css_one(node, &props, &d, &mut notes) {
                        Ok(mut found) => specs.append(&mut found),
                        Err(why) => self.warn(format!("{label} has the CSS animation \"{}\" that is not supported ({why}); the static frame is drawn", d.name)),
                    }
                    for n in notes {
                        self.warn(format!("{label} CSS animation \"{}\": {n}", d.name));
                    }
                }
            }
        }
        specs
    }

    /// One CSS animation, as specs: a wrapper per function of the keyframe
    /// transforms, and a layer for opacity.
    fn css_one(&mut self, node: Node<'a, 'i>, props: &crate::style::Props, d: &AnimDecl, notes: &mut Vec<String>) -> Result<Vec<Spec>, String> {
        let Some(stops) = self.sheet.keyframes.get(&d.name).cloned() else {
            return Err(format!("@keyframes {} is not defined", d.name));
        };
        if d.paused {
            return Err("it is paused, so it would only ever show its first frame".into());
        }
        if d.duration < EPS_DUR {
            return Err("its animation-duration is 0 or too short".into());
        }
        if d.count == Some(0.0) {
            return Err("its animation-iteration-count is 0".into());
        }
        let timing = |factor: f64| Timing { begin: d.delay, dur: d.duration * factor, active: d.count.map(|c| c * d.duration), freeze: d.forwards, fill_before: d.backwards };

        type Entry<V> = (f64, V, Option<Ease>);
        let mut transform: Vec<Entry<Vec<Tf>>> = Vec::new();
        let mut opacity: Vec<Entry<f64>> = Vec::new();
        let mut fill_opacity: Vec<Entry<f64>> = Vec::new();
        let mut stroke_opacity: Vec<Entry<f64>> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for stop in &stops {
            let own = match stop.decls.iter().rev().find(|x| x.name == "animation-timing-function") {
                Some(x) => Some(cssanim::parse_timing_function(&x.value)?),
                None => None,
            };
            for x in &stop.decls {
                let alpha = || parse_alpha(&x.value).ok_or_else(|| format!("{} \"{}\" is not a number", x.name, x.value));
                match x.name.as_str() {
                    "transform" => transform.push((stop.offset, cssanim::parse_transform_list(&x.value)?, own)),
                    "opacity" => opacity.push((stop.offset, alpha()?, own)),
                    "fill-opacity" => fill_opacity.push((stop.offset, alpha()?, own)),
                    "stroke-opacity" => stroke_opacity.push((stop.offset, alpha()?, own)),
                    "animation-timing-function" | "animation-composition" | "transform-origin" | "transform-box" => {}
                    other => {
                        if !skipped.iter().any(|s| s == other) {
                            skipped.push(other.to_string());
                        }
                    }
                }
            }
        }
        for p in skipped {
            notes.push(format!("keyframe property {p} is not supported (no animation channel); the static value is drawn"));
        }

        let mut specs = Vec::new();
        if !transform.is_empty() {
            let mut ents = dedupe(transform);
            if ents[0].0 > 1e-12 {
                ents.insert(0, (0.0, Vec::new(), None));
            }
            if ents[ents.len() - 1].0 < 1.0 - 1e-12 {
                ents.push((1.0, Vec::new(), None));
            }
            let lists = cssanim::align(&ents.iter().map(|e| e.1.clone()).collect::<Vec<_>>())?;
            let origin = props.get("transform-origin").map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).map(|value| {
                let fill_box = match props.get("transform-box").map(|b| b.trim().to_ascii_lowercase()) {
                    Some(b) if b == "fill-box" => true,
                    Some(b) if !b.is_empty() && b != "view-box" => {
                        notes.push(format!("transform-box: {b} is not supported; view-box is used"));
                        false
                    }
                    _ => false,
                };
                Origin { value, fill_box }
            });
            if node.attribute("transform").is_some() || props.get("transform").is_some() {
                notes.push("CSS replaces the transform of the element with the animated one; the static transform is kept and the animation is added to it".into());
            }
            for k in 0..lists[0].len() {
                let stops = Stops { offsets: ents.iter().map(|e| e.0).collect(), values: lists.iter().map(|l| l[k].comps()).collect(), eases: (0..ents.len() - 1).map(|i| ents[i].2.unwrap_or(d.ease)).collect() };
                let (oriented, factor) = stops.orient(d.direction);
                let curve = oriented.curve()?;
                let (props_k, base, pivoted) = match lists[0][k] {
                    Tf::Translate(..) => (vec![AnimatedProperty::TranslateX, AnimatedProperty::TranslateY], vec![0.0, 0.0], false),
                    Tf::Scale(..) => (vec![AnimatedProperty::ScaleX, AnimatedProperty::ScaleY], vec![1.0, 1.0], true),
                    Tf::Rotate(_) => (vec![AnimatedProperty::Rotate], vec![0.0], true),
                };
                specs.push(Spec { props: props_k, curve, timing: timing(factor), base, layer: false, replaces: Replaces::Nothing, pivot: None, origin: if pivoted { origin.clone() } else { None }, moves: true, reveals: false, tier: TIER_TRANSFORM });
            }
        }
        if !opacity.is_empty() {
            let fixed = element_opacity(props);
            specs.push(self.css_alpha(dedupe(opacity), fixed, Replaces::Opacity, d, &timing)?);
        }
        for (list, attr, which) in [(fill_opacity, "fill-opacity", Replaces::FillOpacity), (stroke_opacity, "stroke-opacity", Replaces::StrokeOpacity)] {
            if list.is_empty() {
                continue;
            }
            match self.single_paint(node, attr) {
                Ok(fixed) => specs.push(self.css_alpha(dedupe(list), fixed, which, d, &timing)?),
                Err(why) => notes.push(format!("{attr} was left static: {why}")),
            }
        }
        Ok(specs)
    }

    /// The spec of an opacity-like property: a layer whose track holds the value.
    fn css_alpha(&self, ents: Vec<(f64, f64, Option<Ease>)>, fixed: f64, replaces: Replaces, d: &AnimDecl, timing: &dyn Fn(f64) -> Timing) -> Result<Spec, String> {
        let mut offsets = Vec::new();
        let mut values = Vec::new();
        let mut eases = Vec::new();
        if ents[0].0 > 1e-12 {
            offsets.push(0.0);
            values.push(vec![fixed]);
            eases.push(d.ease);
        }
        for e in &ents {
            offsets.push(e.0);
            values.push(vec![e.1]);
            eases.push(e.2.unwrap_or(d.ease));
        }
        if ents[ents.len() - 1].0 < 1.0 - 1e-12 {
            offsets.push(1.0);
            values.push(vec![fixed]);
            eases.push(d.ease);
        }
        eases.pop();
        let (oriented, factor) = Stops { offsets, values, eases }.orient(d.direction);
        let curve = oriented.curve()?;
        Ok(Spec { props: vec![AnimatedProperty::Opacity], curve, timing: timing(factor), base: vec![fixed], layer: true, replaces, pivot: None, origin: None, moves: false, reveals: false, tier: TIER_OPACITY })
    }

    /// The pivot of a CSS `transform-origin`, measured in the viewport or, for
    /// `fill-box`, in the box of the built element.
    fn resolve_origin(&self, o: &Origin, content: &[Element]) -> Option<(f64, f64)> {
        let (w, h, bx, by) = if o.fill_box {
            let b = crate::elements_bbox(content)?;
            (b.w, b.h, b.x, b.y)
        } else {
            (self.vp.0, self.vp.1, 0.0, 0.0)
        };
        parse_origin(&o.value, w, h).map(|(x, y)| (x + bx, y + by)).filter(|p| *p != (0.0, 0.0))
    }

    /// Finds the animation elements that name an element with `href`, so the
    /// element does not have to be their parent.
    pub(crate) fn index_animations(doc: &'a Document<'i>, ids: &HashMap<&'a str, Node<'a, 'i>>) -> HashMap<NodeId, Vec<Node<'a, 'i>>>
    {
        let mut by_target: HashMap<NodeId, Vec<Node<'a, 'i>>> = HashMap::new();
        for n in doc.descendants().filter(|n| n.is_element() && is_animation_tag(n.tag_name().name())) {
            let href = n.attribute("href").or_else(|| n.attribute((crate::XLINK_NS, "href")));
            if let Some(target) = href.and_then(|h| h.trim().strip_prefix('#')).and_then(|id| ids.get(id)) {
                by_target.entry(target.id()).or_default().push(n);
            }
        }
        by_target
    }

    /// The animation elements that act on `node`: its own children that do not
    /// point elsewhere, then those that name it.
    fn animation_nodes(&self, node: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
        let mut found: Vec<Node<'a, 'i>> = node
            .children()
            .filter(|c| c.is_element() && !is_foreign(c) && is_animation_tag(c.tag_name().name()))
            .filter(|c| c.attribute("href").or_else(|| c.attribute((crate::XLINK_NS, "href"))).is_none())
            .collect();
        if let Some(extra) = self.anim_by_href.get(&node.id()) {
            found.extend(extra.iter().copied());
        }
        found
    }

    /// Whether `node` carries animation elements, so the walk has to wrap it.
    pub(crate) fn has_animation(&self, node: Node<'a, 'i>) -> bool {
        node.children().any(|c| c.is_element() && is_animation_tag(c.tag_name().name())) || self.anim_by_href.contains_key(&node.id()) || self.css_animated(node)
    }

    /// Every animation of `node` that can be imported. The others are reported.
    pub(crate) fn animation_specs(&mut self, node: Node<'a, 'i>) -> Vec<Spec> {
        let label = Self::label(&node);
        let mut specs = Vec::new();
        for a in self.animation_nodes(node) {
            let tag = a.tag_name().name();
            let mut notes = Vec::new();
            match self.build_spec(node, a, &mut notes) {
                Ok(spec) => specs.push(spec),
                Err(why) => self.warn(format!("{label} has a <{tag}> that is not supported ({why}); the static frame is drawn")),
            }
            for n in notes {
                self.warn(format!("{label} <{tag}>: {n}"));
            }
        }
        specs.extend(self.css_specs(node));
        specs
    }

    fn build_spec(&mut self, node: Node<'a, 'i>, a: Node<'a, 'i>, notes: &mut Vec<String>) -> Result<Spec, String> {
        let tag = a.tag_name().name();
        match tag {
            "animateTransform" | "animate" | "set" | "animateMotion" => {}
            other => return Err(format!("<{other}> has no matching animation channel")),
        }
        let timing = if tag == "set" { Timing::parse_set(|k| a.attribute(k), notes)? } else { Timing::parse(|k| a.attribute(k), notes)? };
        if tag == "animateMotion" {
            return self.motion_spec(a, timing, notes);
        }
        let calc = a.attribute("calcMode");
        let (kt, ks) = (a.attribute("keyTimes"), a.attribute("keySplines"));

        if tag == "animateTransform" {
            if a.attribute("attributeName").map(str::trim).is_some_and(|n| n != "transform") {
                return Err("it animates something other than transform".into());
            }
            if a.attribute("additive").map(str::trim).is_none_or(|v| v != "sum") && node.attribute("transform").is_some() {
                notes.push("it replaces the transform attribute in SVG; the static transform is kept and the animation is added to it".into());
            }
            let kind = a.attribute("type").map(str::trim).unwrap_or("translate");
            let (props, ident): (Vec<AnimatedProperty>, Vec<f64>) = match kind {
                "translate" => (vec![AnimatedProperty::TranslateX, AnimatedProperty::TranslateY], vec![0.0, 0.0]),
                "scale" => (vec![AnimatedProperty::ScaleX, AnimatedProperty::ScaleY], vec![1.0, 1.0]),
                "rotate" => (vec![AnimatedProperty::Rotate], vec![0.0, f64::NAN, f64::NAN]),
                "skewX" | "skewY" => return Err(format!("{kind} has no animation channel")),
                other => return Err(format!("type=\"{other}\" is not known")),
            };
            let parse_one = |s: &str| -> Option<Vec<f64>> {
                let n = strict_numbers(s)?;
                match (kind, n.len()) {
                    ("translate", 1) => Some(vec![n[0], 0.0]),
                    ("translate", 2) | ("scale", 2) => Some(n),
                    ("scale", 1) => Some(vec![n[0], n[0]]),
                    ("rotate", 1) => Some(vec![n[0], f64::NAN, f64::NAN]),
                    ("rotate", 3) => Some(n),
                    _ => None,
                }
            };
            let mut values = derive_values(&a, &ident, &parse_one)?;
            let mut pivot = None;
            if kind == "rotate" {
                for v in &values {
                    if v[1].is_finite() {
                        match pivot {
                            None => pivot = Some((v[1], v[2])),
                            Some(p) if p != (v[1], v[2]) => notes.push("the centre of the rotation changes over time; the first one is used".into()),
                            Some(_) => {}
                        }
                    }
                }
                values = values.into_iter().map(|v| vec![v[0]]).collect();
            }
            let base = ident.iter().take(props.len()).copied().collect();
            let curve = Curve::new(values, kt, calc, ks)?;
            return Ok(Spec { props, curve, timing, base, layer: false, replaces: Replaces::Nothing, pivot: pivot.filter(|p| *p != (0.0, 0.0)), origin: None, moves: true, reveals: false, tier: TIER_TRANSFORM });
        }

        let attr = a.attribute("attributeName").map(str::trim).unwrap_or("");
        let node_tag = node.tag_name().name();
        match attr {
            "opacity" => {
                let base = element_opacity(&self.props_for(node));
                let parse_one = |s: &str| -> Option<Vec<f64>> { s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| vec![v.clamp(0.0, 1.0)]) };
                let values = derive_values(&a, &[base], &parse_one)?;
                let curve = Curve::new(values, kt, calc, ks)?;
                Ok(Spec { props: vec![AnimatedProperty::Opacity], curve, timing, base: vec![base], layer: true, replaces: Replaces::Opacity, pivot: None, origin: None, moves: false, reveals: false, tier: TIER_OPACITY })
            }
            "fill-opacity" | "stroke-opacity" => {
                let fixed = self.single_paint(node, attr)?;
                // The static value is what the shape has before the animation
                // begins and after it ends. It is left off the shape.
                let parse_one = |s: &str| -> Option<Vec<f64>> { parse_alpha(s).map(|v| vec![v]) };
                let values = derive_values(&a, &[fixed], &parse_one)?;
                let curve = Curve::new(values, kt, calc, ks)?;
                let replaces = if attr == "fill-opacity" { Replaces::FillOpacity } else { Replaces::StrokeOpacity };
                Ok(Spec { props: vec![AnimatedProperty::Opacity], curve, timing, base: vec![fixed], layer: true, replaces, pivot: None, origin: None, moves: false, reveals: false, tier: TIER_OPACITY })
            }
            "r" => {
                if node_tag != "circle" {
                    return Err(format!("r on <{node_tag}> has no animation channel"));
                }
                if self.paints(node, "stroke") {
                    return Err("the circle has a stroke, which would scale with the radius".into());
                }
                let r0 = self.length(node, "r", 16.0, (self.vp.0 + self.vp.1) / 2.0).unwrap_or(0.0);
                if r0 <= 0.0 {
                    return Err("the circle has no static r to scale from".into());
                }
                let parse_one = |s: &str| -> Option<Vec<f64>> { user_length(s).filter(|v| *v >= 0.0).map(|v| vec![v]) };
                let values = derive_values(&a, &[r0], &parse_one)?;
                let scales: Vec<Vec<f64>> = values.into_iter().map(|v| { let k = (v[0] / r0).max(1e-6); vec![k, k] }).collect();
                let curve = Curve::new(scales, kt, calc, ks)?;
                let cx = self.length(node, "cx", 16.0, self.vp.0).unwrap_or(0.0);
                let cy = self.length(node, "cy", 16.0, self.vp.1).unwrap_or(0.0);
                let props = vec![AnimatedProperty::ScaleX, AnimatedProperty::ScaleY];
                Ok(Spec { props, curve, timing, base: vec![1.0, 1.0], layer: false, replaces: Replaces::Nothing, pivot: Some((cx, cy)).filter(|p| *p != (0.0, 0.0)), origin: None, moves: true, reveals: false, tier: TIER_RADIUS })
            }
            "x" | "y" | "cx" | "cy" => {
                let ok = match attr {
                    "x" | "y" => matches!(node_tag, "rect" | "image" | "use" | "svg"),
                    _ => matches!(node_tag, "circle" | "ellipse"),
                };
                if !ok {
                    return Err(format!("{attr} on <{node_tag}> has no animation channel"));
                }
                let horizontal = attr == "x" || attr == "cx";
                let percent_of = if horizontal { self.vp.0 } else { self.vp.1 };
                let at = self.length(node, attr, 16.0, percent_of).unwrap_or(0.0);
                let parse_one = |s: &str| -> Option<Vec<f64>> { user_length(s).map(|v| vec![v]) };
                let values = derive_values(&a, &[at], &parse_one)?;
                let values: Vec<Vec<f64>> = values.into_iter().map(|v| vec![v[0] - at]).collect();
                let curve = Curve::new(values, kt, calc, ks)?;
                let prop = if horizontal { AnimatedProperty::TranslateX } else { AnimatedProperty::TranslateY };
                Ok(Spec { props: vec![prop], curve, timing, base: vec![0.0], layer: false, replaces: Replaces::Nothing, pivot: None, origin: None, moves: true, reveals: false, tier: TIER_POSITION })
            }
            "visibility" | "display" => {
                let hidden = if attr == "visibility" {
                    self.computed_prop(node, "visibility").is_some_and(|v| matches!(v.trim(), "hidden" | "collapse"))
                } else {
                    self.props_for(node).get("display").is_some_and(|v| v.trim() == "none")
                };
                let base = if hidden { 0.0 } else { 1.0 };
                let parse_one = |s: &str| -> Option<Vec<f64>> {
                    let s = s.trim();
                    let shown = if attr == "visibility" {
                        match s {
                            "visible" => true,
                            "hidden" | "collapse" => false,
                            _ => return None,
                        }
                    } else {
                        match s {
                            "none" => false,
                            "" | "inherit" | "initial" | "unset" | "revert" => return None,
                            _ => true,
                        }
                    };
                    Some(vec![if shown { 1.0 } else { 0.0 }])
                };
                let values = derive_values(&a, &[base], &parse_one)?;
                if hidden && values.iter().all(|v| v[0] == 0.0) {
                    return Err(format!("the element is hidden and the animation never changes its {attr}"));
                }
                // The values of these properties cannot be blended, so they switch.
                let calc = if tag == "set" { calc } else { Some("discrete") };
                let curve = Curve::new(values, kt, calc, ks)?;
                Ok(Spec { props: vec![AnimatedProperty::Opacity], curve, timing, base: vec![base], layer: true, replaces: Replaces::Nothing, pivot: None, origin: None, moves: false, reveals: hidden, tier: TIER_OPACITY })
            }
            "" => Err("it has no attributeName".into()),
            other => Err(format!("animating {other} is not supported; only opacity, fill-opacity, stroke-opacity, visibility, display, x, y, cx, cy, r and transform have a channel")),
        }
    }

    /// An `animateMotion`: the path from `mpath`, `path` or `values` (in that
    /// order of precedence), sampled into x, y and, when the element turns, a
    /// heading.
    fn motion_spec(&mut self, a: Node<'a, 'i>, timing: Timing, notes: &mut Vec<String>) -> Result<Spec, String> {
        let rotate = motion::parse_rotate(a.attribute("rotate"))?;
        let calc = a.attribute("calcMode").map(str::trim).filter(|c| !c.is_empty());
        match calc {
            None | Some("paced") | Some("linear") => {}
            Some(c @ ("discrete" | "spline")) => return Err(format!("calcMode=\"{c}\" is not supported on animateMotion")),
            Some(other) => return Err(format!("calcMode=\"{other}\" is not known")),
        }
        if a.attribute("accumulate").map(str::trim) == Some("sum") {
            notes.push("accumulate=\"sum\" was ignored; every repeat starts at the beginning of the path".into());
        }
        let mpath = a.children().find(|c| c.is_element() && c.tag_name().name() == "mpath");
        // Where on the path each value is, when the path was built from `values`.
        let mut at_values: Option<Vec<f64>> = None;
        let cmds: Vec<PathCommand> = if let Some(m) = mpath {
            let href = m.attribute("href").or_else(|| m.attribute((crate::XLINK_NS, "href"))).map(str::trim).and_then(|h| h.strip_prefix('#'));
            let Some(target) = href.and_then(|id| self.ids.get(id).copied()) else {
                return Err("its mpath does not point at an element".into());
            };
            if target.tag_name().name() != "path" {
                return Err(format!("its mpath points at a <{}>, and only a <path> is supported", target.tag_name().name()));
            }
            let d = target.attribute("d").ok_or("the path of its mpath has no d")?;
            if target.attribute("transform").is_some() {
                notes.push("the transform of the path of the mpath was ignored".into());
            }
            parse_d(d).map_err(|e| format!("the path of its mpath cannot be read ({e})"))?
        } else if let Some(d) = a.attribute("path") {
            parse_d(d).map_err(|e| format!("path=\"{}\" cannot be read ({e})", d.trim()))?
        } else {
            let parse_one = |s: &str| -> Option<Vec<f64>> { strict_numbers(s).filter(|n| n.len() == 2) };
            let points = derive_values(&a, &[0.0, 0.0], &parse_one).map_err(|e| format!("it has no path or mpath and {e}"))?;
            if points.len() < 2 {
                return Err("it has only one point to move through".into());
            }
            let pts: Vec<(f64, f64)> = points.iter().map(|p| (p[0], p[1])).collect();
            at_values = Some(motion::Flat::fractions_of(&pts));
            let mut cmds = vec![PathCommand::MoveTo(Point { x: pts[0].0, y: pts[0].1 })];
            cmds.extend(pts[1..].iter().map(|p| PathCommand::LineTo(Point { x: p.0, y: p.1 })));
            cmds
        };

        // keyTimes and keyPoints. Over a path they decide the progress at each
        // key time. Over `values` each value is a point, and with calcMode
        // linear the values are evenly spaced in time unless keyTimes says otherwise.
        let key_times = a.attribute("keyTimes").map(|k| motion::parse_list(k).ok_or("keyTimes is not a list of numbers")).transpose()?;
        // A keyPoints with an error is dropped and the animation runs without it, as the SVG spec says.
        let key_points = match a.attribute("keyPoints") {
            None => None,
            Some(k) => match motion::parse_list(k) {
                None => {
                    notes.push("keyPoints is not a list of numbers and was ignored".into());
                    None
                }
                Some(p) if p.iter().any(|v| !(0.0..=1.0).contains(v)) => {
                    notes.push("keyPoints must lie in 0 to 1 and was ignored".into());
                    None
                }
                Some(p) if key_times.as_ref().is_some_and(|t| t.len() != p.len()) => {
                    notes.push("keyPoints and keyTimes have different lengths and keyPoints was ignored".into());
                    None
                }
                Some(p) => Some(p),
            },
        };
        let keys: Option<(Vec<f64>, Vec<f64>)> = match (&at_values, key_points, key_times) {
            (Some(points), _, kt) => {
                if calc == Some("linear") {
                    let n = points.len();
                    Some((kt.unwrap_or_else(|| (0..n).map(|i| i as f64 / (n - 1) as f64).collect()), points.clone()))
                } else {
                    None
                }
            }
            (None, Some(kp), Some(kt)) => Some((kt, kp)),
            (None, Some(_), None) => {
                notes.push("keyPoints without keyTimes was ignored".into());
                None
            }
            (None, None, _) => None,
        };

        let flat = motion::flatten(&cmds, rotate)?;
        let turns = rotate.turns();
        let curve = motion::curve(&flat, keys.as_ref().map(|(t, p)| (t.as_slice(), p.as_slice())), turns, timing.dur)?;
        let (props, base) = if turns {
            (vec![AnimatedProperty::TranslateX, AnimatedProperty::TranslateY, AnimatedProperty::Rotate], vec![0.0, 0.0, 0.0])
        } else {
            (vec![AnimatedProperty::TranslateX, AnimatedProperty::TranslateY], vec![0.0, 0.0])
        };
        Ok(Spec { props, curve, timing, base, layer: false, replaces: Replaces::Nothing, pivot: None, origin: None, moves: true, reveals: false, tier: TIER_MOTION })
    }

    /// Wraps `content` for one animation and files its tracks for later.
    fn wrap_animation(&mut self, spec: &Spec, content: Vec<Element>) -> Vec<Element> {
        if self.pending.len() >= MAX_ANIMATIONS {
            self.warn(format!("the SVG has more than {MAX_ANIMATIONS} animations; the rest are drawn static"));
            // An element that only an animation shows stays hidden.
            return if spec.reveals { Vec::new() } else { content };
        }
        let id = loop {
            self.next_anim += 1;
            let id = format!("anim{}", self.next_anim);
            if !self.ids.contains_key(id.as_str()) && self.used_ids.insert(id.clone()) {
                break id;
            }
        };
        self.pending.push(Pending { target: id.clone(), props: spec.props.clone(), curve: spec.curve.clone(), timing: spec.timing, base: spec.base.clone() });
        if spec.layer {
            let mut layer = Layer::new(content);
            layer.id = Some(id);
            return vec![Element::Layer(layer)];
        }
        let around = |children: Vec<Element>, transform: Option<Transform>| vec![Element::Group(Group { children, id: None, transform, style: None })];
        let mut inner = content;
        if let Some((cx, cy)) = spec.pivot {
            inner = around(inner, translate(-cx, -cy));
        }
        let mut wrapped = vec![Element::Group(Group { children: inner, id: Some(id), transform: None, style: None })];
        if let Some((cx, cy)) = spec.pivot {
            wrapped = around(wrapped, translate(cx, cy));
        }
        wrapped
    }

    /// The walk of an element that carries animations: the element is walked
    /// without its own transform and opacity, wrapped once per animation, and
    /// the static transform goes back on the outside.
    pub(crate) fn walk_animated(&mut self, node: Node<'a, 'i>, parent: &crate::style::Inh, out: &mut Vec<Element>, expanded: bool) {
        if self.clip_mode {
            return self.walk_plain(node, parent, out, expanded);
        }
        if self.anim_off > 0 {
            let msg = format!("{} has an animation inside a mask or marker, which is not supported; the static frame is drawn", Self::label(&node));
            self.warn(msg);
            return self.walk_plain(node, parent, out, expanded);
        }
        let mut specs = self.animation_specs(node);
        specs.sort_by_key(|s| s.tier);
        if specs.is_empty() {
            return self.walk_plain(node, parent, out, expanded);
        }
        let moves = specs.iter().any(|s| s.moves);
        let replaces = |what: Replaces| specs.iter().any(|s| s.replaces == what);
        let (fades, fill_fades, stroke_fades) = (replaces(Replaces::Opacity), replaces(Replaces::FillOpacity), replaces(Replaces::StrokeOpacity));
        let static_transform = if moves { self.node_transform(node) } else { None };
        let saved_force = self.force_visible;
        if specs.iter().any(|s| s.reveals) {
            self.force_visible = Some(node.id());
        }
        let saved = (self.skip_transform, self.skip_opacity, self.skip_fill_opacity, self.skip_stroke_opacity);
        if moves {
            self.skip_transform = Some(node.id());
        }
        if fades {
            self.skip_opacity = Some(node.id());
        }
        if fill_fades {
            self.skip_fill_opacity = Some(node.id());
        }
        if stroke_fades {
            self.skip_stroke_opacity = Some(node.id());
        }
        let mut content = Vec::new();
        self.walk_plain(node, parent, &mut content, expanded);
        (self.skip_transform, self.skip_opacity, self.skip_fill_opacity, self.skip_stroke_opacity) = saved;
        self.force_visible = saved_force;
        if content.is_empty() {
            return;
        }
        for spec in specs.iter_mut() {
            if let Some(o) = spec.origin.take() {
                spec.pivot = self.resolve_origin(&o, &content);
            }
        }
        for spec in specs.iter().rev() {
            content = self.wrap_animation(spec, content);
        }
        if let Some(t) = static_transform {
            content = vec![Element::Group(Group { children: content, id: None, transform: Some(t), style: None })];
        }
        out.extend(content);
    }

    /// Bakes every pending animation onto one scene timeline and fills in the
    /// scene's `animations`, `duration` and `loop_mode`.
    pub(crate) fn finish_animations(&mut self, scene: &mut Scene) {
        if self.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        let spans: Vec<Span> = pending.iter().map(|p| Span { period: p.timing.dur, indefinite: p.timing.active.is_none(), end: p.timing.end(), removes: !p.timing.freeze }).collect();
        let plan = anim::plan(&spans);
        for n in &plan.notes {
            self.warn(n.clone());
        }
        if plan.duration <= 0.0 {
            return;
        }
        let mut tracks = Vec::new();
        for p in &pending {
            match anim::bake(&p.curve, &p.timing, &p.base, plan.duration) {
                Ok(points) => {
                    for (i, prop) in p.props.iter().enumerate() {
                        let keys: Vec<Keyframe> = points.iter().map(|(t, v)| Keyframe::linear(*t, v[i])).collect();
                        if keys.is_empty() || keys.iter().all(|k| (k.value - p.base[i]).abs() < 1e-12) {
                            continue;
                        }
                        tracks.push(AnimationTrack::new(p.target.clone(), *prop, keys));
                    }
                }
                Err(why) => self.warn(format!("an animation was left static: {why}")),
            }
        }
        if tracks.is_empty() {
            return;
        }
        scene.animations = tracks;
        scene.duration = plan.duration;
        scene.loop_mode = if plan.looping { LoopMode::Loop } else { LoopMode::Once };
    }
}
