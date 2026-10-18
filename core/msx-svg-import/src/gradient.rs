// core/msx-svg-import/src/gradient.rs
//! SVG gradients to MSX gradients.
//!
//! MSX gradients live in the unit square of the shape's bounding box and
//! have no transform. SVG gradients may use user-space coordinates and a
//! `gradientTransform`. For each shape that references a gradient the
//! importer builds an MSX gradient in that shape's bounding-box space:
//!
//! - A linear gradient maps exactly under any affine transform. Its
//!   parameter is an affine function of position, and every such function
//!   is a linear gradient with suitable end points.
//! - A radial gradient maps exactly when the transform is a similarity
//!   (uniform scale, rotation, translation). Otherwise the circle becomes
//!   a tilted or stretched ellipse that MSX cannot express, so it is
//!   approximated by a circle of the same area and a warning is recorded.
//! - A shape without a bounding box area (a perfectly horizontal or
//!   vertical line) cannot use a user-space gradient, so it gets the
//!   average color of the stops.

use msx_ast::{Color, Def, LinearGradient, Matrix2D, Paint, RadialGradient, Stop};
use roxmltree::Node;

use crate::geom::{apply, invert, mul, BBox};
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

/// A transform entry as a matrix.
fn matrix_of(t: &msx_ast::Transform) -> Matrix2D {
    t.to_matrix()
}

/// Gradient geometry in the target (MSX unit-square) space.
#[derive(Debug, Clone, PartialEq)]
enum Geometry {
    Linear { x1: f64, y1: f64, x2: f64, y2: f64 },
    Radial { cx: f64, cy: f64, r: f64, fx: f64, fy: f64 },
    /// Everything collapses to one color: the last stop.
    Solid,
}

/// Average of the stop colors, the fallback when a gradient cannot be used.
fn average(stops: &[(f64, Color)]) -> Color {
    let n = stops.len().max(1) as f64;
    let sum = |f: fn(&Color) -> u8| (stops.iter().map(|(_, c)| f(c) as f64).sum::<f64>() / n).round() as u8;
    Color::rgba(sum(|c| c.r), sum(|c| c.g), sum(|c| c.b), sum(|c| c.a))
}

/// End points of the MSX linear gradient equal to the SVG one after the
/// affine map `t` (gradient space to MSX unit space).
fn linear_through(t: Matrix2D, p1: (f64, f64), p2: (f64, f64)) -> Option<Geometry> {
    let inv = invert(t)?;
    let d = (p2.0 - p1.0, p2.1 - p1.1);
    let dd = d.0 * d.0 + d.1 * d.1;
    if dd < 1e-18 {
        return Some(Geometry::Solid);
    }
    // parameter(u) = u . k + b, with k = A^T d / |d|^2 and b = (c - P1) . d / |d|^2,
    // where g = A u + c is the inverse map from unit space back to gradient space.
    let k = ((inv.a * d.0 + inv.b * d.1) / dd, (inv.c * d.0 + inv.d * d.1) / dd);
    let b = ((inv.e - p1.0) * d.0 + (inv.f - p1.1) * d.1) / dd;
    let kk = k.0 * k.0 + k.1 * k.1;
    if kk < 1e-18 {
        return Some(Geometry::Solid);
    }
    let q1 = (-b * k.0 / kk, -b * k.1 / kk);
    Some(Geometry::Linear { x1: q1.0, y1: q1.1, x2: q1.0 + k.0 / kk, y2: q1.1 + k.1 / kk })
}

