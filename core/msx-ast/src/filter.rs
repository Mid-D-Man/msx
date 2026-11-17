// core/msx-ast/src/filter.rs
//! The filter graph of `Effect::Graph`: a list of primitives where each reads
//! the layer pixels, or the result of an earlier primitive.
//!
//! NOTICE: see docs/guide/layers.md, section "Filter graph". A graph is the
//! form of an SVG `<filter>` that a chain of single input effects cannot say:
//! `feFlood`, `feComposite`, `feMerge`, `feBlend`, `feComponentTransfer`, named
//! results and primitive regions. Wire version 3.
//!
//! The result of the graph is the result of its last node. A node can only
//! read nodes before it, so a graph has no cycles and is evaluated in order.

use crate::{BlendMode, Color, Matrix2D, Point};

/// Most nodes a graph can hold. A file with more is refused when it is read.
pub const MAX_FILTER_NODES: usize = 256;
/// Most entries in a `Table` or `Discrete` transfer function.
pub const MAX_TRANSFER_VALUES: usize = 1024;
/// Most inputs of a `Merge`.
pub const MAX_MERGE_INPUTS: usize = 64;

/// Where a primitive reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterInput {
    /// The pixels of the layer, before any effect.
    SourceGraphic,
    /// The alpha of the layer with the color set to black.
    SourceAlpha,
    /// The result of an earlier node, by index.
    Node(u16),
}

impl FilterInput {
    /// The name used in `.msx` source: `source`, `alpha`, or the index.
    pub fn source_name(self) -> String {
        match self {
            FilterInput::SourceGraphic => "source".to_string(),
            FilterInput::SourceAlpha => "alpha".to_string(),
            FilterInput::Node(i) => i.to_string(),
        }
    }

    /// The inverse of `source_name`.
    pub fn from_source_name(s: &str) -> Option<FilterInput> {
        match s.trim() {
            "source" => Some(FilterInput::SourceGraphic),
            "alpha" => Some(FilterInput::SourceAlpha),
            n => n.parse::<u16>().ok().map(FilterInput::Node),
        }
    }
}

/// How `feComposite` combines two inputs. `a` is `in` and `b` is `in2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompositeOp {
    /// `a` over `b`.
    Over,
    /// The part of `a` that is inside `b`.
    In,
    /// The part of `a` that is outside `b`.
    Out,
    /// `a` where `b` is, and `b` elsewhere.
    Atop,
    /// `a` and `b` where the other is not.
    Xor,
    /// `k1 a b + k2 a + k3 b + k4` on every premultiplied channel, held to 0 to 1.
    Arithmetic { k1: f64, k2: f64, k3: f64, k4: f64 },
}

impl CompositeOp {
    pub fn source_name(self) -> &'static str {
        match self {
            CompositeOp::Over => "over",
            CompositeOp::In => "in",
            CompositeOp::Out => "out",
            CompositeOp::Atop => "atop",
            CompositeOp::Xor => "xor",
            CompositeOp::Arithmetic { .. } => "arithmetic",
        }
    }
}

/// One channel of `feComponentTransfer`, on straight values in 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub enum TransferFn {
    Identity,
    /// Linear interpolation between the values, evenly spaced.
    Table(Vec<f64>),
    /// A staircase with one step per value.
    Discrete(Vec<f64>),
    /// `slope * c + intercept`.
    Linear { slope: f64, intercept: f64 },
    /// `amplitude * c ^ exponent + offset`.
    Gamma { amplitude: f64, exponent: f64, offset: f64 },
}

impl TransferFn {
    /// The value for the channel value `c` in 0 to 1, held to 0 to 1.
    pub fn apply(&self, c: f64) -> f64 {
        let v = match self {
            TransferFn::Identity => c,
            TransferFn::Table(t) if t.is_empty() => c,
            TransferFn::Table(t) => {
                let n = t.len() - 1;
                if n == 0 {
                    t[0]
                } else {
                    let k = ((c * n as f64).floor() as usize).min(n - 1);
                    let (v0, v1) = (t[k], t[k + 1]);
                    v0 + (c - k as f64 / n as f64) * n as f64 * (v1 - v0)
                }
            }
            TransferFn::Discrete(t) if t.is_empty() => c,
            TransferFn::Discrete(t) => {
                let n = t.len();
                t[((c * n as f64).floor() as usize).min(n - 1)]
            }
            TransferFn::Linear { slope, intercept } => slope * c + intercept,
            TransferFn::Gamma { amplitude, exponent, offset } => amplitude * c.powf(*exponent) + offset,
        };
        v.clamp(0.0, 1.0)
    }

    pub fn source_name(&self) -> &'static str {
        match self {
            TransferFn::Identity => "identity",
            TransferFn::Table(_) => "table",
            TransferFn::Discrete(_) => "discrete",
            TransferFn::Linear { .. } => "linear",
            TransferFn::Gamma { .. } => "gamma",
        }
    }
}

