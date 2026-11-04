// core/msx-svg-import/src/pattern.rs
//! Pattern fills by expansion.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Patterns". MSX has no
//! pattern paint, so the importer draws the tiles: the pattern content is built
//! once, copied into every tile that meets the bounds of the shape, and the
//! copies are clipped to the shape. The shape itself keeps its stroke and loses
//! its fill. Every renderer draws the result the same way, at the cost of one
//! group per tile, which the tile limits below keep in check.
//!
//! A pattern fill leaves a marker in the style of the shape (`mark`), because
//! the paint is resolved before the shape exists. `push_leaf` turns the marker
//! into the tiles (`expand_pattern`).

use msx_ast::{Color, Element, Group, Layer, Matrix2D, Paint, Style, Transform};
use roxmltree::Node;

use crate::geom::{self, BBox};
use crate::parse::parse_transform;
use crate::{compose, elements_bbox, is_foreign, plain_rect, transform_mut, translate, view_box, viewbox_transform, Importer};

/// Most tiles one pattern fill may be expanded into.
const MAX_TILES: usize = 4096;
/// The same limit when each tile needs a layer of its own, because the content
/// reaches past the tile.
const MAX_CLIPPED_TILES: usize = 1024;

const MARK: &str = "\u{0}pattern:";

/// The paint a pattern fill leaves in the style of a shape until the shape is built.
pub(crate) fn mark(id: &str, opacity: f64, bbox: Option<BBox>) -> Paint {
    let b = bbox.map_or("none".to_string(), |b| format!("{},{},{},{}", b.x, b.y, b.w, b.h));
    Paint::Ref(format!("{MARK}{id}\u{0}{opacity}\u{0}{b}"))
}

pub(crate) fn is_mark(paint: &Option<Paint>) -> bool {
    matches!(paint, Some(Paint::Ref(r)) if r.starts_with(MARK))
}

fn parse_mark(r: &str) -> Option<(String, f64, Option<BBox>)> {
    let mut parts = r.strip_prefix(MARK)?.split('\u{0}');
    let id = parts.next()?.to_string();
    let opacity: f64 = parts.next()?.parse().ok()?;
    let b = parts.next()?;
    let bbox = if b == "none" {
        None
    } else {
        let v: Vec<f64> = b.split(',').filter_map(|x| x.parse().ok()).collect();
        (v.len() == 4).then(|| BBox { x: v[0], y: v[1], w: v[2], h: v[3] })
    };
    Some((id, opacity, bbox))
}

fn style_mut(el: &mut Element) -> Option<&mut Style> {
    match el {
        Element::Rect(e) => Some(&mut e.style),
        Element::Circle(e) => Some(&mut e.style),
        Element::Ellipse(e) => Some(&mut e.style),
        Element::Line(e) => Some(&mut e.style),
        Element::Polyline(e) | Element::Polygon(e) => Some(&mut e.style),
        Element::Path(e) => Some(&mut e.style),
        Element::Text(e) => Some(&mut e.style),
        _ => None,
    }
}

/// A number or a percentage as a fraction, for `objectBoundingBox` units.
fn fraction(s: &str) -> Option<f64> {
    let s = s.trim();
    let v = match s.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok()? / 100.0,
        None => s.parse::<f64>().ok()?,
    };
    v.is_finite().then_some(v)
}

impl<'a, 'i> Importer<'a, 'i> {
    /// The `pattern` elements an element inherits from through `href`, the element first.
    fn pattern_chain(&self, start: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
        let mut out = vec![start];
        let mut cur = start;
        while let Some(next) = cur.attribute("href").or_else(|| cur.attribute((crate::XLINK_NS, "href"))).and_then(|h| h.trim().strip_prefix('#')).and_then(|id| self.ids.get(id).copied()) {
            if next.tag_name().name() != "pattern" || out.iter().any(|n| n.id() == next.id()) || out.len() >= 16 {
                break;
            }
            out.push(next);
            cur = next;
        }
        out
    }

    /// If the fill of `el` is a pattern marker, removes the fill and returns
    /// the layer that draws the pattern in its place: the tiles, clipped to the
    /// shape, in the coordinate system of the shape. The caller puts the layer
    /// and the shape in one layer that carries the transform of the shape.
    pub(crate) fn expand_pattern(&mut self, el: &mut Element) -> Option<Element> {
        let style = style_mut(el)?;
        let Some(Paint::Ref(r)) = style.fill.clone() else { return None };
        let (id, opacity, bbox) = parse_mark(&r)?;
        style.fill = Some(Paint::None);
        // The shape and its tiles fade together, so the opacity moves to the
        // layer that holds both.
        style.opacity = Some(1.0);
        let mut clip = el.clone();
        if let Some(s) = style_mut(&mut clip) {
            *s = Style { fill: Some(Paint::Color(Color::BLACK)), stroke: Some(Paint::None), stroke_width: Some(0.0), opacity: Some(1.0), fill_rule: s.fill_rule, ..Style::default() };
        }
        // The transform of the shape moves to the layer that `push_leaf` puts
        // around the tiles and the shape, so the clip and the tiles have none.
        if let Some(slot) = transform_mut(&mut clip) {
            *slot = None;
        }
        let tiles = self.pattern_tiles(&id, bbox, &clip)?;
        let mut layer = Layer::new(tiles);
        layer.opacity = opacity;
        layer.clip_path = Some(vec![clip]);
        Some(Element::Layer(layer))
    }

