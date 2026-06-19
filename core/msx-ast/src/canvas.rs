// core/msx-ast/src/canvas.rs

use crate::color::Color;
use crate::element::Element;
use crate::gradient::Def;
use crate::primitives::ViewBox;

/// Drawing canvas — dimensions, background color, optional viewbox.
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

/// Top-level scene graph — the root of an MSX file after parsing or decoding.
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

    /// Total recursive element count (groups and layers count their children).
    pub fn element_count(&self) -> usize {
        count_recursive(&self.elements)
    }
}

fn count_recursive(elements: &[Element]) -> usize {
    elements.iter().map(|e| match e {
        Element::Group(g) => 1 + count_recursive(&g.children),
        Element::Layer(l) => 1 + count_recursive(&l.children),
        _ => 1,
    }).sum()
}
