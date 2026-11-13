// core/msx-svg-import/src/gradient.rs
//! SVG gradients to MSX gradients.
//!
//! From wire version 3 an MSX linear or radial gradient has the same geometry
//! as the SVG one: `gradientUnits`, `spreadMethod` and `gradientTransform` are
//! carried over as they are, so nothing is approximated and one gradient def
//! serves every shape that uses it (one per opacity, which is folded into the
//! stop colors). The importer only resolves what SVG resolves for it:
//!
//! - attributes and stops that come through an `href` chain;
//! - lengths and percentages, in user units or as fractions of the box;
//! - a gradient whose two points coincide, or whose radius is zero, which SVG
//!   paints as its last stop color;
//! - a bounding-box gradient on a shape without area, which paints the
//!   fallback color or nothing.
//!
//! The focal radius `fr` is carried too, and a negative one counts as 0.

use msx_ast::{Color, Def, GradientUnits, LinearGradient, Paint, RadialGradient, SpreadMethod, Stop, Transform};
use roxmltree::Node;

use crate::geom::BBox;
use crate::parse::{parse_color, parse_length, parse_number, parse_transform};
use crate::style::{diagonal, with_alpha};
use crate::Importer;

const XLINK: &str = "http://www.w3.org/1999/xlink";

fn is_gradient(n: &Node) -> bool {
    n.is_element() && matches!(n.tag_name().name(), "linearGradient" | "radialGradient")
}

fn href<'a>(n: &Node<'a, '_>) -> Option<&'a str> {
    n.attribute("href").or_else(|| n.attribute((XLINK, "href"))).map(|h| h.trim().trim_start_matches('#'))
}

impl<'a, 'i> Importer<'a, 'i> {
    /// The paint for `url(#id)`, or `None` when nothing should be drawn.
    ///
    /// `opacity` is folded into every stop alpha, because the raster
    /// renderers do not read `fill-opacity` or `stroke-opacity`.
    pub(crate) fn paint_for_url(&mut self, id: &str, fallback: &Option<Option<Color>>, bbox: Option<BBox>, opacity: f64) -> Option<Paint> {
        let fallback_paint = || match fallback {
            Some(Some(c)) => Some(Paint::Color(with_alpha(*c, opacity))),
            _ => None,
        };
        let Some(node) = self.ids.get(id).copied() else {
            self.warn(format!("paint refers to #{id}, which does not exist"));
            return fallback_paint();
        };
        if !is_gradient(&node) {
            if node.tag_name().name() == "pattern" {
                // A fill is drawn as tiles once the shape exists. A stroke is not.
                if self.pattern_fill_ok {
                    return Some(crate::pattern::mark(id, opacity, bbox));
                }
                self.warn(format!("paint #{id} is a <pattern> used on a stroke, which is not supported; patterns work on fills only"));
                return fallback_paint();
            }
            self.warn(format!("paint #{id} is a <{}>, which is not supported; only gradients are imported", node.tag_name().name()));
            return fallback_paint();
        }

        // The def does not depend on the shape, only on whether its box has area (a gradient in
        // box units has nothing to paint without).
        let key = format!("{id}|{opacity:.5}|{}|{fallback:?}", bbox.is_some_and(|b| b.has_area()));
        if let Some(hit) = self.grad_cache.get(&key) {
            return hit.clone();
        }
        let result = self.build_gradient(id, node, bbox, opacity, fallback_paint());
        self.grad_cache.insert(key, result.clone());
        result
    }

    fn chain(&self, start: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
        let mut out = vec![start];
        let mut cur = start;
        while let Some(target) = href(&cur).and_then(|h| self.ids.get(h).copied()) {
            if !is_gradient(&target) || out.iter().any(|n| n.id() == target.id()) || out.len() >= 16 {
                break;
            }
            out.push(target);
            cur = target;
        }
        out
    }