/// The MSX radial gradient for the SVG one after `t`, and whether that is
/// only an approximation.
fn radial_through(t: Matrix2D, c: (f64, f64), f: (f64, f64), r: f64) -> (Geometry, bool) {
    let (cx, cy) = apply(t, c.0, c.1);
    let (fx, fy) = apply(t, f.0, f.1);
    let (n1, n2) = ((t.a * t.a + t.b * t.b).sqrt(), (t.c * t.c + t.d * t.d).sqrt());
    let dot = t.a * t.c + t.b * t.d;
    let conformal = (n1 - n2).abs() <= 1e-6 * n1.max(n2) && dot.abs() <= 1e-6 * n1 * n2;
    let scale = if conformal { n1 } else { (t.a * t.d - t.b * t.c).abs().sqrt() };
    (Geometry::Radial { cx, cy, r: r * scale, fx, fy }, !conformal)
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
            self.warn(format!("paint #{id} is a <{}>, which is not supported; only gradients are imported", node.tag_name().name()));
            return fallback_paint();
        }

        let key = format!("{id}|{opacity:.5}|{}", bbox.map_or("none".to_string(), |b| format!("{:.6},{:.6},{:.6},{:.6}", b.x, b.y, b.w, b.h)));
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
        if let Some(spread) = shared("spreadMethod") {
            if spread.trim() != "pad" {
                self.warn(format!("gradient #{base_id} uses spreadMethod=\"{}\", imported as pad", spread.trim()));
            }
        }
        let gtransform = match shared("gradientTransform") {
            Some(s) => match parse_transform(s) {
                Ok(t) => t.as_ref().map(matrix_of).unwrap_or_else(Matrix2D::identity),
                Err(e) => {
                    self.warn(format!("gradient #{base_id}: {e}; transform ignored"));
                    Matrix2D::identity()
                }
            },
            None => Matrix2D::identity(),
        };

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
                Some(l) if user => l.to_px(16.0, if name == "r" { diagonal(vp) } else { base }),
                Some(l) => match l.unit {
                    crate::parse::Unit::Percent => l.value / 100.0,
                    _ => l.value,
                },
                None if user => default_frac * base,
                None => default_frac,
            }
        };

        // Gradient space to the shape's bounding-box (MSX unit) space.
        let (t, usable) = if user {
            match bbox.filter(BBox::has_area).and_then(|b| invert(b.unit_to_box())) {
                Some(inv_box) => (mul(inv_box, gtransform), true),
                None => (gtransform, false),
            }
        } else {
            (gtransform, true)
        };

        if !usable {
            self.warn(format!("gradient #{base_id} on a shape with an empty bounding box is imported as its average color"));
            return Some(Paint::Color(average(&stops)));
        }

        let geometry = if radial {
            let (cx, cy) = (coord("cx", 0.5, true, true), coord("cy", 0.5, false, true));
            let r = coord("r", 0.5, true, true);
            let fx = if own("fx").is_some() { coord("fx", 0.5, true, true) } else { cx };
            let fy = if own("fy").is_some() { coord("fy", 0.5, false, true) } else { cy };
            if own("fr").and_then(parse_length).is_some_and(|l| l.value != 0.0) {
                self.warn(format!("radial gradient #{base_id} uses fr, which is not supported; ignored"));
            }
            if (fx - cx).hypot(fy - cy) > r * (1.0 + 1e-9) {
                self.warn(format!("radial gradient #{base_id} has its focal point outside the circle; it is moved onto the circle edge"));
            }
            if r <= 0.0 {
                Geometry::Solid
            } else {
                let (g, approximated) = radial_through(t, (cx, cy), (fx, fy), r);
                if approximated {
                    self.warn(format!("radial gradient #{base_id} is stretched or skewed by its transform, approximated by a circle"));
                }
                g
            }
        } else {
            let p1 = (coord("x1", 0.0, true, true), coord("y1", 0.0, false, true));
            let p2 = (coord("x2", 1.0, true, true), coord("y2", 0.0, false, true));
            match linear_through(t, p1, p2) {
                Some(g) => g,
                None => {
                    self.warn(format!("gradient #{base_id} has a degenerate transform, imported as its average color"));
                    return Some(Paint::Color(average(&stops)));
                }
            }
        };

        let msx_stops: Vec<Stop> = stops.iter().map(|(o, c)| Stop::new(*o, *c)).collect();
        let def_id = self.fresh_gradient_id(base_id);
        let def = match geometry {
            Geometry::Solid => return Some(Paint::Color(stops[stops.len() - 1].1)),
            Geometry::Linear { x1, y1, x2, y2 } => Def::LinearGradient(LinearGradient::new(def_id.clone(), x1, y1, x2, y2, msx_stops)),
            Geometry::Radial { cx, cy, r, fx, fy } => Def::RadialGradient(RadialGradient::new(def_id.clone(), cx, cy, r, fx, fy, msx_stops)),
        };
        self.defs.push(def);
        Some(Paint::Ref(format!("url(#{def_id})")))
    }

    /// An id for a new gradient def: the SVG id when free, a numbered
    /// variant otherwise. Characters that would break `url(#...)` are replaced.
    fn fresh_gradient_id(&mut self, base: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The SVG linear parameter at gradient-space point g.
    fn svg_t(p1: (f64, f64), p2: (f64, f64), g: (f64, f64)) -> f64 {
        let d = (p2.0 - p1.0, p2.1 - p1.1);
        ((g.0 - p1.0) * d.0 + (g.1 - p1.1) * d.1) / (d.0 * d.0 + d.1 * d.1)
    }

    /// The MSX linear parameter at unit-square point u.
    fn msx_t(q1: (f64, f64), q2: (f64, f64), u: (f64, f64)) -> f64 {
        svg_t(q1, q2, u)
    }

    #[test]
    fn linear_conversion_is_exact_under_rotation_scale_skew_and_translation() {
        let transforms = [
            Matrix2D { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 },
            Matrix2D { a: 0.6, b: 0.8, c: -0.8, d: 0.6, e: 0.3, f: -0.2 },
            Matrix2D { a: 2.0, b: 0.0, c: 0.0, d: 0.5, e: 0.1, f: 0.4 },
            Matrix2D { a: 1.0, b: 0.3, c: 0.7, d: 1.2, e: -0.5, f: 0.25 },
            Matrix2D { a: -1.0, b: 0.0, c: 0.0, d: 1.0, e: 1.0, f: 0.0 },
        ];
        let (p1, p2) = ((0.1, 0.2), (0.9, 0.7));
        for t in transforms {
            let Some(Geometry::Linear { x1, y1, x2, y2 }) = linear_through(t, p1, p2) else { panic!("expected a linear gradient for {t:?}") };
            for u in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5), (0.2, 0.9), (-0.3, 1.4)] {
                // The point of gradient space that lands on unit-square point u.
                let g = apply(invert(t).unwrap(), u.0, u.1);
                let want = svg_t(p1, p2, g);
                let got = msx_t((x1, y1), (x2, y2), u);
                assert!((want - got).abs() < 1e-9, "t {t:?} at {u:?}: svg {want} msx {got}");
            }
        }
    }

    #[test]
    fn degenerate_linear_gradients_become_solid() {
        let id = Matrix2D::identity();
        assert_eq!(linear_through(id, (0.3, 0.3), (0.3, 0.3)), Some(Geometry::Solid));
        // A transform that flattens everything onto a line along the gradient normal.
        assert!(linear_through(Matrix2D { a: 0.0, b: 0.0, c: 0.0, d: 0.0, e: 0.0, f: 0.0 }, (0.0, 0.0), (1.0, 0.0)).is_none());
    }

    #[test]
    fn radial_under_a_similarity_is_exact() {
        // Rotate 90 degrees, scale by 2, translate: still a circle.
        let t = Matrix2D { a: 0.0, b: 2.0, c: -2.0, d: 0.0, e: 0.5, f: 0.5 };
        let (g, approx) = radial_through(t, (0.0, 0.0), (0.1, 0.0), 0.25);
        assert!(!approx);
        let Geometry::Radial { cx, cy, r, fx, fy } = g else { panic!("expected radial") };
        assert!((cx - 0.5).abs() < 1e-12 && (cy - 0.5).abs() < 1e-12);
        assert!((r - 0.5).abs() < 1e-12);
        assert!((fx - 0.5).abs() < 1e-12 && (fy - 0.7).abs() < 1e-12);
    }

    #[test]
    fn radial_under_a_stretch_is_flagged_and_keeps_the_area() {
        let t = Matrix2D { a: 2.0, b: 0.0, c: 0.0, d: 0.5, e: 0.0, f: 0.0 };
        let (g, approx) = radial_through(t, (0.5, 0.5), (0.5, 0.5), 0.4);
        assert!(approx);
        let Geometry::Radial { r, .. } = g else { panic!("expected radial") };
        assert!((r - 0.4).abs() < 1e-12, "scales 2 and 0.5 have geometric mean 1, got r {r}");
    }

    #[test]
    fn average_color_of_stops() {
        let stops = vec![(0.0, Color::rgb(0, 0, 0)), (1.0, Color::rgb(200, 100, 50))];
        assert_eq!(average(&stops), Color::rgb(100, 50, 25));
    }
}
