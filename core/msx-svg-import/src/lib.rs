// core/msx-svg-import/src/lib.rs
//! SVG importer: reads an `.svg` document into an MSX [`Scene`].
//!
//! The importer resolves everything the MSX renderers do not do themselves,
//! so the result renders the same on the SVG, CPU and GPU backends:
//!
//! - The CSS cascade (presentation attributes, `<style>` rules, `style`
//!   attributes) and inheritance are resolved, and every shape carries
//!   explicit paint values.
//! - `fill-opacity`, `stroke-opacity` and `currentColor` are folded into
//!   plain colors and gradient stop alphas.
//! - `<use>` is expanded in place, with the style of the `<use>` element.
//! - `width`, `height`, `viewBox` and `preserveAspectRatio` are baked into
//!   the canvas size and one wrapper transform.
//! - Gradients are converted to the bounding-box space MSX uses.
//! - `<g opacity>` becomes an isolated layer.
//!
//! What it cannot express is reported in [`Imported::warnings`] rather than
//! dropped silently: clipping, masks, filters, patterns, images, markers,
//! text on a path, spread methods other than pad, and a few gradient
//! transforms.

mod anim;
mod animate;
mod geom;
mod markers;
mod gradient;
mod parse;
mod raster;
mod style;

use std::collections::{HashMap, HashSet};
use std::fmt;

use msx_ast::path::parse_d_partial;
use msx_ast::{
    BlendMode, Canvas, Circle, Color, Element, Ellipse, Group, Layer, Line, MaskType, Paint, Path, PathCommand, Point, Polyline, Rect, Scene, Style, Text, Transform,
};
use roxmltree::{Document, Node, ParsingOptions};

use geom::{path_bbox, reaches_outside, BBox};
use parse::{number_list, parse_css_transform, parse_length, parse_origin, parse_stylesheet, parse_transform, Decl, Stylesheet, Unit};
use style::{apply, diagonal, element_opacity, with_alpha, Inh, PaintSpec, Props, PRESENTATION};

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const MAX_USE_DEPTH: usize = 24;
/// Stops an exponentially nested `use` chain from running away.
const MAX_ELEMENTS: usize = 200_000;
/// Children `<switch>` may choose between.
const SWITCHABLE: &[&str] = &["g", "a", "svg", "use", "rect", "circle", "ellipse", "line", "polyline", "polygon", "path", "text", "image", "foreignObject", "switch"];