/// What a node does.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    /// Fills the region with a color. The alpha of the color is the flood opacity.
    Flood { color: Color },
    Composite { a: FilterInput, b: FilterInput, op: CompositeOp },
    /// The inputs painted over each other, the first at the bottom.
    Merge { inputs: Vec<FilterInput> },
    /// `a` drawn over `b` with a blend mode.
    Blend { a: FilterInput, b: FilterInput, mode: BlendMode },
    /// Moves the input by whole pixels (the amount is cut toward zero).
    Offset { input: FilterInput, dx: f64, dy: f64 },
    /// Gaussian blur with a standard deviation per axis.
    Blur { input: FilterInput, rx: f64, ry: f64 },
    /// A 4x5 matrix, row major, as in `Effect::ColorMatrix`.
    ColorMatrix { input: FilterInput, values: [f64; 20] },
    /// One function each for red, green, blue and alpha.
    ComponentTransfer { input: FilterInput, funcs: [TransferFn; 4] },
}

impl FilterOp {
    /// Every input the op reads.
    pub fn inputs(&self) -> Vec<FilterInput> {
        match self {
            FilterOp::Flood { .. } => Vec::new(),
            FilterOp::Composite { a, b, .. } | FilterOp::Blend { a, b, .. } => vec![*a, *b],
            FilterOp::Merge { inputs } => inputs.clone(),
            FilterOp::Offset { input, .. }
            | FilterOp::Blur { input, .. }
            | FilterOp::ColorMatrix { input, .. }
            | FilterOp::ComponentTransfer { input, .. } => vec![*input],
        }
    }

    pub fn source_name(&self) -> &'static str {
        match self {
            FilterOp::Flood { .. } => "flood",
            FilterOp::Composite { .. } => "composite",
            FilterOp::Merge { .. } => "merge",
            FilterOp::Blend { .. } => "blend",
            FilterOp::Offset { .. } => "offset",
            FilterOp::Blur { .. } => "blur",
            FilterOp::ColorMatrix { .. } => "color_matrix",
            FilterOp::ComponentTransfer { .. } => "component_transfer",
        }
    }
}

/// The rectangle a node paints in, in the units of the layer. Outside it the
/// result of the node is transparent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl FilterRegion {
    /// The box that holds this region after `m`, which is the region itself for
    /// a scale or a translation.
    pub fn transformed(&self, m: &Matrix2D) -> FilterRegion {
        let corners = [
            (self.x, self.y),
            (self.x + self.width, self.y),
            (self.x, self.y + self.height),
            (self.x + self.width, self.y + self.height),
        ]
        .map(|(x, y)| m.transform_point(Point { x, y }));
        let min_x = corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let max_x = corners.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = corners.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        FilterRegion { x: min_x, y: min_y, width: max_x - min_x, height: max_y - min_y }
    }
}

/// One primitive of the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterNode {
    pub op: FilterOp,
    /// `None` is the whole layer. A node paints nothing outside its region.
    pub region: Option<FilterRegion>,
    /// Whether the node works on linear light instead of sRGB values, which
    /// is what `color-interpolation-filters` sets. `Flood` and `Offset` keep
    /// the space of what they read, so the flag does nothing for them.
    pub linear: bool,
}

impl FilterNode {
    pub fn new(op: FilterOp) -> Self {
        FilterNode { op, region: None, linear: false }
    }
}

/// A filter graph. The result is the last node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilterGraph {
    pub nodes: Vec<FilterNode>,
}

impl FilterGraph {
    /// Checks what the readers rely on: at most `MAX_FILTER_NODES` nodes, every
    /// node input an earlier node, finite numbers, regions with an area, and
    /// the list sizes of merges and transfer tables within their limits.
    pub fn validate(&self) -> Result<(), String> {
        if self.nodes.len() > MAX_FILTER_NODES {
            return Err(format!("a filter graph has at most {MAX_FILTER_NODES} nodes, found {}", self.nodes.len()));
        }
        for (i, node) in self.nodes.iter().enumerate() {
            for input in node.op.inputs() {
                if let FilterInput::Node(j) = input {
                    if usize::from(j) >= i {
                        return Err(format!("filter node {i} reads node {j}, which is not before it"));
                    }
                }
            }
            if let Some(r) = node.region {
                if ![r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()) || r.width <= 0.0 || r.height <= 0.0 {
                    return Err(format!("filter node {i} has a region without a finite area"));
                }
            }
            let finite = |vs: &[f64]| vs.iter().all(|v| v.is_finite());
            let ok = match &node.op {
                FilterOp::Flood { .. } => true,
                FilterOp::Composite { op: CompositeOp::Arithmetic { k1, k2, k3, k4 }, .. } => finite(&[*k1, *k2, *k3, *k4]),
                FilterOp::Composite { .. } | FilterOp::Blend { .. } => true,
                FilterOp::Merge { inputs } => inputs.len() <= MAX_MERGE_INPUTS,
                FilterOp::Offset { dx, dy, .. } => finite(&[*dx, *dy]),
                FilterOp::Blur { rx, ry, .. } => finite(&[*rx, *ry]),
                FilterOp::ColorMatrix { values, .. } => finite(values),
                FilterOp::ComponentTransfer { funcs, .. } => funcs.iter().all(|f| match f {
                    TransferFn::Identity => true,
                    TransferFn::Table(t) | TransferFn::Discrete(t) => t.len() <= MAX_TRANSFER_VALUES && finite(t),
                    TransferFn::Linear { slope, intercept } => finite(&[*slope, *intercept]),
                    TransferFn::Gamma { amplitude, exponent, offset } => finite(&[*amplitude, *exponent, *offset]),
                }),
            };
            if !ok {
                return Err(format!("filter node {i} ({}) has a value that is not finite, or a list that is too long", node.op.source_name()));
            }
        }
        Ok(())
    }

