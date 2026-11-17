// render/msx-render-cpu/src/filter_graph.rs
//! Evaluates a filter graph (`Effect::Graph`) on the layer buffer.
//!
//! NOTICE: see docs/guide/layers.md, section "Filter graph". Every node makes
//! an image the size of the buffer, in premultiplied `f32`, tagged with the
//! color space it is in (sRGB values or linear light). A node converts what it
//! reads to its own space, as the SVG filter model does, and the result of
//! the last node goes back to sRGB and into the buffer. `Flood` and `Offset`
//! keep the space of what they read or make, so the `linear` flag does not
//! reach them. A node with a region clears everything outside it.

use msx_ast::{BlendMode, CompositeOp, FilterGraph, FilterInput, FilterNode, FilterOp, FilterRegion, TransferFn};
use rayon::prelude::*;
use tiny_skia::Pixmap;

use crate::effects::{blur_rows, transpose};

/// A premultiplied RGBA image, four `f32` per pixel in 0 to 1.
#[derive(Clone)]
struct Img {
    data: Vec<f32>,
    /// Whether the color channels are linear light.
    linear: bool,
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

impl Img {
    fn transparent(len: usize) -> Img {
        Img { data: vec![0.0; len], linear: false }
    }

    /// The image with its colors in `linear` light or sRGB values.
    fn in_space(mut self, linear: bool) -> Img {
        if self.linear != linear {
            let f: fn(f32) -> f32 = if linear { to_linear } else { to_srgb };
            self.data.par_chunks_exact_mut(4).for_each(|px| {
                let a = px[3];
                if a > 0.0 {
                    for c in &mut px[..3] {
                        *c = f((*c / a).clamp(0.0, 1.0)) * a;
                    }
                }
            });
            self.linear = linear;
        }
        self
    }

    /// Runs `f` on the straight (not premultiplied) color and alpha of every
    /// pixel and stores the premultiplied answer.
    fn map_straight(&mut self, f: impl Fn([f32; 4]) -> [f32; 4] + Sync) {
        self.data.par_chunks_exact_mut(4).for_each(|px| {
            let a = px[3];
            let straight = if a > 0.0 { [(px[0] / a).clamp(0.0, 1.0), (px[1] / a).clamp(0.0, 1.0), (px[2] / a).clamp(0.0, 1.0), a] } else { [0.0, 0.0, 0.0, 0.0] };
            let [r, g, b, na] = f(straight);
            let na = na.clamp(0.0, 1.0);
            px[0] = r.clamp(0.0, 1.0) * na;
            px[1] = g.clamp(0.0, 1.0) * na;
            px[2] = b.clamp(0.0, 1.0) * na;
            px[3] = na;
        });
    }