/// The result of an import.
#[derive(Debug)]
pub struct Imported {
    pub scene: Scene,
    /// Things in the SVG that could not be imported faithfully.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    /// The text is not well-formed XML.
    Xml(String),
    /// The document is XML but not an SVG.
    NotSvg(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Xml(e) => write!(f, "not valid XML: {e}"),
            ImportError::NotSvg(e) => write!(f, "not an SVG document: {e}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Settings for an import.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Directory that relative `<image>` file references are read from. With it
    /// the image bytes are embedded in the scene, so the result stands alone.
    /// Without it a file reference is kept as a path and reported.
    pub base_dir: Option<std::path::PathBuf>,
}

/// Import SVG source text.
pub fn import_svg(source: &str) -> Result<Imported, ImportError> {
    import_svg_with(source, &ImportOptions::default())
}

/// Import SVG source text with `options`.
pub fn import_svg_with(source: &str, options: &ImportOptions) -> Result<Imported, ImportError> {
    // A byte order mark or blank lines before `<?xml` make an XML parser
    // reject the file, and real SVG exports have both.
    let source = source.trim_start_matches('\u{feff}').trim_start();
    let opts = ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = Document::parse_with_options(source, opts).map_err(|e| ImportError::Xml(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(ImportError::NotSvg(format!("the root element is <{}>", root.tag_name().name())));
    }
    let mut imp = Importer::new(&doc);
    imp.base_dir = options.base_dir.clone();
    Ok(imp.run(root))
}

pub(crate) struct Importer<'a, 'i> {
    pub(crate) ids: HashMap<&'a str, Node<'a, 'i>>,
    sheet: Stylesheet,
    pub(crate) warnings: Vec<String>,
    seen: HashSet<String>,
    pub(crate) defs: Vec<msx_ast::Def>,
    pub(crate) grad_cache: HashMap<String, Option<Paint>>,
    pub(crate) used_ids: HashSet<String>,
    pub(crate) next_id: usize,
    /// Viewport for `%` lengths.
    pub(crate) vp: (f64, f64),
    depth: usize,
    /// Ids of the `use` targets being expanded right now.
    use_stack: Vec<String>,
    /// Shapes and text written so far.
    emitted: usize,
    /// True while the children of a `clipPath` are walked: only geometry and
    /// `clip-rule` matter, so paint is not resolved and effects are ignored.
    clip_mode: bool,
    /// Markers already built, by id. `None` is a marker that cannot be used.
    marker_cache: HashMap<String, Option<std::rc::Rc<MarkerDef>>>,
    /// Ids of the markers being built right now, to stop cycles.
    marker_stack: Vec<String>,
    /// Where relative `<image>` files are read from, if anywhere.
    base_dir: Option<std::path::PathBuf>,
    /// True when some `clipPath` or `mask` works in `objectBoundingBox` units: a
    /// hidden shape still counts toward the box, so it is kept as an invisible
    /// element.
    keep_hidden: bool,
    /// Ids of the clip paths being resolved right now, to stop cycles.
    clip_stack: Vec<String>,
    /// Animation elements that name their target with `href`, by target.
    anim_by_href: HashMap<roxmltree::NodeId, Vec<Node<'a, 'i>>>,
    /// Animation wrappers written so far, waiting for the scene timeline.
    pending: Vec<animate::Pending>,
    /// Counter for the ids of animation wrappers.
    next_anim: usize,
    /// The element whose own transform the walk leaves off, because an
    /// animation wrapper carries it instead.
    skip_transform: Option<roxmltree::NodeId>,
    /// The element whose own opacity the walk leaves off, because an opacity
    /// animation replaces it.
    skip_opacity: Option<roxmltree::NodeId>,
    /// Above zero while mask or marker content is walked: the resolver does
    /// not reach animations there.
    anim_off: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Shape,
    Line,
    Text,
}

enum Shape {
    Rect { x: f64, y: f64, w: f64, h: f64, rx: Option<f64>, ry: Option<f64> },
    Circle { cx: f64, cy: f64, r: f64 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    Poly { points: Vec<Point>, closed: bool },
    Path { commands: Vec<PathCommand> },
}

/// One run of text that shares a position and a style.
struct Run {
    x: f64,
    y: f64,
    text: String,
    inh: Inh,
}

/// How an element composites against what is already drawn.
#[derive(Clone)]
struct Comp {
    opacity: f64,
    blend: BlendMode,
    isolate: bool,
    /// Clip paths that all apply, in the element's own coordinate space. The
    /// first is the outermost layer, so the visible area is their intersection.
    clips: Vec<Vec<Element>>,
    /// A mask that applies, as its elements and how it is read.
    mask: Option<(Vec<Element>, MaskType)>,
}

impl Comp {
    const NONE: Comp = Comp { opacity: 1.0, blend: BlendMode::Normal, isolate: false, clips: Vec::new(), mask: None };

    fn needs_layer(&self) -> bool {
        self.opacity < 1.0 || self.blend != BlendMode::Normal || self.isolate || !self.clips.is_empty() || self.mask.is_some()
    }
}

/// A marker ready to be placed at the vertices of a shape.
#[derive(Debug)]
struct MarkerDef {
    /// The content, drawn in the marker's own coordinates.
    kids: Vec<Element>,
    /// The marker viewport.
    width: f64,
    height: f64,
    /// Maps the content into the viewport (`viewBox` and `preserveAspectRatio`).
    view: Option<Transform>,
    /// The reference point in viewport coordinates, which lands on the vertex.
    ref_x: f64,
    ref_y: f64,
    /// `markerUnits="strokeWidth"`: scale by the stroke width of the shape.
    scale_by_stroke: bool,
    orient: markers::Orient,
    /// The content reaches past the viewport, which clips it.
    clip: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum MarkerKind {
    Start,
    Mid,
    End,
}

/// The transform slot of `el`, for the elements that have one.
fn transform_mut(el: &mut Element) -> Option<&mut Option<Transform>> {
    match el {
        Element::Rect(e) => Some(&mut e.transform),
        Element::Circle(e) => Some(&mut e.transform),
        Element::Ellipse(e) => Some(&mut e.transform),
        Element::Line(e) => Some(&mut e.transform),
        Element::Polyline(e) | Element::Polygon(e) => Some(&mut e.transform),
        Element::Path(e) => Some(&mut e.transform),
        Element::Text(e) => Some(&mut e.transform),
        Element::Image(e) => Some(&mut e.transform),
        _ => None,
    }
}

/// `url(#id)` with optional quotes and a trailing fallback; anything else is `None`.
fn parse_url_id(v: &str) -> Option<String> {
    let v = v.trim();
    if !v.get(..4)?.eq_ignore_ascii_case("url(") {
        return None;
    }
    let inner = v[4..].split(')').next()?.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    inner.strip_prefix('#').filter(|id| !id.is_empty()).map(String::from)
}

/// The `w` by `h` viewport as `(x0, y0, x1, y1)` in the space the content is
/// drawn in, which a `viewBox` moves and scales away from `(0, 0, w, h)`.
fn viewport_bounds(view: &Option<Transform>, w: f64, h: f64) -> (f64, f64, f64, f64) {
    match view.as_ref().and_then(|t| geom::invert(t.to_matrix())) {
        Some(inv) => {
            let (ax, ay) = geom::apply(inv, 0.0, 0.0);
            let (bx, by) = geom::apply(inv, w, h);
            (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
        }
        None => (0.0, 0.0, w, h),
    }
}

/// One copy of `def` at `vertex`: turned by `orient`, scaled by the stroke
/// width when the marker works in stroke units, and moved so that the
/// reference point lands on the vertex. A marker that works out to nothing
/// (empty, or a zero stroke width) gives no element.
fn marker_copy(def: &MarkerDef, vertex: &markers::Vertex, kind: MarkerKind, stroke_width: f64) -> Option<Element> {
    if def.kids.is_empty() {
        return None;
    }
    let scale = if def.scale_by_stroke { stroke_width } else { 1.0 };
    if scale <= 0.0 {
        return None;
    }
    let radians = match def.orient {
        markers::Orient::Auto => vertex.angle,
        markers::Orient::AutoStartReverse if kind == MarkerKind::Start => vertex.angle + std::f64::consts::PI,
        markers::Orient::AutoStartReverse => vertex.angle,
        markers::Orient::Angle(a) => a,
    };
    let degrees = radians.to_degrees();
    let place = compose(vec![
        translate(vertex.x, vertex.y),
        (degrees != 0.0).then_some(Transform::Rotate { angle: degrees, cx: None, cy: None }),
        (scale != 1.0).then_some(Transform::Scale { x: scale, y: scale }),
        translate(-def.ref_x, -def.ref_y),
    ]);
    let content = def.kids.clone();
    if def.clip {
        let inner = match &def.view {
            Some(v) => vec![Element::Group(Group { children: content, id: None, transform: Some(v.clone()), style: None })],
            None => content,
        };
        let mut layer = Layer::new(inner).with_clip_path(vec![plain_rect(0.0, 0.0, def.width, def.height)]);
        layer.transform = place;
        Some(Element::Layer(layer))
    } else {
        Some(Element::Group(Group { children: content, id: None, transform: compose(vec![place, def.view.clone()]), style: None }))
    }
}

/// Whether an `overflow` value switches clipping off: only `visible` and `auto` do.
fn overflow_is_open(v: &str) -> bool {
    matches!(v.trim(), "visible" | "auto")
}

/// Bounding box of the geometry of freshly built `kids`, for
/// `objectBoundingBox` units.
fn elements_bbox(kids: &[Element]) -> Option<BBox> {
    geom::elements_bbox(kids, msx_ast::Matrix2D::identity())
}

/// The visible area of a `w` by `h` viewport as a rectangle in the space that
/// `view` maps content into, found by mapping the viewport back through `view`.
fn viewport_rect(view: &Option<Transform>, w: f64, h: f64) -> Element {
    let (x0, y0, x1, y1) = viewport_bounds(view, w, h);
    plain_rect(x0, y0, x1, y1)
}

/// A black rectangle from `(x0, y0)` to `(x1, y1)`, for clip shapes.
fn plain_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Element {
    let style = Style { fill: Some(Paint::Color(Color::BLACK)), stroke: Some(Paint::None), ..Style::default() };
    Element::Rect(Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0, rx: None, ry: None, id: None, transform: None, style })
}

/// A number or a percentage as a fraction, for `objectBoundingBox` units.
fn obb_fraction(v: &str) -> Option<f64> {
    let v = v.trim();
    match v.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok().map(|n| n / 100.0),
        None => v.parse::<f64>().ok(),
    }
}

/// Layers for one clipped element, outermost first: the first clip, opacity,
/// blend mode and transform go on the outer layer, each further clip on its own
/// layer inside it.
fn layered(kids: Vec<Element>, opacity: f64, blend: BlendMode, mut clips: Vec<Vec<Element>>, mask: Option<(Vec<Element>, MaskType)>, transform: Option<Transform>, id: Option<String>) -> Element {
    let outer_clip = (!clips.is_empty()).then(|| clips.remove(0));
    let mut inner = kids;
    for shapes in clips.into_iter().rev() {
        inner = vec![Element::Layer(Layer::new(inner).with_clip_path(shapes))];
    }
    let mut layer = Layer::new(inner);
    layer.clip_path = outer_clip;
    if let Some((elements, kind)) = mask {
        layer.mask = Some(elements);
        layer.mask_type = kind;
    }
    layer.opacity = opacity;
    layer.blend_mode = blend;
    layer.transform = transform;
    layer.id = id;
    Element::Layer(layer)
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

fn id_mut(el: &mut Element) -> Option<&mut Option<String>> {
    match el {
        Element::Rect(e) => Some(&mut e.id),
        Element::Circle(e) => Some(&mut e.id),
        Element::Ellipse(e) => Some(&mut e.id),
        Element::Line(e) => Some(&mut e.id),
        Element::Polyline(e) | Element::Polygon(e) => Some(&mut e.id),
        Element::Path(e) => Some(&mut e.id),
        Element::Text(e) => Some(&mut e.id),
        _ => None,
    }
}

/// `paint-order: stroke` as two elements: the stroke first, then the fill.
/// MSX always draws the fill and then the stroke of one element.
fn split_paint_order(el: Element) -> Vec<Element> {
    let mut el = el;
    let (has_fill, has_stroke) = match style_mut(&mut el) {
        Some(s) => (matches!(&s.fill, Some(p) if *p != Paint::None), matches!(&s.stroke, Some(p) if *p != Paint::None) && s.stroke_width.unwrap_or(1.0) > 0.0),
        None => (false, false),
    };
    if !(has_fill && has_stroke) {
        return vec![el];
    }
    let mut stroke_only = el.clone();
    if let Some(s) = style_mut(&mut stroke_only) {
        s.fill = Some(Paint::None);
    }
    let mut fill_only = el;
    if let Some(s) = style_mut(&mut fill_only) {
        s.stroke = Some(Paint::None);
    }
    if let Some(id) = id_mut(&mut fill_only) {
        *id = None;
    }
    vec![stroke_only, fill_only]
}

fn translate(x: f64, y: f64) -> Option<Transform> {
    (x != 0.0 || y != 0.0).then_some(Transform::Translate { x, y })
}

/// Chain transforms in SVG order (first is outermost).
fn compose(parts: Vec<Option<Transform>>) -> Option<Transform> {
    let mut flat = Vec::new();
    for t in parts.into_iter().flatten() {
        match t {
            Transform::Multiple(v) => flat.extend(v),
            Transform::None => {}
            other => flat.push(other),
        }
    }
    match flat.len() {
        0 => None,
        1 => flat.pop(),
        _ => Some(Transform::Multiple(flat)),
    }
}

fn view_box(node: &Node) -> Option<Vec<f64>> {
    node.attribute("viewBox").map(number_list).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0)
}

/// The transform that maps a `viewBox` onto a `w` by `h` viewport.
fn viewbox_transform(vb: Option<&[f64]>, par: &str, w: f64, h: f64) -> Option<Transform> {
    let v = vb?;
    let (vx, vy, vw, vh) = (v[0], v[1], v[2], v[3]);
    let mut words = par.split_whitespace();
    let mut align = words.next().unwrap_or("xMidYMid");
    if align == "defer" {
        align = words.next().unwrap_or("xMidYMid");
    }
    let slice = words.next() == Some("slice");
    let (sx, sy) = (w / vw, h / vh);
    if align == "none" {
        return compose(vec![translate(-vx * sx, -vy * sy), Some(Transform::Scale { x: sx, y: sy }).filter(|_| sx != 1.0 || sy != 1.0)]);
    }
    let s = if slice { sx.max(sy) } else { sx.min(sy) };
    let frac = |part: &str, min: &str, max: &str| {
        if part == min {
            0.0
        } else if part == max {
            1.0
        } else {
            0.5
        }
    };
    let (fx, fy) = if align.len() == 8 { (frac(&align[..4], "xMin", "xMax"), frac(&align[4..], "YMin", "YMax")) } else { (0.5, 0.5) };
    let tx = -vx * s + fx * (w - vw * s);
    let ty = -vy * s + fy * (h - vh * s);
    compose(vec![translate(tx, ty), Some(Transform::Scale { x: s, y: s }).filter(|_| s != 1.0)])
}

/// Path data from commands, at full precision, with explicit separators.
fn path_data(cmds: &[PathCommand]) -> String {
    let n = |v: f64| if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") };
    let p = |pt: &Point| format!("{} {}", n(pt.x), n(pt.y));
    cmds.iter()
        .map(|c| match c {
            PathCommand::MoveTo(a) => format!("M {}", p(a)),
            PathCommand::LineTo(a) => format!("L {}", p(a)),
            PathCommand::HLineTo(x) => format!("H {}", n(*x)),
            PathCommand::VLineTo(y) => format!("V {}", n(*y)),
            PathCommand::CubicBezier { c1, c2, to } => format!("C {} {} {}", p(c1), p(c2), p(to)),
            PathCommand::SmoothCubic { c2, to } => format!("S {} {}", p(c2), p(to)),
            PathCommand::QuadBezier { c, to } => format!("Q {} {}", p(c), p(to)),
            PathCommand::SmoothQuad { to } => format!("T {}", p(to)),
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } => {
                format!("A {} {} {} {} {} {}", n(*rx), n(*ry), n(*x_rotation), *large_arc as u8, *sweep as u8, p(to))
            }
            PathCommand::RelMoveTo(a) => format!("m {}", p(a)),
            PathCommand::RelLineTo(a) => format!("l {}", p(a)),
            PathCommand::RelHLineTo(x) => format!("h {}", n(*x)),
            PathCommand::RelVLineTo(y) => format!("v {}", n(*y)),
            PathCommand::RelCubicBezier { c1, c2, to } => format!("c {} {} {}", p(c1), p(c2), p(to)),
            PathCommand::RelSmoothCubic { c2, to } => format!("s {} {}", p(c2), p(to)),
            PathCommand::RelQuadBezier { c, to } => format!("q {} {}", p(c), p(to)),
            PathCommand::RelSmoothQuad { to } => format!("t {}", p(to)),
            PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
                format!("a {} {} {} {} {} {}", n(*rx), n(*ry), n(*x_rotation), *large_arc as u8, *sweep as u8, p(to))
            }
            PathCommand::ClosePath => "Z".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_foreign(node: &Node) -> bool {
    node.tag_name().namespace().is_some_and(|ns| ns != SVG_NS)
}

impl<'a, 'i> Importer<'a, 'i> {
    fn new(doc: &'a Document<'i>) -> Self {
        let mut ids = HashMap::new();
        let mut sheet = Stylesheet::default();
        let mut keyframes = false;
        let mut keep_hidden = false;
        for n in doc.descendants().filter(|n| n.is_element()) {
            if let Some(id) = n.attribute("id") {
                ids.entry(id).or_insert(n);
            }
            keep_hidden |= (n.tag_name().name() == "clipPath" && n.attribute("clipPathUnits").is_some_and(|u| u.trim() == "objectBoundingBox"))
                || (n.tag_name().name() == "mask" && n.attribute("maskUnits").is_none_or(|u| u.trim() != "userSpaceOnUse"));
            if n.tag_name().name() == "style" && n.attribute("type").is_none_or(|t| t.is_empty() || t == "text/css") {
                let css: String = n.descendants().filter(|c| c.is_text()).filter_map(|c| c.text()).collect();
                keyframes |= css.contains("@keyframes");
                parse_stylesheet(&css, &mut sheet);
            }
        }
        let anim_by_href = Self::index_animations(doc, &ids);
        let mut imp = Importer {
            ids,
            sheet,
            warnings: Vec::new(),
            seen: HashSet::new(),
            defs: Vec::new(),
            grad_cache: HashMap::new(),
            used_ids: HashSet::new(),
            next_id: 0,
            vp: (300.0, 150.0),
            depth: 0,
            use_stack: Vec::new(),
            clip_mode: false,
            marker_cache: HashMap::new(),
            marker_stack: Vec::new(),
            base_dir: None,
            keep_hidden,
            clip_stack: Vec::new(),
            emitted: 0,
            anim_by_href,
            pending: Vec::new(),
            next_anim: 0,
            skip_transform: None,
            skip_opacity: None,
            anim_off: 0,
        };
        if keyframes {
            imp.warn("CSS @keyframes animations are not supported; the static frame is drawn".to_string());
        }
        let skipped = std::mem::take(&mut imp.sheet.unsupported);
        for s in skipped {
            imp.warn(format!("CSS selector \"{s}\" is not supported and its rule was skipped"));
        }
        imp
    }

    pub(crate) fn warn(&mut self, msg: String) {
        if self.seen.insert(msg.clone()) {
            self.warnings.push(msg);
        }
    }

    /// Specified properties of an element: presentation attributes, then
    /// matching stylesheet rules by specificity and order, then the
    /// `style` attribute, then `!important` declarations.
    pub(crate) fn props_for(&self, node: Node) -> Props {
        let mut props = Props::new();
        for attr in node.attributes() {
            if attr.namespace().is_none() && PRESENTATION.contains(&attr.name()) {
                props.insert(attr.name().to_string(), attr.value().trim().to_string());
            }
        }

        let tag = node.tag_name().name();
        let id = node.attribute("id");
        let classes: Vec<&str> = node.attribute("class").map(|c| c.split_whitespace().collect()).unwrap_or_default();
        let mut matched: Vec<_> = self.sheet.rules.iter().filter(|(sel, ..)| sel.matches(tag, id, &classes)).collect();
        matched.sort_by_key(|(sel, _, order)| (sel.specificity(), *order));

        let inline: Vec<Decl> = node.attribute("style").map(parse::parse_declarations).unwrap_or_default();
        let ordered: Vec<&Decl> = matched.iter().flat_map(|(_, decls, _)| decls.iter()).chain(inline.iter()).collect();
        for d in ordered.iter().filter(|d| !d.important) {
            props.insert(d.name.clone(), d.value.clone());
        }
        for d in ordered.iter().filter(|d| d.important) {
            props.insert(d.name.clone(), d.value.clone());
        }
        props
    }

    fn label(node: &Node) -> String {
        match node.attribute("id") {
            Some(id) => format!("<{} id=\"{id}\">", node.tag_name().name()),
            None => format!("<{}>", node.tag_name().name()),
        }
    }

    fn check_unsupported(&mut self, node: &Node, props: &Props) {
        if self.clip_mode {
            return;
        }
        for key in ["animation", "animation-name"] {
            if props.get(key).is_some_and(|v| !v.trim().is_empty() && v.trim() != "none") {
                self.warn(format!("{} uses a CSS {key}, which is not supported; the static frame is drawn", Self::label(node)));
            }
        }
        for key in ["filter"] {
            if let Some(v) = props.get(key) {
                if !v.is_empty() && v != "none" {
                    self.warn(format!("{} uses {key}, which is not supported; drawn without it", Self::label(node)));
                }
            }
        }
        for key in ["fill", "stroke"] {
            if let Some(v) = props.get(key) {
                if v.trim().starts_with("context-") {
                    self.warn(format!("{} uses {key}: {}, which is not supported; painted as none", Self::label(node), v.trim()));
                }
            }
        }
        // Text and stroke properties the renderers have no slot for. Each is
        // reported only when it differs from the initial value.
        let ignored: [(&str, &[&str]); 6] = [
            ("text-decoration", &["none", ""]),
            ("letter-spacing", &["normal", "0", "0px", ""]),
            ("word-spacing", &["normal", "0", "0px", ""]),
            ("font-style", &["normal", ""]),
            ("writing-mode", &["horizontal-tb", "lr", "lr-tb", ""]),
            ("vector-effect", &["none", ""]),
        ];
        for (key, initial) in ignored {
            if let Some(v) = props.get(key) {
                if !initial.contains(&v.trim()) {
                    self.warn(format!("{} uses {key}: {}, which is not supported; ignored", Self::label(node), v.trim()));
                }
            }
        }
        if let Some(b) = props.get("transform-box") {
            if !matches!(b.trim(), "view-box" | "") {
                self.warn(format!("{} uses transform-box: {}, which is not supported; view-box is used", Self::label(node), b.trim()));
            }
        }
    }

    /// `requiredExtensions` and `systemLanguage`. The language is English.
    fn passes_conditions(node: &Node) -> bool {
        if node.has_attribute("requiredExtensions") {
            return false;
        }
        match node.attribute("systemLanguage") {
            Some(langs) => langs.split(',').map(str::trim).any(|l| l == "en" || l.starts_with("en-")),
            None => true,
        }
    }

    /// Opacity, blend mode and isolation of one element.
    fn composite(&mut self, node: &Node, props: &Props, bbox: Option<BBox>) -> Comp {
        if self.clip_mode {
            if props.get("clip-path").is_some_and(|v| !v.trim().is_empty() && v.trim() != "none") {
                self.warn(format!("{} uses clip-path inside a clip path, which is not supported; ignored", Self::label(node)));
            }
            return Comp::NONE;
        }
        let blend = match props.get("mix-blend-mode").map(|v| v.trim()) {
            None | Some("normal") | Some("") => BlendMode::Normal,
            Some("multiply") => BlendMode::Multiply,
            Some("screen") => BlendMode::Screen,
            Some("overlay") => BlendMode::Overlay,
            Some("darken") => BlendMode::Darken,
            Some("lighten") => BlendMode::Lighten,
            Some("hard-light") => BlendMode::HardLight,
            Some("soft-light") => BlendMode::SoftLight,
            Some("difference") => BlendMode::Difference,
            Some("exclusion") => BlendMode::Exclusion,
            Some(other) => {
                self.warn(format!("{} uses mix-blend-mode: {other}, which is not supported; drawn normally", Self::label(node)));
                BlendMode::Normal
            }
        };
        let clips = self.resolve_clips(node, props, bbox);
        let mask = self.resolve_mask(node, props, bbox);
        let opacity = if self.skip_opacity == Some(node.id()) { 1.0 } else { element_opacity(props) };
        Comp { opacity, blend, isolate: props.get("isolation").is_some_and(|v| v.trim() == "isolate"), clips, mask }
    }

    // ── Masks ─────────────────────────────────────────────────────────────────

    /// The mask that applies to `node`, from its `mask` property. An unusable
    /// reference is reported and the element is drawn without it.
    fn resolve_mask(&mut self, node: &Node, props: &Props, bbox: Option<BBox>) -> Option<(Vec<Element>, MaskType)> {
        let v = props.get("mask")?.trim().to_string();
        if v.is_empty() || v == "none" {
            return None;
        }
        let label = Self::label(node);
        match parse_url_id(&v) {
            Some(id) => self.mask_chain(&label, &id, bbox),
            None => {
                self.warn(format!("{label} uses mask: {v}, which is not supported (only url(#id)); drawn without it"));
                None
            }
        }
    }

    fn mask_chain(&mut self, label: &str, id: &str, bbox: Option<BBox>) -> Option<(Vec<Element>, MaskType)> {
        let key = format!("mask:{id}");
        if self.clip_stack.contains(&key) {
            self.warn(format!("{label} refers to the mask #{id}, which refers back to itself; drawn without it"));
            return None;
        }
        let Some(m) = self.ids.get(id).copied() else {
            self.warn(format!("{label} uses the mask #{id}, which does not exist; drawn without it"));
            return None;
        };
        if m.tag_name().name() != "mask" {
            self.warn(format!("{label} uses #{id} as a mask, but it is a <{}>; drawn without it", m.tag_name().name()));
            return None;
        }
        self.clip_stack.push(key);
        let out = self.mask_content(label, m, bbox);
        self.clip_stack.pop();
        Some(out)
    }

    /// The elements of one `mask`, in the coordinate space of the masked
    /// element, and how they are read. A region that cannot be worked out, or
    /// is empty, leaves a mask with nothing in it, which hides the element.
    /// The region becomes a clip of the content, and a `mask` set on the
    /// `mask` itself becomes a mask of the content.
    fn mask_content(&mut self, label: &str, m: Node<'a, 'i>, bbox: Option<BBox>) -> (Vec<Element>, MaskType) {
        let mprops = self.props_for(m);
        let kind = MaskType::from_name(mprops.get("mask-type").map(String::as_str).unwrap_or("luminance"));
        if mprops.get("color-interpolation").is_some_and(|v| v.trim() == "linearRGB") {
            self.warn(format!("{label} uses a mask with color-interpolation: linearRGB, which is not supported; read as sRGB"));
        }
        let hidden = (Vec::new(), kind);
        let bbox = bbox.filter(|b| b.w > 0.0 && b.h > 0.0);
        let region = if m.attribute("maskUnits").is_some_and(|u| u.trim() == "userSpaceOnUse") {
            let (vw, vh) = self.vp;
            let get = |this: &Self, name: &str, base: f64, default: f64| this.length(m, name, 16.0, base).unwrap_or(default);
            (get(self, "x", vw, -0.1 * vw), get(self, "y", vh, -0.1 * vh), get(self, "width", vw, 1.2 * vw), get(self, "height", vh, 1.2 * vh))
        } else {
            let Some(b) = bbox else { return hidden };
            let f = |name: &str, default: f64| m.attribute(name).and_then(obb_fraction).unwrap_or(default);
            (b.x + f("x", -0.1) * b.w, b.y + f("y", -0.1) * b.h, f("width", 1.2) * b.w, f("height", 1.2) * b.h)
        };
        if region.2 <= 0.0 || region.3 <= 0.0 {
            return hidden;
        }
        let inh = self.inh_chain(m);
        let mut content = Vec::new();
        self.anim_off += 1;
        for child in m.children().filter(|c| c.is_element() && !is_foreign(c)) {
            self.walk(child, &inh, &mut content, true);
        }
        self.anim_off -= 1;
        if m.attribute("maskContentUnits").is_some_and(|u| u.trim() == "objectBoundingBox") {
            let Some(b) = bbox else { return hidden };
            let t = Transform::Multiple(vec![Transform::Translate { x: b.x, y: b.y }, Transform::Scale { x: b.w, y: b.h }]);
            if !content.is_empty() {
                content = vec![Element::Group(Group { children: content, id: None, transform: Some(t), style: None })];
            }
        }
        let bounds = (region.0, region.1, region.0 + region.2, region.1 + region.3);
        if reaches_outside(&content, bounds) {
            content = vec![Element::Layer(Layer::new(content).with_clip_path(vec![plain_rect(bounds.0, bounds.1, bounds.2, bounds.3)]))];
        }
        if let Some(v) = mprops.get("mask").map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && v != "none") {
            match parse_url_id(&v) {
                Some(nested) => {
                    if let Some((elements, nested_kind)) = self.mask_chain(label, &nested, bbox) {
                        content = vec![Element::Layer(Layer::new(content).with_mask(elements, nested_kind))];
                    }
                }
                None => self.warn(format!("<mask> uses mask: {v}, which is not supported (only url(#id)); ignored")),
            }
        }
        (content, kind)
    }

    // ── Clip paths ────────────────────────────────────────────────────────────

    /// The clip paths that apply to `node`, from its `clip-path` property. An
    /// unusable reference is reported and the element is drawn unclipped.
    fn resolve_clips(&mut self, node: &Node, props: &Props, bbox: Option<BBox>) -> Vec<Vec<Element>> {
        let Some(v) = props.get("clip-path").map(|v| v.trim().to_string()) else { return Vec::new() };
        if v.is_empty() || v == "none" {
            return Vec::new();
        }
        let label = Self::label(node);
        let mut out = Vec::new();
        match parse_url_id(&v) {
            Some(id) => self.clip_chain(&label, &id, bbox, &mut out),
            None => self.warn(format!("{label} uses clip-path: {v}, which is not supported (only url(#id)); drawn without it")),
        }
        out
    }

    /// Resolves one `clipPath`, then the `clip-path` set on that `clipPath`
    /// itself, which narrows it further.
    fn clip_chain(&mut self, label: &str, id: &str, bbox: Option<BBox>, out: &mut Vec<Vec<Element>>) {
        if self.clip_stack.iter().any(|s| s == id) {
            self.warn(format!("{label} refers to the clip path #{id}, which refers back to itself; drawn without it"));
            return;
        }
        let Some(cp) = self.ids.get(id).copied() else {
            self.warn(format!("{label} uses the clip path #{id}, which does not exist; drawn without it"));
            return;
        };
        if cp.tag_name().name() != "clipPath" {
            self.warn(format!("{label} uses #{id} as a clip path, but it is a <{}>; drawn without it", cp.tag_name().name()));
            return;
        }
        self.clip_stack.push(id.to_string());
        let shapes = self.clip_shapes(cp, bbox);
        out.push(shapes);
        let nested = self.props_for(cp).get("clip-path").map(|v| v.trim().to_string());
        if let Some(v) = nested.filter(|v| !v.is_empty() && v != "none") {
            match parse_url_id(&v) {
                Some(next) => self.clip_chain(label, &next, bbox, out),
                None => self.warn(format!("<clipPath id=\"{id}\"> uses clip-path: {v}, which is not supported (only url(#id)); ignored")),
            }
        }
        self.clip_stack.pop();
    }

    /// The shapes of one `clipPath` in the coordinate space of the clipped
    /// element. Only shapes, text and `use` of a shape count; a `line` has no
    /// area. `clipPathUnits="objectBoundingBox"` maps the unit square onto
    /// `bbox`, and an empty box leaves nothing visible.
    fn clip_shapes(&mut self, cp: Node<'a, 'i>, bbox: Option<BBox>) -> Vec<Element> {
        let mut parts = vec![self.node_transform(cp)];
        if cp.attribute("clipPathUnits").is_some_and(|u| u.trim() == "objectBoundingBox") {
            match bbox.filter(|b| b.w > 0.0 && b.h > 0.0) {
                Some(b) => parts.push(Some(Transform::Multiple(vec![Transform::Translate { x: b.x, y: b.y }, Transform::Scale { x: b.w, y: b.h }]))),
                None => return Vec::new(),
            }
        }
        let inh = self.inh_chain(cp);
        let saved = std::mem::replace(&mut self.clip_mode, true);
        let mut kids = Vec::new();
        for child in cp.children().filter(|c| c.is_element() && !is_foreign(c)) {
            if self.is_clip_child(child) {
                self.walk(child, &inh, &mut kids, true);
            }
        }
        self.clip_mode = saved;
        match compose(parts) {
            Some(t) if !kids.is_empty() => vec![Element::Group(Group { children: kids, id: None, transform: Some(t), style: None })],
            _ => kids,
        }
    }

    /// Children that may contribute to a clip path: shapes with an area, text,
    /// and a `use` that points at one of those.
    fn is_clip_child(&self, child: Node<'a, 'i>) -> bool {
        const SHAPES: [&str; 7] = ["rect", "circle", "ellipse", "polyline", "polygon", "path", "text"];
        match child.tag_name().name() {
            t if SHAPES.contains(&t) => true,
            "use" => {
                let href = child.attribute("href").or_else(|| child.attribute((XLINK_NS, "href")));
                href.map(|h| h.trim().trim_start_matches('#')).and_then(|id| self.ids.get(id)).is_some_and(|t| SHAPES.contains(&t.tag_name().name()))
            }
            _ => false,
        }
    }

    /// Inherited style in effect on `node`, folded down from the root.
    fn inh_chain(&mut self, node: Node<'a, 'i>) -> Inh {
        let mut chain: Vec<Node> = node.ancestors().filter(|n| n.is_element()).collect();
        chain.reverse();
        let mut inh = Inh::default();
        for n in chain {
            let props = self.props_for(n);
            apply(&mut inh, &props, self.vp);
        }
        inh
    }

    /// The `color` property in effect on `node`, from its ancestors.
    pub(crate) fn color_of(&self, node: Node<'a, 'i>) -> Color {
        let mut chain: Vec<Node> = node.ancestors().filter(|n| n.is_element()).collect();
        chain.reverse();
        let mut color = Color::BLACK;
        for n in chain {
            if let Some(c) = self.props_for(n).get("color").and_then(|v| parse::parse_color(v)) {
                color = c;
            }
        }
        color
    }

    /// A non-inherited property, following `inherit` up the tree. `None`
    /// means the initial value.
    pub(crate) fn inherited_prop(&self, node: Node<'a, 'i>, name: &str) -> Option<String> {
        let mut cur = Some(node);
        while let Some(n) = cur {
            let props = self.props_for(n);
            let v = props.get(name)?;
            if !v.trim().eq_ignore_ascii_case("inherit") {
                return Some(v.clone());
            }
            cur = n.parent().filter(|p| p.is_element());
        }
        None
    }

    /// Cascade for one element. `None` when it is `display: none`.
    fn begin(&mut self, node: Node<'a, 'i>, parent: &Inh) -> Option<(Inh, Props)> {
        let props = self.props_for(node);
        if props.get("display").is_some_and(|v| v.trim() == "none") {
            return None;
        }
        let mut inh = parent.clone();
        apply(&mut inh, &props, self.vp);
        self.check_unsupported(&node, &props);
        Some((inh, props))
    }

    /// The element's transform. A CSS `transform` property beats the
    /// attribute, and `transform-origin` moves the point it turns around.
    fn node_transform(&mut self, node: Node) -> Option<Transform> {
        if self.skip_transform == Some(node.id()) {
            return None;
        }
        let props = self.props_for(node);
        let parsed = match props.get("transform") {
            Some(css) => parse_css_transform(css),
            None => parse_transform(node.attribute("transform")?),
        };
        let t = match parsed {
            Ok(t) => t?,
            Err(e) => {
                self.warn(format!("{}: {e}; transform ignored", Self::label(&node)));
                return None;
            }
        };
        let origin = props.get("transform-origin").and_then(|v| parse_origin(v, self.vp.0, self.vp.1));
        match origin {
            Some((ox, oy)) if ox != 0.0 || oy != 0.0 => compose(vec![translate(ox, oy), Some(t), translate(-ox, -oy)]),
            _ => Some(t),
        }
    }

    /// A length attribute in user units.
    fn length(&self, node: Node, name: &str, font_size: f64, percent_of: f64) -> Option<f64> {
        node.attribute(name).and_then(parse_length).map(|l| l.to_px(font_size, percent_of))
    }

    // ── Root ──────────────────────────────────────────────────────────────────

    fn run(&mut self, root: Node<'a, 'i>) -> Imported {
        let vb = view_box(&root);
        let size = |name: &str| {
            root.attribute(name).and_then(parse_length).filter(|l| l.unit != Unit::Percent).map(|l| l.to_px(16.0, 0.0)).filter(|v| *v > 0.0)
        };
        let (w, h) = (size("width"), size("height"));
        let (width, height) = match (w, h, &vb) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some(v)) => (w, w * v[3] / v[2]),
            (None, Some(h), Some(v)) => (h * v[2] / v[3], h),
            (None, None, Some(v)) => (v[2], v[3]),
            (w, h, None) => {
                self.warn("the SVG has no usable viewBox, or width and height; using 300 by 150".to_string());
                (w.unwrap_or(300.0), h.unwrap_or(150.0))
            }
        };
        self.vp = vb.as_ref().map_or((width, height), |v| (v[2], v[3]));

        let mut scene = Scene::new(Canvas::new(width, height, Color::rgba(0, 0, 0, 0)));
        let Some((inh, props)) = self.begin(root, &Inh::default()) else {
            return Imported { scene, warnings: std::mem::take(&mut self.warnings) };
        };

        let mut kids = Vec::new();
        for child in root.children() {
            self.walk(child, &inh, &mut kids, false);
        }
        let view = viewbox_transform(vb.as_deref(), root.attribute("preserveAspectRatio").unwrap_or(""), width, height);
        // The root transform sits outside the viewBox mapping, so a percentage
        // in transform-origin is a share of the viewport, not of the viewBox.
        let user_vp = std::mem::replace(&mut self.vp, (width, height));
        let root_transform = self.node_transform(root);
        self.vp = user_vp;
        let transform = compose(vec![root_transform, view]);
        let mut out = Vec::new();
        let bbox = elements_bbox(&kids);
        let comp = self.composite(&root, &props, bbox);
        self.emit(kids, comp, transform, None, &mut out);

        scene.defs = std::mem::take(&mut self.defs);
        scene.elements = out;
        self.finish_animations(&mut scene);
        Imported { scene, warnings: std::mem::take(&mut self.warnings) }
    }

    /// Wrap `kids` as needed: an isolated layer for group opacity, a group
    /// for a transform or id, nothing at all otherwise.
    fn emit(&mut self, kids: Vec<Element>, comp: Comp, transform: Option<Transform>, id: Option<String>, out: &mut Vec<Element>) {
        if kids.is_empty() {
            return;
        }
        if comp.needs_layer() {
            out.push(layered(kids, comp.opacity, comp.blend, comp.clips, comp.mask, transform, id));
        } else if transform.is_some() || id.is_some() {
            out.push(Element::Group(Group { children: kids, id, transform, style: None }));
        } else {
            out.extend(kids);
        }
    }

    // ── Walk ──────────────────────────────────────────────────────────────────

    fn walk(&mut self, node: Node<'a, 'i>, parent: &Inh, out: &mut Vec<Element>, expanded: bool) {
        if node.is_element() && !is_foreign(&node) && self.has_animation(node) && Self::passes_conditions(&node) {
            self.walk_animated(node, parent, out, expanded);
        } else {
            self.walk_plain(node, parent, out, expanded);
        }
    }

    fn walk_plain(&mut self, node: Node<'a, 'i>, parent: &Inh, out: &mut Vec<Element>, expanded: bool) {
        if !node.is_element() || is_foreign(&node) || !Self::passes_conditions(&node) {
            return;
        }
        match node.tag_name().name() {
            "g" | "a" => self.walk_group(node, parent, out, expanded, false),
            "switch" => self.walk_group(node, parent, out, expanded, true),
            "svg" => self.walk_nested_svg(node, parent, (None, None), out),
            "use" => self.walk_use(node, parent, out),
            tag @ ("rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path") => self.walk_shape(node, tag, parent, out, expanded),
            "text" => self.walk_text(node, parent, out, expanded),
            "image" => self.walk_image(node, parent, out, expanded),
            "foreignObject" => {
                let msg = format!("{} is not supported and was skipped", Self::label(&node));
                self.warn(msg);
            }
            _ => {}
        }
    }

    // ── Markers ───────────────────────────────────────────────────────────────

    /// The marker copies for a shape with `cmds` as its outline: a copy of the
    /// marker content at every vertex it applies to, start markers first, then
    /// the mid ones, then the end ones. The copies sit in the shape's own
    /// coordinate space.
    fn marker_elements(&mut self, node: &Node, inh: &Inh, cmds: &[PathCommand]) -> Vec<Element> {
        if self.clip_mode || inh.hidden {
            return Vec::new();
        }
        let label = Self::label(node);
        let mut defs: [Option<std::rc::Rc<MarkerDef>>; 3] = [None, None, None];
        for (slot, spec) in defs.iter_mut().zip([&inh.marker_start, &inh.marker_mid, &inh.marker_end]) {
            let Some(v) = spec else { continue };
            match parse_url_id(v) {
                Some(id) => *slot = self.marker_def(&label, &id),
                None => self.warn(format!("{label} uses a marker value {v}, which is not supported (only url(#id)); drawn without it")),
            }
        }
        if defs.iter().all(Option::is_none) {
            return Vec::new();
        }
        let verts = markers::vertices(cmds);
        let last = verts.len().saturating_sub(1);
        let mut out = Vec::new();
        for (kind, def_ix) in [(MarkerKind::Start, 0), (MarkerKind::Mid, 1), (MarkerKind::End, 2)] {
            let Some(def) = &defs[def_ix] else { continue };
            for (i, v) in verts.iter().enumerate() {
                let applies = match kind {
                    MarkerKind::Start => i == 0,
                    MarkerKind::End => i == last,
                    MarkerKind::Mid => i != 0 && i != last,
                };
                if !applies {
                    continue;
                }
                if self.emitted + out.len() >= MAX_ELEMENTS {
                    self.warn(format!("the SVG has more than {MAX_ELEMENTS} shapes; the rest of the markers were not imported"));
                    return out;
                }
                out.extend(marker_copy(def, v, kind, inh.stroke_width));
            }
        }
        out
    }

    /// The marker `id`, built once and then reused. An unusable reference is
    /// reported and the marker is left out.
    fn marker_def(&mut self, label: &str, id: &str) -> Option<std::rc::Rc<MarkerDef>> {
        if let Some(found) = self.marker_cache.get(id) {
            return found.clone();
        }
        if self.marker_stack.iter().any(|s| s == id) {
            self.warn(format!("{label} refers to the marker #{id}, which refers back to itself; drawn without it"));
            return None;
        }
        let Some(m) = self.ids.get(id).copied() else {
            self.warn(format!("{label} uses the marker #{id}, which does not exist; drawn without it"));
            return None;
        };
        if m.tag_name().name() != "marker" {
            self.warn(format!("{label} uses #{id} as a marker, but it is a <{}>; drawn without it", m.tag_name().name()));
            return None;
        }
        // The content stands alone: a `use` being expanded around the shape that
        // asked for the marker must not make the marker's own `use` look recursive.
        let outer_uses = std::mem::take(&mut self.use_stack);
        self.marker_stack.push(id.to_string());
        let def = self.build_marker(m).map(std::rc::Rc::new);
        self.marker_stack.pop();
        self.use_stack = outer_uses;
        self.marker_cache.insert(id.to_string(), def.clone());
        def
    }

    fn build_marker(&mut self, m: Node<'a, 'i>) -> Option<MarkerDef> {
        let vp = self.vp;
        let width = self.length(m, "markerWidth", 16.0, vp.0).unwrap_or(3.0);
        let height = self.length(m, "markerHeight", 16.0, vp.1).unwrap_or(3.0);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let vb = view_box(&m);
        let view = viewbox_transform(vb.as_deref(), m.attribute("preserveAspectRatio").unwrap_or(""), width, height);
        let ref_x = self.length(m, "refX", 16.0, vp.0).unwrap_or(0.0);
        let ref_y = self.length(m, "refY", 16.0, vp.1).unwrap_or(0.0);
        let (ref_x, ref_y) = match view.as_ref() {
            Some(t) => geom::apply(t.to_matrix(), ref_x, ref_y),
            None => (ref_x, ref_y),
        };
        let inh = self.inh_chain(m);
        let mut kids = Vec::new();
        self.anim_off += 1;
        for child in m.children().filter(|c| c.is_element() && !is_foreign(c)) {
            self.walk(child, &inh, &mut kids, true);
        }
        self.anim_off -= 1;
        let clip = self.viewport_clips(m) && reaches_outside(&kids, viewport_bounds(&view, width, height));
        Some(MarkerDef {
            kids,
            width,
            height,
            view,
            ref_x,
            ref_y,
            scale_by_stroke: m.attribute("markerUnits").is_none_or(|u| u.trim() != "userSpaceOnUse"),
            orient: markers::parse_orient(m.attribute("orient").unwrap_or("0")),
            clip,
        })
    }

    /// Whether a `symbol`, nested `svg` or `marker` clips to its viewport. All
    /// three clip by default, and `overflow` of `visible` or `auto` switches it
    /// off. `inherit` takes the value of the parent, and with none anywhere up
    /// the tree it falls back to `visible`.
    fn viewport_clips(&mut self, node: Node<'a, 'i>) -> bool {
        let mut current = node;
        loop {
            let value = self.props_for(current).get("overflow").map(|v| v.trim().to_string());
            match value.as_deref() {
                Some("inherit") => match current.parent_element() {
                    Some(parent) => current = parent,
                    None => return false,
                },
                Some(v) => return !overflow_is_open(v),
                None if current == node => return true,
                None => return false,
            }
        }
    }

    /// `<image>` with a PNG or JPEG, from a `data:` URI or a file. The image is
    /// placed by `preserveAspectRatio` into the box the SVG gives, since MSX
    /// stretches an image to its width and height, and a `slice` that spills
    /// over the box is clipped to it.
    fn walk_image(&mut self, node: Node<'a, 'i>, parent: &Inh, out: &mut Vec<Element>, expanded: bool) {
        let Some((inh, props)) = self.begin(node, parent) else { return };
        if inh.hidden {
            return;
        }
        let label = Self::label(&node);
        let href = node.attribute("href").or_else(|| node.attribute((XLINK_NS, "href"))).map(str::trim).unwrap_or("");
        if href.is_empty() {
            return;
        }
        let Some((source, intrinsic)) = self.load_image(&label, href) else { return };

        let (fs, vp) = (inh.font_size, self.vp);
        let x = self.length(node, "x", fs, vp.0).unwrap_or(0.0);
        let y = self.length(node, "y", fs, vp.1).unwrap_or(0.0);
        let (w, h) = (self.length(node, "width", fs, vp.0), self.length(node, "height", fs, vp.1));
        let known = intrinsic.map(|(iw, ih)| (iw as f64, ih as f64));
        // A missing size comes from the image, the other side keeping its ratio.
        let (w, h) = match (w, h, known) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some((iw, ih))) => (w, w * ih / iw),
            (None, Some(h), Some((iw, ih))) => (h * iw / ih, h),
            (None, None, Some((iw, ih))) => (iw, ih),
            _ => {
                self.warn(format!("{label} has no usable size and its pixel size is unknown; skipped"));
                return;
            }
        };
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        let par = raster::parse_aspect_ratio(node.attribute("preserveAspectRatio").unwrap_or(""));
        let ((ix, iy, iw, ih), spill) = match known {
            Some(k) => raster::fit((x, y, w, h), k, par),
            None => ((x, y, w, h), false),
        };
        let bbox = Some(BBox { x, y, w, h });
        let mut comp = self.composite(&node, &props, bbox);
        if spill {
            comp.clips.push(vec![plain_rect(x, y, x + w, y + h)]);
        }
        if let Some(v) = props.get("image-rendering").map(|v| v.trim()) {
            if !matches!(v, "" | "auto" | "optimizeQuality") {
                self.warn(format!("{label} uses image-rendering: {v}, which is not supported; smooth scaling is used"));
            }
        }
        let mut image = msx_ast::Image::new(source, ix, iy, iw, ih);
        image.id = if expanded { None } else { node.attribute("id").map(String::from) };
        image.transform = self.node_transform(node);
        image.style = Style { opacity: Some(element_opacity(&props)), ..Style::default() };
        self.push_leaf(Element::Image(image), Vec::new(), &inh, comp, out);
    }