    /// The graph as the renderer sees it under `m`: blur amounts, offsets and
    /// regions follow the matrix the way the other effects do.
    pub fn scaled(&self, m: &Matrix2D) -> FilterGraph {
        let nodes = self
            .nodes
            .iter()
            .map(|node| {
                let op = match &node.op {
                    FilterOp::Offset { input, dx, dy } => {
                        FilterOp::Offset { input: *input, dx: m.a * dx + m.c * dy, dy: m.b * dx + m.d * dy }
                    }
                    FilterOp::Blur { input, rx, ry } => {
                        let (rx, ry) = scale_blur(*rx, *ry, m);
                        FilterOp::Blur { input: *input, rx, ry }
                    }
                    other => other.clone(),
                };
                FilterNode { op, region: node.region.map(|r| r.transformed(m)), linear: node.linear }
            })
            .collect();
        FilterGraph { nodes }
    }

    /// How far the graph can move or spread pixels, for sizing a buffer.
    pub fn max_extent(&self) -> f64 {
        self.nodes
            .iter()
            .map(|n| match &n.op {
                FilterOp::Blur { rx, ry, .. } => rx.max(*ry).max(0.0) * 3.0,
                FilterOp::Offset { dx, dy, .. } => dx.abs().max(dy.abs()),
                _ => 0.0,
            })
            .sum()
    }
}