    /// Clears everything outside the device rectangle of `region`.
    fn clip(&mut self, width: usize, height: usize, region: FilterRegion) {
        let x0 = region.x.floor().clamp(0.0, width as f64) as usize;
        let y0 = region.y.floor().clamp(0.0, height as f64) as usize;
        let x1 = (region.x.floor() + region.width.ceil()).clamp(0.0, width as f64) as usize;
        let y1 = (region.y.floor() + region.height.ceil()).clamp(0.0, height as f64) as usize;
        self.data.par_chunks_mut(width * 4).enumerate().for_each(|(y, row)| {
            for x in 0..width {
                if y < y0 || y >= y1 || x < x0 || x >= x1 {
                    row[x * 4..x * 4 + 4].fill(0.0);
                }
            }
        });
    }
}

/// Runs the graph on `pixmap` and writes the result back. An empty graph
/// leaves the buffer as it is.
pub fn apply_graph(pixmap: &mut Pixmap, graph: &FilterGraph) {
    if graph.nodes.is_empty() || graph.validate().is_err() {
        return;
    }
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    let source = Img { data: pixmap.data().iter().map(|&b| f32::from(b) / 255.0).collect(), linear: false };
    let mut alpha: Option<Img> = None;

    // The last node that reads each node, so an image is dropped once nobody needs it.
    let mut last_use = vec![0usize; graph.nodes.len()];
    for (i, node) in graph.nodes.iter().enumerate() {
        for input in node.op.inputs() {
            if let FilterInput::Node(j) = input {
                last_use[usize::from(j)] = i;
            }
        }
    }

    let mut results: Vec<Option<Img>> = Vec::with_capacity(graph.nodes.len());
    for (i, node) in graph.nodes.iter().enumerate() {
        // `Some(space)` converts what is read to that space, `None` reads it as it is.
        let mut get = |input: FilterInput, space: Option<bool>| -> Img {
            let img = match input {
                FilterInput::SourceGraphic => source.clone(),
                FilterInput::SourceAlpha => alpha
                    .get_or_insert_with(|| {
                        let mut a = source.clone();
                        a.data.par_chunks_exact_mut(4).for_each(|px| {
                            px[0] = 0.0;
                            px[1] = 0.0;
                            px[2] = 0.0;
                        });
                        a
                    })
                    .clone(),
                FilterInput::Node(j) => results[usize::from(j)].clone().unwrap_or_else(|| Img::transparent(source.data.len())),
            };
            match space {
                Some(linear) => img.in_space(linear),
                None => img,
            }
        };
        let mut out = evaluate(node, &mut get, width, height, source.data.len());
        if let Some(region) = node.region {
            out.clip(width, height, region);
        }
        results.push(Some(out));
        for j in 0..i {
            if last_use[j] <= i {
                results[j] = None;
            }
        }
    }

    let result = results.pop().flatten().unwrap_or_else(|| Img::transparent(source.data.len())).in_space(false);
    for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(result.data.chunks_exact(4)) {
        let a = (src[3] * 255.0).round().clamp(0.0, 255.0) as u8;
        for k in 0..3 {
            dst[k] = ((src[k] * 255.0).round().clamp(0.0, 255.0) as u8).min(a);
        }
        dst[3] = a;
    }
}

fn evaluate(node: &FilterNode, get: &mut impl FnMut(FilterInput, Option<bool>) -> Img, width: usize, height: usize, len: usize) -> Img {
    let linear = node.linear;
    match &node.op {
        FilterOp::Flood { color } => {
            let a = f32::from(color.a) / 255.0;
            let px = [f32::from(color.r) / 255.0 * a, f32::from(color.g) / 255.0 * a, f32::from(color.b) / 255.0 * a, a];
            let mut img = Img::transparent(len);
            img.data.par_chunks_exact_mut(4).for_each(|d| d.copy_from_slice(&px));
            img
        }
        FilterOp::Composite { a, b, op } => {
            let (ia, ib) = (get(*a, Some(linear)), get(*b, Some(linear)));
            composite(ia, &ib, *op)
        }
        FilterOp::Merge { inputs } => {
            let mut acc = Img::transparent(len);
            acc.linear = linear;
            for input in inputs {
                let top = get(*input, Some(linear));
                acc.data.par_chunks_exact_mut(4).zip(top.data.par_chunks_exact(4)).for_each(|(d, s)| {
                    let k = 1.0 - s[3];
                    for c in 0..4 {
                        d[c] = s[c] + d[c] * k;
                    }
                });
            }
            acc
        }
        FilterOp::Blend { a, b, mode } => {
            let (top, mut base) = (get(*a, Some(linear)), get(*b, Some(linear)));
            blend(&mut base, &top, *mode);
            base
        }
        FilterOp::Offset { input, dx, dy } => {
            // The shift is in whole pixels, cut toward zero, and keeps the space of the input.
            offset(get(*input, None), width, height, *dx as i32, *dy as i32)
        }
        FilterOp::Blur { input, rx, ry } => {
            let mut img = get(*input, Some(linear));
            blur(&mut img, width, height, *rx as f32, *ry as f32);
            img
        }
        FilterOp::ColorMatrix { input, values } => {
            let mut img = get(*input, Some(linear));
            let m: [f32; 20] = values.map(|v| v as f32);
            img.map_straight(|[r, g, b, a]| {
                let row = |k: usize| m[k] * r + m[k + 1] * g + m[k + 2] * b + m[k + 3] * a + m[k + 4];
                [row(0), row(5), row(10), row(15)]
            });
            img
        }
        FilterOp::ComponentTransfer { input, funcs } => {
            let mut img = get(*input, Some(linear));
            img.map_straight(|[r, g, b, a]| {
                let f = |func: &TransferFn, c: f32| func.apply(f64::from(c)) as f32;
                [f(&funcs[0], r), f(&funcs[1], g), f(&funcs[2], b), f(&funcs[3], a)]
            });
            img
        }
    }
}

fn composite(a: Img, b: &Img, op: CompositeOp) -> Img {
    let mut out = a;
    out.data.par_chunks_exact_mut(4).zip(b.data.par_chunks_exact(4)).for_each(|(pa, pb)| {
        let (aa, ba) = (pa[3], pb[3]);
        let src = [pa[0], pa[1], pa[2], pa[3]];
        for c in 0..4 {
            let (x, y) = (src[c], pb[c]);
            pa[c] = match op {
                CompositeOp::Over => x + y * (1.0 - aa),
                CompositeOp::In => x * ba,
                CompositeOp::Out => x * (1.0 - ba),
                CompositeOp::Atop => x * ba + y * (1.0 - aa),
                CompositeOp::Xor => x * (1.0 - ba) + y * (1.0 - aa),
                CompositeOp::Arithmetic { k1, k2, k3, k4 } => (k1 as f32 * x * y + k2 as f32 * x + k3 as f32 * y + k4 as f32).clamp(0.0, 1.0),
            };
        }
        if matches!(op, CompositeOp::Arithmetic { .. }) {
            // Premultiplied color cannot exceed alpha.
            let a = pa[3];
            for v in pa.iter_mut().take(3) {
                *v = v.min(a);
            }
        }
    });
    out
}

/// Draws `top` over `base` with `mode`, on straight channels.
fn blend(base: &mut Img, top: &Img, mode: BlendMode) {
    base.data.par_chunks_exact_mut(4).zip(top.data.par_chunks_exact(4)).for_each(|(d, s)| {
        let straight = |p: &[f32]| if p[3] > 0.0 { (p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]) } else { (0.0, 0.0, 0.0, 0.0) };
        let (r, g, b, a) = msx_render_core::composite(mode, straight(d), straight(s));
        d[0] = r.clamp(0.0, 1.0) * a;
        d[1] = g.clamp(0.0, 1.0) * a;
        d[2] = b.clamp(0.0, 1.0) * a;
        d[3] = a.clamp(0.0, 1.0);
    });
}