    /// The embedded bytes or the file reference for `href`, and the size in
    /// pixels when it is known. Only PNG and JPEG can be held; anything else is
    /// reported and skipped.
    fn load_image(&mut self, label: &str, href: &str) -> Option<(msx_ast::MediaSource, Option<(u32, u32)>)> {
        let bytes = match raster::decode_data_uri(href) {
            Some(Ok(bytes)) => bytes,
            Some(Err(e)) => {
                self.warn(format!("{label} has a bad data URI ({e}); skipped"));
                return None;
            }
            None => {
                let lower = href.to_ascii_lowercase();
                if lower.starts_with("http://") || lower.starts_with("https://") {
                    self.warn(format!("{label} points at a web address, which is not fetched; skipped"));
                    return None;
                }
                let path = href.strip_prefix("file://").unwrap_or(href);
                let path = String::from_utf8_lossy(&raster::percent_decode(path)).into_owned();
                match &self.base_dir {
                    Some(dir) => match std::fs::read(dir.join(&path)) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            self.warn(format!("{label} could not read {path} ({e}); skipped"));
                            return None;
                        }
                    },
                    None => {
                        self.warn(format!("{label} refers to the file {path}, which is kept as a path and not embedded because no base directory is known; its size is unknown"));
                        return Some((msx_ast::MediaSource::FileRef(path), None));
                    }
                }
            }
        };
        match raster::sniff(&bytes) {
            f @ (raster::Format::Png | raster::Format::Jpeg) => {
                let size = raster::dimensions(&bytes, f);
                Some((msx_ast::MediaSource::Embedded(bytes), size))
            }
            raster::Format::Other(name) => {
                self.warn(format!("{label} holds an image MSX cannot embed ({name}; only PNG and JPEG); skipped"));
                None
            }
        }
    }

    fn walk_group(&mut self, node: Node<'a, 'i>, parent: &Inh, out: &mut Vec<Element>, expanded: bool, only_first: bool) {
        let Some((inh, props)) = self.begin(node, parent) else { return };
        let mut kids = Vec::new();
        if only_first {
            // <switch> draws its first child only.
            let usable = |c: &Node| c.is_element() && !is_foreign(c) && SWITCHABLE.contains(&c.tag_name().name()) && Self::passes_conditions(c);
            if let Some(child) = node.children().find(usable) {
                self.walk(child, &inh, &mut kids, expanded);
            }
        } else {
            for child in node.children() {
                self.walk(child, &inh, &mut kids, expanded);
            }
        }
        let id = if expanded { None } else { node.attribute("id").map(String::from) };
        let transform = self.node_transform(node);
        let bbox = elements_bbox(&kids);
        let comp = self.composite(&node, &props, bbox);
        self.emit(kids, comp, transform, id, out);
    }

    /// `size` is the width and height a `use` gives, each separately, since a
    /// `use` that sets only one leaves the other to the `svg` itself.
    fn walk_nested_svg(&mut self, node: Node<'a, 'i>, parent: &Inh, size: (Option<f64>, Option<f64>), out: &mut Vec<Element>) {
        let Some((inh, props)) = self.begin(node, parent) else { return };
        let (fs, vp) = (inh.font_size, self.vp);
        let x = self.length(node, "x", fs, vp.0).unwrap_or(0.0);
        let y = self.length(node, "y", fs, vp.1).unwrap_or(0.0);
        let w = size.0.unwrap_or_else(|| self.length(node, "width", fs, vp.0).unwrap_or(vp.0));
        let h = size.1.unwrap_or_else(|| self.length(node, "height", fs, vp.1).unwrap_or(vp.1));
        let vb = view_box(&node);
        let inner_vp = vb.as_ref().map_or((w, h), |v| (v[2], v[3]));
        let saved = std::mem::replace(&mut self.vp, inner_vp);
        let mut kids = Vec::new();
        for child in node.children() {
            self.walk(child, &inh, &mut kids, true);
        }
        self.vp = saved;
        let view = viewbox_transform(vb.as_deref(), node.attribute("preserveAspectRatio").unwrap_or(""), w, h);
        let transform = compose(vec![self.node_transform(node), translate(x, y), view.clone()]);
        let bbox = elements_bbox(&kids);
        let mut comp = self.composite(&node, &props, bbox);
        if self.viewport_clips(node) && reaches_outside(&kids, viewport_bounds(&view, w, h)) {
            comp.clips.push(vec![viewport_rect(&view, w, h)]);
        }
        self.emit(kids, comp, transform, None, out);
    }

    fn walk_use(&mut self, node: Node<'a, 'i>, parent: &Inh, out: &mut Vec<Element>) {
        let Some((inh, props)) = self.begin(node, parent) else { return };
        let href = node.attribute("href").or_else(|| node.attribute((XLINK_NS, "href")));
        let Some(target_id) = href.map(|h| h.trim().trim_start_matches('#')) else { return };
        let Some(target) = self.ids.get(target_id).copied() else {
            self.warn(format!("{} refers to #{target_id}, which does not exist", Self::label(&node)));
            return;
        };
        if self.use_stack.iter().any(|s| s == target_id) || node.ancestors().any(|a| a.id() == target.id()) {
            self.warn(format!("{} refers back to itself and was skipped", Self::label(&node)));
            return;
        }
        if self.emitted >= MAX_ELEMENTS {
            self.warn(format!("the SVG has more than {MAX_ELEMENTS} shapes; the rest was not imported"));
            return;
        }
        if self.depth >= MAX_USE_DEPTH {
            self.warn(format!("{} nests too deeply (a use that refers back to itself?) and was skipped", Self::label(&node)));
            return;
        }
        let (fs, vp) = (inh.font_size, self.vp);
        let x = self.length(node, "x", fs, vp.0).unwrap_or(0.0);
        let y = self.length(node, "y", fs, vp.1).unwrap_or(0.0);
        let (use_w, use_h) = (self.length(node, "width", fs, vp.0), self.length(node, "height", fs, vp.1));
        let size = match (use_w, use_h) {
            (None, None) => None,
            (w, h) => Some((w.unwrap_or(vp.0), h.unwrap_or(vp.1))),
        };

        let mut kids = Vec::new();
        self.depth += 1;
        self.use_stack.push(target_id.to_string());
        match target.tag_name().name() {
            "symbol" => self.walk_symbol(target, &inh, size, &mut kids),
            "svg" => self.walk_nested_svg(target, &inh, (use_w, use_h), &mut kids),
            _ => self.walk(target, &inh, &mut kids, true),
        }
        self.use_stack.pop();
        self.depth -= 1;

        let transform = compose(vec![self.node_transform(node), translate(x, y)]);
        let bbox = elements_bbox(&kids);
        let comp = self.composite(&node, &props, bbox);
        self.emit(kids, comp, transform, None, out);
    }

    fn walk_symbol(&mut self, symbol: Node<'a, 'i>, parent: &Inh, size: Option<(f64, f64)>, out: &mut Vec<Element>) {
        let Some((inh, _)) = self.begin(symbol, parent) else { return };
        let vb = view_box(&symbol);
        let (w, h) = size.unwrap_or(self.vp);
        let inner_vp = vb.as_ref().map_or((w, h), |v| (v[2], v[3]));
        let saved = std::mem::replace(&mut self.vp, inner_vp);
        let mut kids = Vec::new();
        for child in symbol.children() {
            self.walk(child, &inh, &mut kids, true);
        }
        self.vp = saved;
        let view = viewbox_transform(vb.as_deref(), symbol.attribute("preserveAspectRatio").unwrap_or(""), w, h);
        let mut comp = Comp::NONE;
        if self.viewport_clips(symbol) && reaches_outside(&kids, viewport_bounds(&view, w, h)) {
            comp.clips.push(vec![viewport_rect(&view, w, h)]);
        }
        self.emit(kids, comp, view, None, out);
    }

    // ── Shapes ────────────────────────────────────────────────────────────────

    fn walk_shape(&mut self, node: Node<'a, 'i>, tag: &str, parent: &Inh, out: &mut Vec<Element>, expanded: bool) {
        let Some((inh, props)) = self.begin(node, parent) else { return };
        if inh.hidden && (!self.keep_hidden || self.clip_mode) {
            return;
        }
        let (fs, vp) = (inh.font_size, self.vp);
        let (wx, wy, diag) = (vp.0, vp.1, diagonal(vp));
        let num = |s: &Self, name: &str, base: f64| s.length(node, name, fs, base).unwrap_or(0.0);

        let (shape, bbox, kind) = match tag {
            "rect" => {
                let (x, y) = (num(self, "x", wx), num(self, "y", wy));
                let (w, h) = (num(self, "width", wx), num(self, "height", wy));
                if w <= 0.0 || h <= 0.0 {
                    return;
                }
                let radius = |s: &Self, name: &str, base: f64| s.length(node, name, fs, base).filter(|v| *v >= 0.0);
                let (rx, ry) = match (radius(self, "rx", wx), radius(self, "ry", wy)) {
                    (None, None) => (None, None),
                    (Some(a), None) => (Some(a), Some(a)),
                    (None, Some(b)) => (Some(b), Some(b)),
                    (Some(a), Some(b)) => (Some(a), Some(b)),
                };
                let (rx, ry) = (rx.map(|v| v.min(w / 2.0)), ry.map(|v| v.min(h / 2.0)));
                let (rx, ry) = if rx.unwrap_or(0.0) <= 0.0 && ry.unwrap_or(0.0) <= 0.0 { (None, None) } else { (rx, ry) };
                (Shape::Rect { x, y, w, h, rx, ry }, Some(BBox { x, y, w, h }), Kind::Shape)
            }
            "circle" => {
                let (cx, cy, r) = (num(self, "cx", wx), num(self, "cy", wy), num(self, "r", diag));
                if r <= 0.0 {
                    return;
                }
                (Shape::Circle { cx, cy, r }, Some(BBox { x: cx - r, y: cy - r, w: 2.0 * r, h: 2.0 * r }), Kind::Shape)
            }
            "ellipse" => {
                let (cx, cy) = (num(self, "cx", wx), num(self, "cy", wy));
                let rx_attr = self.length(node, "rx", fs, wx).filter(|v| *v > 0.0);
                let ry_attr = self.length(node, "ry", fs, wy).filter(|v| *v > 0.0);
                let (rx, ry) = match (rx_attr, ry_attr) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, a),
                    (None, Some(b)) => (b, b),
                    (None, None) => return,
                };
                (Shape::Ellipse { cx, cy, rx, ry }, Some(BBox { x: cx - rx, y: cy - ry, w: 2.0 * rx, h: 2.0 * ry }), Kind::Shape)
            }
            "line" => {
                let (x1, y1, x2, y2) = (num(self, "x1", wx), num(self, "y1", wy), num(self, "x2", wx), num(self, "y2", wy));
                let b = BBox::from_corners(x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2));
                (Shape::Line { x1, y1, x2, y2 }, Some(b), Kind::Line)
            }
            "polyline" | "polygon" => {
                let nums = number_list(node.attribute("points").unwrap_or(""));
                if nums.len() % 2 == 1 {
                    self.warn(format!("{} has an odd number of coordinates; the last one was dropped", Self::label(&node)));
                }
                let points: Vec<Point> = nums.chunks_exact(2).map(|c| Point::new(c[0], c[1])).collect();
                if points.len() < 2 {
                    return;
                }
                let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for p in &points {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                (Shape::Poly { points, closed: tag == "polygon" }, Some(BBox::from_corners(x0, y0, x1, y1)), Kind::Shape)
            }
            _ => {
                let d = node.attribute("d").unwrap_or("");
                let (commands, err) = parse_d_partial(d);
                if let Some(e) = err {
                    self.warn(format!("{}: path data error ({e}); drawn up to the error", Self::label(&node)));
                }
                if commands.is_empty() {
                    return;
                }
                if !matches!(commands[0], PathCommand::MoveTo(_) | PathCommand::RelMoveTo(_)) {
                    self.warn(format!("{}: path data must start with a moveto, so it was not drawn", Self::label(&node)));
                    return;
                }
                let bbox = path_bbox(&commands);
                (Shape::Path { commands }, bbox, Kind::Shape)
            }
        };

        let comp = self.composite(&node, &props, bbox);
        let mut style = self.leaf_style(&inh, &props, bbox, kind);
        if inh.hidden {
            style.fill = Some(Paint::None);
            style.stroke = Some(Paint::None);
        }
        let id = if expanded { None } else { node.attribute("id").map(String::from) };
        let transform = self.node_transform(node);
        // Markers go on every shape, as SVG 2 has it: a rectangle and an ellipse
        // through the path they stand for. The outline is only built when a
        // marker property is set.
        let wants_markers = inh.marker_start.is_some() || inh.marker_mid.is_some() || inh.marker_end.is_some();
        let marker_cmds = if !wants_markers {
            None
        } else {
            match &shape {
                Shape::Path { commands } => Some(commands.clone()),
                Shape::Line { x1, y1, x2, y2 } => Some(vec![PathCommand::MoveTo(Point::new(*x1, *y1)), PathCommand::LineTo(Point::new(*x2, *y2))]),
                Shape::Poly { points, closed } => {
                    let mut cmds: Vec<PathCommand> = points.iter().enumerate().map(|(i, p)| if i == 0 { PathCommand::MoveTo(*p) } else { PathCommand::LineTo(*p) }).collect();
                    if *closed {
                        cmds.push(PathCommand::ClosePath);
                    }
                    Some(cmds)
                }
                Shape::Rect { x, y, w, h, rx, ry } => Some(markers::rect_commands(*x, *y, *w, *h, *rx, *ry)),
                Shape::Circle { cx, cy, r } => Some(markers::ellipse_commands(*cx, *cy, *r, *r)),
                Shape::Ellipse { cx, cy, rx, ry } => Some(markers::ellipse_commands(*cx, *cy, *rx, *ry)),
            }
        };
        let extras = marker_cmds.map(|cmds| self.marker_elements(&node, &inh, &cmds)).unwrap_or_default();
        if !extras.is_empty() {
            style.opacity = Some(1.0);
        }
        let element = match shape {
            Shape::Rect { x, y, w, h, rx, ry } => Element::Rect(Rect { x, y, width: w, height: h, rx, ry, id, transform, style }),
            Shape::Circle { cx, cy, r } => Element::Circle(Circle { cx, cy, r, id, transform, style }),
            Shape::Ellipse { cx, cy, rx, ry } => Element::Ellipse(Ellipse { cx, cy, rx, ry, id, transform, style }),
            Shape::Line { x1, y1, x2, y2 } => Element::Line(Line { x1, y1, x2, y2, id, transform, style }),
            Shape::Poly { points, closed } => {
                let poly = Polyline { points, closed, id, transform, style };
                if closed { Element::Polygon(poly) } else { Element::Polyline(poly) }
            }
            Shape::Path { commands } => {
                let d_raw = path_data(&commands);
                Element::Path(Path { commands, d_raw, id, transform, style })
            }
        };
        self.push_leaf(element, extras, &inh, comp, out);
    }

    /// Add one shape or text to `out`, split for `paint-order` and wrapped
    /// in a layer for `mix-blend-mode`.
    fn push_leaf(&mut self, el: Element, extras: Vec<Element>, inh: &Inh, comp: Comp, out: &mut Vec<Element>) {
        if self.emitted >= MAX_ELEMENTS {
            self.warn(format!("the SVG has more than {MAX_ELEMENTS} shapes; the rest was not imported"));
            return;
        }
        self.emitted += 1;
        let mut parts = if inh.stroke_first && !matches!(el, Element::Image(_)) { split_paint_order(el) } else { vec![el] };
        let with_extras = !extras.is_empty();
        if comp.blend != BlendMode::Normal || !comp.clips.is_empty() || comp.mask.is_some() || with_extras {
            // A clip, a mask or markers live in the element's own space, so the
            // element's transform moves from the shape to the layer.
            let mut transform = None;
            if !comp.clips.is_empty() || comp.mask.is_some() || with_extras {
                for part in parts.iter_mut() {
                    if let Some(slot) = transform_mut(part) {
                        let t = slot.take();
                        transform = transform.or(t);
                    }
                }
            }
            parts.extend(extras);
            // The shape and its markers fade together, so the opacity that is
            // otherwise in the shape's style sits on the layer.
            let opacity = if with_extras { comp.opacity } else { 1.0 };
            out.push(layered(parts, opacity, comp.blend, comp.clips, comp.mask, transform, None));
        } else {
            out.extend(parts);
        }
    }

    /// Fully resolved style for one shape.
    fn leaf_style(&mut self, inh: &Inh, props: &Props, bbox: Option<BBox>, kind: Kind) -> Style {
        let mut st = Style::default();
        // Inside a clip path only geometry counts: opaque black, no stroke,
        // and `clip-rule` in the place of `fill-rule`.
        let clip = self.clip_mode;
        let fill = if clip {
            Paint::Color(Color::BLACK)
        } else if kind == Kind::Line {
            Paint::None
        } else {
            self.resolve_paint(&inh.fill, inh, inh.fill_opacity, bbox)
        };
        let stroke = if clip { Paint::None } else { self.resolve_paint(&inh.stroke, inh, inh.stroke_opacity, bbox) };
        let rule = if clip { inh.clip_rule } else { inh.fill_rule };
        st.fill = Some(fill);
        st.stroke = Some(stroke);
        st.stroke_width = Some(if clip { 0.0 } else { inh.stroke_width });
        st.opacity = Some(if clip { 1.0 } else { element_opacity(props) });
        st.fill_rule = (rule == msx_ast::FillRule::EvenOdd).then_some(rule);
        st.stroke_linecap = (inh.linecap != msx_ast::LineCap::Butt).then_some(inh.linecap);
        st.stroke_linejoin = (inh.linejoin != msx_ast::LineJoin::Miter).then_some(inh.linejoin);
        st.stroke_miterlimit = (inh.miterlimit != 4.0).then_some(inh.miterlimit);
        st.stroke_dasharray = (!inh.dasharray.is_empty()).then(|| inh.dasharray.clone());
        st.stroke_dashoffset = (inh.dashoffset != 0.0).then_some(inh.dashoffset);
        if kind == Kind::Text {
            st.font_size = Some(inh.font_size);
            st.font_family = inh.font_family.clone();
            st.font_weight = inh.font_weight.clone();
            st.text_anchor = inh.text_anchor;
            st.dominant_baseline = inh.dominant_baseline.clone();
        }
        st
    }

    fn resolve_paint(&mut self, spec: &PaintSpec, inh: &Inh, opacity: f64, bbox: Option<BBox>) -> Paint {
        match spec {
            PaintSpec::None => Paint::None,
            PaintSpec::Current => Paint::Color(with_alpha(inh.color, opacity)),
            PaintSpec::Color(c) => Paint::Color(with_alpha(*c, opacity)),
            PaintSpec::Url { id, fallback, fallback_current } => {
                let is_gradient = self.ids.get(id.as_str()).is_some_and(|n| matches!(n.tag_name().name(), "linearGradient" | "radialGradient"));
                if !is_gradient && *fallback_current {
                    // The reference does not resolve, so the fallback applies.
                    Paint::Color(with_alpha(inh.color, opacity))
                } else {
                    self.paint_for_url(id, fallback, bbox, opacity).unwrap_or(Paint::None)
                }
            }
        }
    }

    // ── Text ──────────────────────────────────────────────────────────────────

    /// Text box of a run, used as the gradient bounding box.
    fn text_bbox(x: f64, y: f64, content: &str, inh: &Inh) -> Option<BBox> {
        let style = Style {
            font_size: Some(inh.font_size),
            font_weight: inh.font_weight.clone(),
            text_anchor: inh.text_anchor,
            dominant_baseline: inh.dominant_baseline.clone(),
            ..Style::default()
        };
        let probe = Text { x, y, content: content.to_string(), id: None, transform: None, style };
        msx_text::layout(&probe).map(|l| BBox { x: l.bounds.0, y: l.bounds.1, w: l.bounds.2, h: l.bounds.3 })
    }

    /// First value of a coordinate list attribute.
    fn first_coord(&self, node: Node, name: &str, font_size: f64, base: f64) -> Option<f64> {
        let raw = node.attribute(name)?;
        let first = raw.split(|c: char| c == ',' || c.is_whitespace()).find(|t| !t.is_empty())?;
        parse_length(first).map(|l| l.to_px(font_size, base))
    }

    fn collect_runs(&mut self, node: Node<'a, 'i>, inh: &Inh, pos: (f64, f64), runs: &mut Vec<Run>, new_run: &mut bool) {
        for child in node.children() {
            if child.is_text() {
                let t = child.text().unwrap_or("");
                if *new_run || runs.is_empty() {
                    runs.push(Run { x: pos.0, y: pos.1, text: t.to_string(), inh: inh.clone() });
                    *new_run = false;
                } else if let Some(last) = runs.last_mut() {
                    let differs = last.inh.fill != inh.fill || last.inh.font_size != inh.font_size || last.inh.font_weight != inh.font_weight;
                    last.text.push_str(t);
                    if differs {
                        self.warn("text with differently styled spans on one line is imported with the style of the line start".to_string());
                    }
                }
            } else if child.is_element() && !is_foreign(&child) {
                match child.tag_name().name() {
                    "tspan" => {
                        let props = self.props_for(child);
                        if props.get("display").is_some_and(|v| v.trim() == "none") {
                            continue;
                        }
                        let mut cinh = inh.clone();
                        apply(&mut cinh, &props, self.vp);
                        if cinh.hidden {
                            continue;
                        }
                        let (fs, vp) = (cinh.font_size, self.vp);
                        let positioned = child.has_attribute("x") || child.has_attribute("y");
                        let mut cpos = pos;
                        if let Some(x) = self.first_coord(child, "x", fs, vp.0) {
                            cpos.0 = x;
                        }
                        if let Some(y) = self.first_coord(child, "y", fs, vp.1) {
                            cpos.1 = y;
                        }
                        cpos.0 += self.first_coord(child, "dx", fs, vp.0).unwrap_or(0.0);
                        cpos.1 += self.first_coord(child, "dy", fs, vp.1).unwrap_or(0.0);
                        if positioned {
                            *new_run = true;
                        }
                        self.collect_runs(child, &cinh, cpos, runs, new_run);
                    }
                    "textPath" => {
                        let msg = format!("{} is not supported (text on a path); its text was skipped", Self::label(&child));
                        self.warn(msg);
                    }
                    _ => {}
                }
            }
        }
    }

    fn walk_text(&mut self, node: Node<'a, 'i>, parent: &Inh, out: &mut Vec<Element>, expanded: bool) {
        let Some((inh, props)) = self.begin(node, parent) else { return };
        if inh.hidden {
            return;
        }
        let (fs, vp) = (inh.font_size, self.vp);
        let x = self.first_coord(node, "x", fs, vp.0).unwrap_or(0.0) + self.first_coord(node, "dx", fs, vp.0).unwrap_or(0.0);
        let y = self.first_coord(node, "y", fs, vp.1).unwrap_or(0.0) + self.first_coord(node, "dy", fs, vp.1).unwrap_or(0.0);

        let mut runs = Vec::new();
        let mut new_run = true;
        self.collect_runs(node, &inh, (x, y), &mut runs, &mut new_run);

        let transform = self.node_transform(node);
        let mut first = true;
        for run in runs {
            let content = run.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if content.is_empty() {
                continue;
            }
            let bbox = Self::text_bbox(run.x, run.y, &content, &run.inh);
            let comp = self.composite(&node, &props, bbox);
            let style = self.leaf_style(&run.inh, &props, bbox, Kind::Text);
            let id = if first && !expanded { node.attribute("id").map(String::from) } else { None };
            first = false;
            let element = Element::Text(Text { x: run.x, y: run.y, content, id, transform: transform.clone(), style });
            self.push_leaf(element, Vec::new(), &run.inh, comp, out);
        }
    }
}

#[cfg(test)]
mod tests;
