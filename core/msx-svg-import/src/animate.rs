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
//!   opacity the same way on every renderer.
//!
//! Several animations on one element nest in document order, first outermost.
//! Anything that has no channel (colors, paths, dash offsets, skew, motion
//! paths, `set`) is reported and the static value is drawn.

use std::collections::HashMap;

use msx_ast::{AnimatedProperty, AnimationTrack, Element, Group, Keyframe, Layer, LoopMode, Scene, Transform};
use roxmltree::{Document, Node, NodeId};

use crate::anim::{self, Curve, Span, Timing};
use crate::style::element_opacity;
use crate::{is_foreign, translate, Importer};

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
    /// Centre of a rotation.
    pivot: Option<(f64, f64)>,
    /// Moves the element, so the element's own transform goes outside the wrapper.
    moves: bool,
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

impl<'a, 'i> Importer<'a, 'i> {
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
        node.children().any(|c| c.is_element() && is_animation_tag(c.tag_name().name())) || self.anim_by_href.contains_key(&node.id())
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
        specs
    }

    fn build_spec(&mut self, node: Node<'a, 'i>, a: Node<'a, 'i>, notes: &mut Vec<String>) -> Result<Spec, String> {
        let tag = a.tag_name().name();
        match tag {
            "animateTransform" | "animate" => {}
            other => return Err(format!("<{other}> has no matching animation channel")),
        }
        let timing = Timing::parse(|k| a.attribute(k), notes)?;
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
            return Ok(Spec { props, curve, timing, base, layer: false, pivot: pivot.filter(|p| *p != (0.0, 0.0)), moves: true });
        }

        let attr = a.attribute("attributeName").map(str::trim).unwrap_or("");
        let node_tag = node.tag_name().name();
        match attr {
            "opacity" => {
                let base = element_opacity(&self.props_for(node));
                let parse_one = |s: &str| -> Option<Vec<f64>> { s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| vec![v.clamp(0.0, 1.0)]) };
                let values = derive_values(&a, &[base], &parse_one)?;
                let curve = Curve::new(values, kt, calc, ks)?;
                Ok(Spec { props: vec![AnimatedProperty::Opacity], curve, timing, base: vec![base], layer: true, pivot: None, moves: false })
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
                Ok(Spec { props: vec![prop], curve, timing, base: vec![0.0], layer: false, pivot: None, moves: true })
            }
            "" => Err("it has no attributeName".into()),
            other => Err(format!("animating {other} is not supported; only opacity, x, y, cx, cy and transform have a channel")),
        }
    }

    /// Wraps `content` for one animation and files its tracks for later.
    fn wrap_animation(&mut self, spec: &Spec, content: Vec<Element>) -> Vec<Element> {
        if self.pending.len() >= MAX_ANIMATIONS {
            self.warn(format!("the SVG has more than {MAX_ANIMATIONS} animations; the rest are drawn static"));
            return content;
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
        let specs = self.animation_specs(node);
        if specs.is_empty() {
            return self.walk_plain(node, parent, out, expanded);
        }
        let moves = specs.iter().any(|s| s.moves);
        let fades = specs.iter().any(|s| s.layer);
        let static_transform = if moves { self.node_transform(node) } else { None };
        let saved = (self.skip_transform, self.skip_opacity);
        if moves {
            self.skip_transform = Some(node.id());
        }
        if fades {
            self.skip_opacity = Some(node.id());
        }
        let mut content = Vec::new();
        self.walk_plain(node, parent, &mut content, expanded);
        (self.skip_transform, self.skip_opacity) = saved;
        if content.is_empty() {
            return;
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
        let spans: Vec<Span> = pending.iter().map(|p| Span { period: p.timing.dur, indefinite: p.timing.active.is_none(), end: p.timing.end() }).collect();
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