fn offset(src: Img, width: usize, height: usize, dx: i32, dy: i32) -> Img {
    if dx == 0 && dy == 0 {
        return src;
    }
    let mut out = Img { data: vec![0.0; src.data.len()], linear: src.linear };
    for y in 0..height as i32 {
        let ty = y + dy;
        if ty < 0 || ty >= height as i32 {
            continue;
        }
        for x in 0..width as i32 {
            let tx = x + dx;
            if tx < 0 || tx >= width as i32 {
                continue;
            }
            let (s, d) = ((y as usize * width + x as usize) * 4, (ty as usize * width + tx as usize) * 4);
            out.data[d..d + 4].copy_from_slice(&src.data[s..s + 4]);
        }
    }
    out
}

/// A Gaussian blur with a deviation per axis, as `gaussian_blur_xy` does it.
fn blur(img: &mut Img, width: usize, height: usize, sigma_x: f32, sigma_y: f32) {
    let blurs = |s: f32| !s.is_nan() && s > 0.0;
    if !blurs(sigma_x) && !blurs(sigma_y) {
        return;
    }
    if blurs(sigma_x) {
        blur_rows(&mut img.data, width, sigma_x);
    }
    let mut turned = transpose(&img.data, width, height);
    if blurs(sigma_y) {
        blur_rows(&mut turned, height, sigma_y);
    }
    img.data = transpose(&turned, height, width);
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Color, FilterNode};

    fn pm(w: u32, h: u32) -> Pixmap {
        Pixmap::new(w, h).unwrap()
    }

    fn set(p: &mut Pixmap, x: usize, y: usize, rgba: [u8; 4]) {
        let w = p.width() as usize;
        p.data_mut()[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&rgba);
    }

    fn at(p: &Pixmap, x: usize, y: usize) -> [u8; 4] {
        let w = p.width() as usize;
        p.data()[(y * w + x) * 4..(y * w + x) * 4 + 4].try_into().unwrap()
    }

    fn near(a: [u8; 4], b: [u8; 4], tol: i32) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (*x as i32 - *y as i32).abs() <= tol)
    }

    fn node(op: FilterOp) -> FilterNode {
        FilterNode::new(op)
    }

    use FilterInput::{Node, SourceAlpha, SourceGraphic};

    /// A 6 by 1 strip: opaque red at x = 1 and x = 2, half transparent blue at 4.
    fn strip() -> Pixmap {
        let mut p = pm(6, 1);
        set(&mut p, 1, 0, [255, 0, 0, 255]);
        set(&mut p, 2, 0, [255, 0, 0, 255]);
        set(&mut p, 4, 0, [0, 0, 128, 128]);
        p
    }

    #[test]
    fn an_empty_graph_and_an_invalid_graph_leave_the_buffer_alone() {
        let mut p = strip();
        let before = p.data().to_vec();
        apply_graph(&mut p, &FilterGraph::default());
        assert_eq!(p.data(), before.as_slice());
        let invalid = FilterGraph { nodes: vec![node(FilterOp::Offset { input: Node(3), dx: 1.0, dy: 0.0 })] };
        apply_graph(&mut p, &invalid);
        assert_eq!(p.data(), before.as_slice());
    }

    #[test]
    fn flood_composite_in_tints_the_silhouette() {
        // The classic recolor: a green flood kept where the source has alpha.
        let g = FilterGraph {
            nodes: vec![
                node(FilterOp::Flood { color: Color::rgb(0, 255, 0) }),
                node(FilterOp::Composite { a: Node(0), b: SourceGraphic, op: CompositeOp::In }),
            ],
        };
        let mut p = strip();
        apply_graph(&mut p, &g);
        assert_eq!(at(&p, 1, 0), [0, 255, 0, 255]);
        assert_eq!(at(&p, 0, 0), [0, 0, 0, 0], "nothing where the source is empty");
        assert!(near(at(&p, 4, 0), [0, 128, 0, 128], 1), "the half transparent pixel keeps its alpha: {:?}", at(&p, 4, 0));
    }

    #[test]
    fn the_porter_duff_operators() {
        // a = flood red (alpha 1), b = the strip, so the operators differ where b is empty or partial.
        let run = |op| {
            let g = FilterGraph { nodes: vec![node(FilterOp::Flood { color: Color::rgb(255, 0, 0) }), node(FilterOp::Composite { a: Node(0), b: SourceGraphic, op })] };
            let mut p = strip();
            apply_graph(&mut p, &g);
            p
        };
        assert_eq!(at(&run(CompositeOp::Over), 0, 0), [255, 0, 0, 255]);
        assert_eq!(at(&run(CompositeOp::Out), 1, 0), [0, 0, 0, 0], "out removes what is inside b");
        assert_eq!(at(&run(CompositeOp::Out), 0, 0), [255, 0, 0, 255], "and keeps the rest");
        assert_eq!(at(&run(CompositeOp::Atop), 0, 0), [0, 0, 0, 0], "atop shows nothing outside b");
        assert_eq!(at(&run(CompositeOp::Atop), 1, 0), [255, 0, 0, 255]);
        assert_eq!(at(&run(CompositeOp::Xor), 1, 0), [0, 0, 0, 0], "xor removes the overlap");
        assert_eq!(at(&run(CompositeOp::Xor), 0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn arithmetic_composite_follows_its_formula_and_keeps_color_under_alpha() {
        // result = 0.5 * a + 0.5 * b with a = source, b = source alpha: the color
        // is halved and the alpha stays.
        let g = FilterGraph { nodes: vec![node(FilterOp::Composite { a: SourceGraphic, b: SourceAlpha, op: CompositeOp::Arithmetic { k1: 0.0, k2: 0.5, k3: 0.5, k4: 0.0 } })] };
        let mut p = pm(1, 1);
        set(&mut p, 0, 0, [200, 100, 0, 255]);
        apply_graph(&mut p, &g);
        assert!(near(at(&p, 0, 0), [100, 50, 0, 255], 1), "{:?}", at(&p, 0, 0));
        // A constant makes color out of nothing, but never above the alpha.
        let k4 = FilterGraph { nodes: vec![node(FilterOp::Composite { a: SourceGraphic, b: SourceGraphic, op: CompositeOp::Arithmetic { k1: 0.0, k2: 0.0, k3: 0.0, k4: 0.5 } })] };
        let mut q = pm(1, 1);
        apply_graph(&mut q, &k4);
        let px = at(&q, 0, 0);
        assert!(near(px, [128, 128, 128, 128], 1), "{px:?}");
    }

    #[test]
    fn merge_paints_the_first_input_at_the_bottom() {
        let g = FilterGraph {
            nodes: vec![
                node(FilterOp::Flood { color: Color::rgb(0, 0, 255) }),
                node(FilterOp::Merge { inputs: vec![Node(0), SourceGraphic] }),
            ],
        };
        let mut p = strip();
        apply_graph(&mut p, &g);
        assert_eq!(at(&p, 1, 0), [255, 0, 0, 255], "the source is on top");
        assert_eq!(at(&p, 0, 0), [0, 0, 255, 255], "the flood shows through");
        let reversed = FilterGraph {
            nodes: vec![node(FilterOp::Flood { color: Color::rgb(0, 0, 255) }), node(FilterOp::Merge { inputs: vec![SourceGraphic, Node(0)] })],
        };
        let mut q = strip();
        apply_graph(&mut q, &reversed);
        assert_eq!(at(&q, 1, 0), [0, 0, 255, 255], "now the flood is on top");
    }

    #[test]
    fn blend_multiply_draws_the_first_input_over_the_second() {
        let g = FilterGraph {
            nodes: vec![
                node(FilterOp::Flood { color: Color::rgb(128, 128, 128) }),
                node(FilterOp::Blend { a: SourceGraphic, b: Node(0), mode: BlendMode::Multiply }),
            ],
        };
        let mut p = pm(2, 1);
        set(&mut p, 0, 0, [255, 0, 0, 255]);
        apply_graph(&mut p, &g);
        assert!(near(at(&p, 0, 0), [128, 0, 0, 255], 1), "{:?}", at(&p, 0, 0));
        assert!(near(at(&p, 1, 0), [128, 128, 128, 255], 1), "an empty source leaves the backdrop: {:?}", at(&p, 1, 0));
    }

    #[test]
    fn offset_moves_whole_pixels_cut_toward_zero_and_drops_what_leaves() {
        let g = |dx, dy| FilterGraph { nodes: vec![node(FilterOp::Offset { input: SourceGraphic, dx, dy })] };
        let mut p = strip();
        apply_graph(&mut p, &g(1.9, 0.0));
        assert_eq!(at(&p, 2, 0), [255, 0, 0, 255], "1.9 moves one pixel");
        assert_eq!(at(&p, 1, 0), [0, 0, 0, 0]);
        let mut q = strip();
        apply_graph(&mut q, &g(-1.9, 0.0));
        assert_eq!(at(&q, 0, 0), [255, 0, 0, 255], "-1.9 moves one pixel left");
        let mut off = strip();
        apply_graph(&mut off, &g(20.0, 0.0));
        assert!(off.data().iter().all(|b| *b == 0), "everything left the buffer");
    }

    #[test]
    fn blur_in_a_graph_spreads_along_the_axis_with_an_amount() {
        let g = FilterGraph { nodes: vec![node(FilterOp::Blur { input: SourceGraphic, rx: 1.5, ry: 0.0 })] };
        let mut p = pm(11, 5);
        set(&mut p, 5, 2, [255, 255, 255, 255]);
        apply_graph(&mut p, &g);
        assert!(at(&p, 3, 2)[3] > 0 && at(&p, 7, 2)[3] > 0);
        assert_eq!(at(&p, 5, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn color_matrix_and_component_transfer_work_on_straight_channels() {
        let mut swap = [0.0; 20];
        swap[2] = 1.0; // red from blue
        swap[6] = 1.0; // green from green
        swap[10] = 1.0; // blue from red
        swap[18] = 1.0; // alpha stays
        let g = FilterGraph { nodes: vec![node(FilterOp::ColorMatrix { input: SourceGraphic, values: swap })] };
        let mut p = pm(1, 1);
        set(&mut p, 0, 0, [100, 50, 25, 128]);
        apply_graph(&mut p, &g);
        assert!(near(at(&p, 0, 0), [25, 50, 100, 128], 1), "{:?}", at(&p, 0, 0));

        // Invert the red channel with a linear function, and posterize green.
        let ct = FilterGraph {
            nodes: vec![node(FilterOp::ComponentTransfer {
                input: SourceGraphic,
                funcs: [TransferFn::Linear { slope: -1.0, intercept: 1.0 }, TransferFn::Discrete(vec![0.0, 1.0]), TransferFn::Identity, TransferFn::Identity],
            })],
        };
        let mut q = pm(2, 1);
        set(&mut q, 0, 0, [255, 100, 10, 255]);
        set(&mut q, 1, 0, [0, 200, 10, 255]);
        apply_graph(&mut q, &ct);
        assert!(near(at(&q, 0, 0), [0, 0, 10, 255], 1), "{:?}", at(&q, 0, 0));
        assert!(near(at(&q, 1, 0), [255, 255, 10, 255], 1), "{:?}", at(&q, 1, 0));
    }

    #[test]
    fn a_region_clears_everything_outside_it() {
        let mut flood = node(FilterOp::Flood { color: Color::rgb(255, 0, 0) });
        flood.region = Some(FilterRegion { x: 2.0, y: 0.0, width: 2.0, height: 1.0 });
        let mut p = pm(6, 1);
        apply_graph(&mut p, &FilterGraph { nodes: vec![flood] });
        assert_eq!(at(&p, 1, 0), [0, 0, 0, 0]);
        assert_eq!(at(&p, 2, 0), [255, 0, 0, 255]);
        assert_eq!(at(&p, 3, 0), [255, 0, 0, 255]);
        assert_eq!(at(&p, 4, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn linear_light_changes_the_result_of_a_blend_of_two_grays() {
        // Half of an opaque white over half gray in an arithmetic average:
        // sRGB gives 0.5 * 255 + ..., linear light gives a brighter value.
        let average = |linear: bool| {
            let mut n = node(FilterOp::Composite { a: SourceGraphic, b: Node(0), op: CompositeOp::Arithmetic { k1: 0.0, k2: 0.5, k3: 0.5, k4: 0.0 } });
            n.linear = linear;
            let g = FilterGraph { nodes: vec![node(FilterOp::Flood { color: Color::BLACK }), n] };
            let mut p = pm(1, 1);
            set(&mut p, 0, 0, [255, 255, 255, 255]);
            apply_graph(&mut p, &g);
            at(&p, 0, 0)[0] as i32
        };
        let (s, l) = (average(false), average(true));
        assert!((s - 128).abs() <= 1, "sRGB: {s}");
        assert!((l - 188).abs() <= 2, "linear light: {l}");
    }

    #[test]
    fn source_alpha_has_black_color() {
        let g = FilterGraph { nodes: vec![node(FilterOp::Offset { input: SourceAlpha, dx: 0.0, dy: 0.0 })] };
        let mut p = strip();
        apply_graph(&mut p, &g);
        assert_eq!(at(&p, 1, 0), [0, 0, 0, 255]);
        assert_eq!(at(&p, 4, 0)[3], 128);
    }
}