    fn pattern_tiles(&mut self, id: &str, bbox: Option<BBox>, clip: &Element) -> Option<Vec<Element>> {
        let start = self.ids.get(id).copied()?;
        if self.pattern_stack.iter().any(|p| p == id) {
            self.warn(format!("pattern #{id} uses itself in its content; drawn without"));
            return None;
        }
        let chain = self.pattern_chain(start);
        let attr = |name: &str| chain.iter().find_map(|n| n.attribute(name));
        let content_node = chain.iter().find(|n| n.children().any(|c| c.is_element() && !is_foreign(&c))).copied()?;
        let shape = bbox.or_else(|| elements_bbox(std::slice::from_ref(clip)))?;

        let user_units = attr("patternUnits").is_some_and(|u| u.trim() == "userSpaceOnUse");
        let (vpw, vph) = self.vp;
        // In `objectBoundingBox` units a number is a share of the box, in user
        // space it is a length. A missing one is 0.
        let value = |name: &str, base: f64, extent: f64| -> Option<f64> {
            match chain.iter().find(|n| n.attribute(name).is_some()) {
                None => Some(0.0),
                Some(n) if user_units => self.length(*n, name, 16.0, base),
                Some(n) => fraction(n.attribute(name)?).map(|f| f * extent),
            }
        };
        let (ox, oy) = if user_units { (0.0, 0.0) } else { (shape.x, shape.y) };
        let tx = ox + value("x", vpw, shape.w)?;
        let ty = oy + value("y", vph, shape.h)?;
        let tw = value("width", vpw, shape.w)?;
        let th = value("height", vph, shape.h)?;
        if !(tw > 0.0 && th > 0.0 && tw.is_finite() && th.is_finite()) {
            return None;
        }

        let pt = attr("patternTransform").and_then(|t| parse_transform(t).ok().flatten());
        let mp = pt.as_ref().map_or(Matrix2D::identity(), Transform::to_matrix);
        let inv = geom::invert(mp)?;

        // The content, built once with the style it has where the pattern is defined.
        let inh = self.inh_chain(content_node);
        self.pattern_stack.push(id.to_string());
        self.anim_off += 1;
        let mut content = Vec::new();
        for child in content_node.children().filter(|c| c.is_element() && !is_foreign(c)) {
            self.walk(child, &inh, &mut content, true);
        }
        self.anim_off -= 1;
        self.pattern_stack.pop();
        if content.is_empty() {
            return None;
        }

        let vb = chain.iter().find_map(|n| view_box(n));
        let content_t = if vb.is_some() {
            viewbox_transform(vb.as_deref(), attr("preserveAspectRatio").unwrap_or(""), tw, th)
        } else if attr("patternContentUnits").is_some_and(|u| u.trim() == "objectBoundingBox") {
            Some(Transform::Scale { x: shape.w, y: shape.h })
        } else {
            None
        };
        let probe = Element::Group(Group { children: content.clone(), id: None, transform: content_t.clone(), style: None });
        let reach = elements_bbox(std::slice::from_ref(&probe))?;
        let eps = 1e-6;
        let overflows = reach.x < -eps || reach.y < -eps || reach.x + reach.w > tw + eps || reach.y + reach.h > th + eps;

        // The tiles that meet the shape, counted in the space of the pattern.
        let corners = [(shape.x, shape.y), (shape.x + shape.w, shape.y), (shape.x, shape.y + shape.h), (shape.x + shape.w, shape.y + shape.h)];
        let mapped: Vec<(f64, f64)> = corners.iter().map(|(x, y)| geom::apply(inv, *x, *y)).collect();
        let (min_x, max_x) = mapped.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
        let (min_y, max_y) = mapped.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
        // A tile that only touches the bounds is not needed.
        let slack = 1e-9;
        let (i0, i1) = (((min_x - tx) / tw + slack).floor(), ((max_x - tx) / tw - slack).ceil() - 1.0);
        let (j0, j1) = (((min_y - ty) / th + slack).floor(), ((max_y - ty) / th - slack).ceil() - 1.0);
        let count = (i1 - i0 + 1.0) * (j1 - j0 + 1.0);
        if count.is_nan() || count < 1.0 {
            return None;
        }
        let limit = if overflows { MAX_CLIPPED_TILES } else { MAX_TILES };
        if !count.is_finite() || count > limit as f64 {
            self.warn(format!("pattern #{id} needs {count} tiles for this shape, more than the {limit} allowed; the fill is left out"));
            return None;
        }
        if self.emitted + (count as usize) * content.len() > crate::MAX_ELEMENTS {
            self.warn(format!("pattern #{id} would pass the limit of {} shapes; the fill is left out", crate::MAX_ELEMENTS));
            return None;
        }

        let mut tiles = Vec::with_capacity(count as usize);
        for j in (j0 as i64)..=(j1 as i64) {
            for i in (i0 as i64)..=(i1 as i64) {
                let origin = translate(tx + i as f64 * tw, ty + j as f64 * th);
                if overflows {
                    let mut layer = Layer::new(vec![Element::Group(Group { children: content.clone(), id: None, transform: content_t.clone(), style: None })]);
                    layer.clip_path = Some(vec![plain_rect(0.0, 0.0, tw, th)]);
                    layer.transform = compose(vec![pt.clone(), origin]);
                    tiles.push(Element::Layer(layer));
                } else {
                    tiles.push(Element::Group(Group { children: content.clone(), id: None, transform: compose(vec![pt.clone(), origin, content_t.clone()]), style: None }));
                }
            }
        }
        Some(tiles)
    }
}