    fn build_gradient(&mut self, base_id: &str, node: Node<'a, 'i>, bbox: Option<BBox>, opacity: f64, fallback: Option<Paint>) -> Option<Paint> {
        let chain = self.chain(node);
        let radial = node.tag_name().name() == "radialGradient";
        // Shared attributes come from anywhere in the chain. Shape
        // attributes only from gradients of the same kind.
        let shared = |name: &str| chain.iter().find_map(|n| n.attribute(name));
        let own = |name: &str| chain.iter().filter(|n| n.tag_name().name() == node.tag_name().name()).find_map(|n| n.attribute(name));

        let user = shared("gradientUnits").is_some_and(|u| u.trim() == "userSpaceOnUse");
        let spread = match shared("spreadMethod").map(str::trim) {
            None | Some("pad") => SpreadMethod::Pad,
            Some(name) => SpreadMethod::from_name(name).unwrap_or_else(|| {
                self.warn(format!("gradient #{base_id} has spreadMethod=\"{name}\", which is not known; imported as pad"));
                SpreadMethod::Pad
            }),
        };
        let transform: Option<Transform> = match shared("gradientTransform") {
            Some(s) => match parse_transform(s) {
                Ok(t) => t,
                Err(e) => {
                    self.warn(format!("gradient #{base_id}: {e}; transform ignored"));
                    None
                }
            },
            None => None,
        };
        if transform.is_some() && msx_ast::usable_transform(&transform).is_none() {
            self.warn(format!("gradient #{base_id} has a gradientTransform that cannot be undone; it is ignored"));
        }

        // Stops come from the first gradient in the chain that has any.
        let stops_node = chain.iter().find(|n| n.children().any(|c| c.is_element() && c.tag_name().name() == "stop"));
        let mut stops: Vec<(f64, Color)> = Vec::new();
        if let Some(sn) = stops_node {
            let mut floor = 0.0f64;
            for stop in sn.children().filter(|c| c.is_element() && c.tag_name().name() == "stop") {
                let raw = stop.attribute("offset").unwrap_or("0").trim();
                let off = match raw.strip_suffix('%') {
                    Some(p) => parse_number(p).map(|v| v / 100.0),
                    None => parse_number(raw),
                }
                .unwrap_or(0.0)
                .clamp(0.0, 1.0)
                .max(floor);
                floor = off;
                let stop_color = self.inherited_prop(stop, "stop-color");
                let color = match stop_color.as_deref().map(str::trim) {
                    Some(v) if v.eq_ignore_ascii_case("currentcolor") => self.color_of(stop),
                    Some(v) => parse_color(v).unwrap_or(Color::BLACK),
                    None => Color::BLACK,
                };
                let stop_opacity = self.inherited_prop(stop, "stop-opacity");
                let so = stop_opacity
                    .as_deref()
                    .map(|v| v.trim())
                    .and_then(|v| match v.strip_suffix('%') {
                        Some(p) => parse_number(p).map(|n| n / 100.0),
                        None => parse_number(v),
                    })
                    .unwrap_or(1.0);
                stops.push((off, with_alpha(color, so.clamp(0.0, 1.0) * opacity)));
            }
        }
        match stops.len() {
            0 => return None,
            1 => return Some(Paint::Color(stops[0].1)),
            _ => {}
        }

        // A bounding-box gradient needs a box with area, or the fallback
        // color (if any) is painted instead.
        if !user && !bbox.is_some_and(|b| b.has_area()) {
            return fallback;
        }

        // Parse a coordinate: a fraction in bounding-box units, user units otherwise.
        let vp = self.vp;
        let coord = |name: &str, default_frac: f64, along_x: bool, own_lookup: bool| -> f64 {
            let raw = if own_lookup { own(name) } else { shared(name) };
            let base = if along_x { vp.0 } else { vp.1 };
            match raw.and_then(parse_length) {
                Some(l) if user => l.to_px(16.0, if name == "r" || name == "fr" { diagonal(vp) } else { base }),
                Some(l) => match l.unit {
                    crate::parse::Unit::Percent => l.value / 100.0,
                    _ => l.value,
                },
                None if user => default_frac * base,
                None => default_frac,
            }
        };

        let units = if user { GradientUnits::UserSpace } else { GradientUnits::ObjectBoundingBox };
        let last = stops[stops.len() - 1].1;
        let msx_stops: Vec<Stop> = stops.iter().map(|(o, c)| Stop::new(*o, *c)).collect();
        let def_id = self.fresh_gradient_id(base_id);

        let def = if radial {
            let (cx, cy) = (coord("cx", 0.5, true, true), coord("cy", 0.5, false, true));
            let r = coord("r", 0.5, true, true);
            let fx = if own("fx").is_some() { coord("fx", 0.5, true, true) } else { cx };
            let fy = if own("fy").is_some() { coord("fy", 0.5, false, true) } else { cy };
            // A radius of 0 is a focal point. A negative one is an error in SVG and counts as 0.
            let fr = if own("fr").is_some() { coord("fr", 0.0, true, true) } else { 0.0 };
            if r <= 0.0 {
                // A radius of zero paints the last stop color.
                return Some(Paint::Color(last));
            }
            Def::RadialGradient(RadialGradient::new(def_id.clone(), cx, cy, r, fx, fy, msx_stops).with_units(units).with_spread(spread).with_fr(fr))
        } else {
            let p1 = (coord("x1", 0.0, true, true), coord("y1", 0.0, false, true));
            let p2 = (coord("x2", 1.0, true, true), coord("y2", 0.0, false, true));
            if (p2.0 - p1.0).hypot(p2.1 - p1.1) < 1e-12 {
                // Two coinciding points paint the last stop color.
                return Some(Paint::Color(last));
            }
            Def::LinearGradient(LinearGradient::new(def_id.clone(), p1.0, p1.1, p2.0, p2.1, msx_stops).with_units(units).with_spread(spread))
        };
        let def = match (def, transform) {
            (Def::LinearGradient(g), Some(t)) => Def::LinearGradient(g.with_transform(t)),
            (Def::RadialGradient(g), Some(t)) => Def::RadialGradient(g.with_transform(t)),
            (d, _) => d,
        };
        self.defs.push(def);
        Some(Paint::Ref(format!("url(#{def_id})")))
    }

    /// An id for a new gradient def: the SVG id when free, a numbered
    /// variant otherwise. Characters that would break `url(#...)` are replaced.
    pub(crate) fn fresh_gradient_id(&mut self, base: &str) -> String {
        let clean: String = base.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
        let clean = if clean.is_empty() { "gradient".to_string() } else { clean };
        let mut candidate = clean.clone();
        while self.used_ids.contains(&candidate) {
            self.next_id += 1;
            candidate = format!("{clean}-{}", self.next_id);
        }
        self.used_ids.insert(candidate.clone());
        candidate
    }
}