/// A blur with an amount for each axis under the matrix `m`. A matrix that
/// keeps the axes (scale, flip, quarter turn) scales each amount by the
/// length of the axis it lands on. Any other matrix has no axis aligned
/// answer, so both amounts use the mean scale.
pub(crate) fn scale_blur(rx: f64, ry: f64, m: &Matrix2D) -> (f64, f64) {
    let s = (m.a * m.d - m.b * m.c).abs().sqrt();
    let (sx, sy) = (m.a.hypot(m.b), m.c.hypot(m.d));
    let keeps_axes = (m.b.abs() < 1e-9 && m.c.abs() < 1e-9) || (m.a.abs() < 1e-9 && m.d.abs() < 1e-9);
    if !keeps_axes {
        (rx * s, ry * s)
    } else if m.b.abs() < 1e-9 {
        (rx * sx, ry * sy)
    } else {
        (ry * sy, rx * sx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn flood() -> FilterNode {
        FilterNode::new(FilterOp::Flood { color: Color::rgb(255, 0, 0) })
    }

    #[test]
    fn inputs_have_a_source_name_that_reads_back() {
        for i in [FilterInput::SourceGraphic, FilterInput::SourceAlpha, FilterInput::Node(0), FilterInput::Node(41)] {
            assert_eq!(FilterInput::from_source_name(&i.source_name()), Some(i));
        }
        assert_eq!(FilterInput::from_source_name(" source "), Some(FilterInput::SourceGraphic));
        assert_eq!(FilterInput::from_source_name("-1"), None);
        assert_eq!(FilterInput::from_source_name("fill"), None);
        assert_eq!(FilterInput::from_source_name("70000"), None);
    }

    #[test]
    fn a_node_can_only_read_nodes_before_it() {
        let read = |i| FilterNode::new(FilterOp::Offset { input: FilterInput::Node(i), dx: 1.0, dy: 0.0 });
        let ok = FilterGraph { nodes: vec![flood(), read(0)] };
        assert!(ok.validate().is_ok());
        let forward = FilterGraph { nodes: vec![flood(), read(1)] };
        assert!(forward.validate().unwrap_err().contains("not before it"));
        let first = FilterGraph { nodes: vec![read(0)] };
        assert!(first.validate().is_err());
        assert!(FilterGraph::default().validate().is_ok());
    }

    #[test]
    fn limits_and_non_finite_numbers_are_refused() {
        let many = FilterGraph { nodes: vec![flood(); MAX_FILTER_NODES + 1] };
        assert!(many.validate().is_err());
        assert!(FilterGraph { nodes: vec![flood(); MAX_FILTER_NODES] }.validate().is_ok());

        let mut n = flood();
        n.region = Some(FilterRegion { x: 0.0, y: 0.0, width: 0.0, height: 5.0 });
        assert!(FilterGraph { nodes: vec![n.clone()] }.validate().unwrap_err().contains("region"));
        n.region = Some(FilterRegion { x: f64::NAN, y: 0.0, width: 1.0, height: 5.0 });
        assert!(FilterGraph { nodes: vec![n] }.validate().is_err());

        let blur = FilterNode::new(FilterOp::Blur { input: FilterInput::SourceGraphic, rx: f64::INFINITY, ry: 1.0 });
        assert!(FilterGraph { nodes: vec![blur] }.validate().is_err());
        let merge = FilterNode::new(FilterOp::Merge { inputs: vec![FilterInput::SourceGraphic; MAX_MERGE_INPUTS + 1] });
        assert!(FilterGraph { nodes: vec![merge] }.validate().is_err());
        let table = FilterNode::new(FilterOp::ComponentTransfer {
            input: FilterInput::SourceGraphic,
            funcs: [TransferFn::Table(vec![0.0; MAX_TRANSFER_VALUES + 1]), TransferFn::Identity, TransferFn::Identity, TransferFn::Identity],
        });
        assert!(FilterGraph { nodes: vec![table] }.validate().is_err());
    }

    #[test]
    fn transfer_functions_follow_the_specification() {
        let id = TransferFn::Identity;
        assert!(close(id.apply(0.3), 0.3));
        let lin = TransferFn::Linear { slope: 2.0, intercept: -0.5 };
        assert!(close(lin.apply(0.5), 0.5) && close(lin.apply(0.0), 0.0) && close(lin.apply(1.0), 1.0), "held to 0 to 1");
        let gamma = TransferFn::Gamma { amplitude: 1.0, exponent: 2.0, offset: 0.0 };
        assert!(close(gamma.apply(0.5), 0.25));
        // Table: values 0, 1, 0 over three evenly spaced stops.
        let table = TransferFn::Table(vec![0.0, 1.0, 0.0]);
        assert!(close(table.apply(0.25), 0.5) && close(table.apply(0.5), 1.0) && close(table.apply(1.0), 0.0));
        assert!(close(TransferFn::Table(vec![0.4]).apply(0.9), 0.4));
        assert!(close(TransferFn::Table(vec![]).apply(0.9), 0.9));
        // Discrete: three steps.
        let steps = TransferFn::Discrete(vec![0.1, 0.5, 0.9]);
        assert!(close(steps.apply(0.0), 0.1) && close(steps.apply(0.34), 0.5) && close(steps.apply(0.67), 0.9) && close(steps.apply(1.0), 0.9));
        assert!(close(TransferFn::Discrete(vec![]).apply(0.2), 0.2));
    }

    #[test]
    fn scaling_moves_offsets_regions_and_blurs() {
        let mut offset = FilterNode::new(FilterOp::Offset { input: FilterInput::SourceGraphic, dx: 2.0, dy: 1.0 });
        offset.region = Some(FilterRegion { x: 1.0, y: 2.0, width: 10.0, height: 20.0 });
        let blur = FilterNode::new(FilterOp::Blur { input: FilterInput::Node(0), rx: 2.0, ry: 5.0 });
        let g = FilterGraph { nodes: vec![offset, blur] };
        let m = Matrix2D { a: 3.0, b: 0.0, c: 0.0, d: 2.0, e: 10.0, f: 20.0 };
        let s = g.scaled(&m);
        let FilterOp::Offset { dx, dy, .. } = &s.nodes[0].op else { unreachable!() };
        assert!(close(*dx, 6.0) && close(*dy, 2.0));
        let r = s.nodes[0].region.unwrap();
        assert!(close(r.x, 13.0) && close(r.y, 24.0) && close(r.width, 30.0) && close(r.height, 40.0), "{r:?}");
        let FilterOp::Blur { rx, ry, .. } = &s.nodes[1].op else { unreachable!() };
        assert!(close(*rx, 6.0) && close(*ry, 10.0));
        // A quarter turn swaps the blur axes and the region keeps its box.
        let q = g.scaled(&Matrix2D::rotate_deg(90.0));
        let FilterOp::Blur { rx, ry, .. } = &q.nodes[1].op else { unreachable!() };
        assert!(close(*rx, 5.0) && close(*ry, 2.0), "{rx} {ry}");
        assert_eq!(s.nodes[1].region, None);
        assert!(g.max_extent() > 15.0);
    }
}
