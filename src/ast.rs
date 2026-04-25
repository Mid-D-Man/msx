// src/ast.rs
//! MSX Scene graph — the in-memory representation after DixScript evaluation.
//! Pure Rust structs; no DixScript or MBFA dependency.

use crate::color::{Color, LinearGradient, Paint, RadialGradient};
use crate::path::PathCommand;
use crate::primitives::{Point, ViewBox};
use crate::style::Style;
use crate::transform::Transform;

// ── Canvas ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Canvas {
    pub width:      f64,
    pub height:     f64,
    pub background: Color,
    pub viewbox:    Option<ViewBox>,
}

impl Canvas {
    pub fn new(width: f64, height: f64, background: Color) -> Self {
        Canvas { width, height, background, viewbox: None }
    }
}

// ── Def enum — gradient / pattern definitions ─────────────────────────────────

#[derive(Debug, Clone)]
pub enum Def {
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
}

impl Def {
    pub fn id(&self) -> &str {
        match self {
            Def::LinearGradient(g) => &g.id,
            Def::RadialGradient(g) => &g.id,
        }
    }

    pub fn to_svg(&self) -> String {
        match self {
            Def::LinearGradient(g) => g.to_svg(),
            Def::RadialGradient(g) => g.to_svg(),
        }
    }
}

// ── Element enum ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Element {
    Rect(Rect),
    Circle(Circle),
    Ellipse(Ellipse),
    Line(Line),
    Polyline(Polyline),
    Polygon(Polygon),
    Path(Path),
    Text(Text),
    Group(Group),
    Use(Use),
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
        }
    }
}

// ── Concrete element types ────────────────────────────────────────────────────

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

#[derive(Debug, Clone)]
pub struct Circle {
    pub cx:        f64,
    pub cy:        f64,
    pub r:         f64,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
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

#[derive(Debug, Clone)]
pub struct Polyline {
    pub points:    Vec<Point>,
    pub closed:    bool,   // false = polyline, true = polygon
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

pub type Polygon = Polyline; // same struct, closed = true in renderer

#[derive(Debug, Clone)]
pub struct Path {
    pub commands:  Vec<PathCommand>,
    /// Original d-string cached for binary round-trips.
    pub d_raw:     String,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    pub style:     Style,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub x:                  f64,
    pub y:                  f64,
    pub content:            String,
    pub id:                 Option<String>,
    pub transform:          Option<Transform>,
    pub style:              Style,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub children:  Vec<Element>,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
    /// Inheritable styles applied to all children.
    pub style:     Option<Style>,
}

#[derive(Debug, Clone)]
pub struct Use {
    pub href:      String,
    pub x:         f64,
    pub y:         f64,
    pub id:        Option<String>,
    pub transform: Option<Transform>,
}

// ── Scene ─────────────────────────────────────────────────────────────────────

/// Top-level scene graph produced by the MSX parser.
#[derive(Debug, Clone)]
pub struct Scene {
    pub canvas:   Canvas,
    pub defs:     Vec<Def>,
    pub elements: Vec<Element>,
}

impl Scene {
    pub fn new(canvas: Canvas) -> Self {
        Scene { canvas, defs: Vec::new(), elements: Vec::new() }
    }

    pub fn element_count(&self) -> usize {
        count_recursive(&self.elements)
    }
}

fn count_recursive(elements: &[Element]) -> usize {
    elements.iter().map(|e| match e {
        Element::Group(g) => 1 + count_recursive(&g.children),
        _ => 1,
    }).sum()
}

// ── Builders — thin convenience constructors ──────────────────────────────────

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64, style: Style) -> Self {
        Rect { x, y, width, height, rx: None, ry: None, id: None, transform: None, style }
    }
}

impl Circle {
    pub fn new(cx: f64, cy: f64, r: f64, style: Style) -> Self {
        Circle { cx, cy, r, id: None, transform: None, style }
    }
}

impl Ellipse {
    pub fn new(cx: f64, cy: f64, rx: f64, ry: f64, style: Style) -> Self {
        Ellipse { cx, cy, rx, ry, id: None, transform: None, style }
    }
}

impl Line {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64, style: Style) -> Self {
        Line { x1, y1, x2, y2, id: None, transform: None, style }
    }
}

impl Text {
    pub fn new(x: f64, y: f64, content: String, style: Style) -> Self {
        Text { x, y, content, id: None, transform: None, style }
    }
}

impl Group {
    pub fn new(children: Vec<Element>) -> Self {
        Group { children, id: None, transform: None, style: None }
    }
}
