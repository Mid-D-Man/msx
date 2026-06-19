// core/msx-ast/src/element.rs

use crate::layer::Layer;
use crate::path::PathCommand;
use crate::primitives::Point;
use crate::sdf::SdfNode;
use crate::splat::GaussianSplat;
use crate::style::Style;
use crate::transform::Transform;

/// All renderable MSX elements.
#[derive(Debug, Clone)]
pub enum Element {
    // ── Standard vector shapes ────────────────────────────────────────────────
    Rect(Rect),
    Circle(Circle),
    Ellipse(Ellipse),
    Line(Line),
    Polyline(Polyline),
    Polygon(Polyline),
    Path(Path),
    Text(Text),
    Group(Group),
    Use(Use),
    // ── New primitives (v0.2) ─────────────────────────────────────────────────
    /// Signed Distance Field compound shape.
    Sdf(SdfNode),
    /// Soft Gaussian splat blob.
    Splat(GaussianSplat),
    /// Compositing layer with explicit blend mode and effects.
    Layer(Layer),
}

impl Element {
    pub fn id(&self) -> Option<&str> {
        match self {
            Element::Rect(e)     => e.id.as_deref(),
            Element::Circle(e)   => e.id.as_deref(),
            Element::Ellipse(e)  => e.id.as_deref(),
            Element::Line(e)     => e.id.as_deref(),
            Element::Polyline(e) => e.id.as_deref(),
            Element::Polygon(e)  => e.id.as_deref(),
            Element::Path(e)     => e.id.as_deref(),
            Element::Text(e)     => e.id.as_deref(),
            Element::Group(e)    => e.id.as_deref(),
            Element::Use(e)      => e.id.as_deref(),
            Element::Sdf(e)      => e.id.as_deref(),
            Element::Splat(e)    => e.id.as_deref(),
            Element::Layer(e)    => e.id.as_deref(),
        }
    }

    pub fn transform(&self) -> Option<&Transform> {
        match self {
            Element::Rect(e)     => e.transform.as_ref(),
            Element::Circle(e)   => e.transform.as_ref(),
            Element::Ellipse(e)  => e.transform.as_ref(),
            Element::Line(e)     => e.transform.as_ref(),
            Element::Polyline(e) => e.transform.as_ref(),
            Element::Polygon(e)  => e.transform.as_ref(),
            Element::Path(e)     => e.transform.as_ref(),
            Element::Text(e)     => e.transform.as_ref(),
            Element::Group(e)    => e.transform.as_ref(),
            Element::Use(e)      => e.transform.as_ref(),
            Element::Sdf(e)      => e.transform.as_ref(),
            Element::Splat(_)    => None,
            Element::Layer(e)    => e.transform.as_ref(),
        }
    }

    pub fn tag_name(&self) -> &'static str {
        match self {
            Element::Rect(_)     => "rect",
            Element::Circle(_)   => "circle",
            Element::Ellipse(_)  => "ellipse",
            Element::Line(_)     => "line",
            Element::Polyline(_) => "polyline",
            Element::Polygon(_)  => "polygon",
            Element::Path(_)     => "path",
            Element::Text(_)     => "text",
            Element::Group(_)    => "g",
            Element::Use(_)      => "use",
            Element::Sdf(_)      => "sdf",
            Element::Splat(_)    => "splat",
            Element::Layer(_)    => "layer",
        }
    }

    /// Whether this element type is native-renderable in SVG 1.1.
    pub fn is_svg_native(&self) -> bool {
        !matches!(self, Element::Sdf(_) | Element::Splat(_))
    }
}

// ── Concrete element structs ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Rect {
    pub x:         f64,
    pub y:         f64,
    pub width:     f64,
    pub height:    f64,
    pub rx:        Option<f64>,
    pub ry:        Option<f64>,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64, style: Style) -> Self {
        Rect { x, y, width, height, rx: None, ry: None, id: None, transform: None, style }
    }
}

#[derive(Debug, Clone)]
pub struct Circle {
    pub cx:        f64,
    pub cy:        f64,
    pub r:         f64,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

impl Circle {
    pub fn new(cx: f64, cy: f64, r: f64, style: Style) -> Self {
        Circle { cx, cy, r, id: None, transform: None, style }
    }
}

#[derive(Debug, Clone)]
pub struct Ellipse {
    pub cx:        f64,
    pub cy:        f64,
    pub rx:        f64,
    pub ry:        f64,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

impl Ellipse {
    pub fn new(cx: f64, cy: f64, rx: f64, ry: f64, style: Style) -> Self {
        Ellipse { cx, cy, rx, ry, id: None, transform: None, style }
    }
}

#[derive(Debug, Clone)]
pub struct Line {
    pub x1:        f64,
    pub y1:        f64,
    pub x2:        f64,
    pub y2:        f64,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

impl Line {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64, style: Style) -> Self {
        Line { x1, y1, x2, y2, id: None, transform: None, style }
    }
}

/// Shared type for polyline (closed=false) and polygon (closed=true).
#[derive(Debug, Clone)]
pub struct Polyline {
    pub points:    Vec<Point>,
    pub closed:    bool,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

pub type Polygon = Polyline;

#[derive(Debug, Clone)]
pub struct Path {
    pub commands:  Vec<PathCommand>,
    /// Cached original `d` string for roundtrip fidelity.
    pub d_raw:     String,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub x:         f64,
    pub y:         f64,
    pub content:   String,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

impl Text {
    pub fn new(x: f64, y: f64, content: String, style: Style) -> Self {
        Text { x, y, content, id: None, transform: None, style }
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub children:  Vec<Element>,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    /// Inheritable styles applied to all children.
    pub style:     Option<Style>,
}

impl Group {
    pub fn new(children: Vec<Element>) -> Self {
        Group { children, id: None, transform: None, style: None }
    }
}

#[derive(Debug, Clone)]
pub struct Use {
    pub href:      String,
    pub x:         f64,
    pub y:         f64,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
}
