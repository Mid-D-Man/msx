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

use msx_ast::{BlendMode, CompositeOp, ConvolveEdge, FilterGraph, FilterInput, FilterNode, FilterOp, FilterRegion, TransferFn};
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
        let (x0, y0, x1, y1) = device_rect(width, height, Some(region));
        self.data.par_chunks_mut(width * 4).enumerate().for_each(|(y, row)| {
            for x in 0..width {
                if y < y0 || y >= y1 || x < x0 || x >= x1 {
                    row[x * 4..x * 4 + 4].fill(0.0);
                }
            }
        });
    }
}

/// The pixel rectangle `(x0, y0, x1, y1)` of a region on a `width` by `height`
/// buffer, cut to the buffer. No region is the whole buffer.
fn device_rect(width: usize, height: usize, region: Option<FilterRegion>) -> (usize, usize, usize, usize) {
    let Some(region) = region else { return (0, 0, width, height) };
    let x0 = region.x.floor().clamp(0.0, width as f64) as usize;
    let y0 = region.y.floor().clamp(0.0, height as f64) as usize;
    let x1 = (region.x.floor() + region.width.ceil()).clamp(0.0, width as f64) as usize;
    let y1 = (region.y.floor() + region.height.ceil()).clamp(0.0, height as f64) as usize;
    (x0, y0, x1.max(x0), y1.max(y0))
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

    let regions: Vec<Option<FilterRegion>> = graph.nodes.iter().map(|n| n.region).collect();
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
        let mut out = evaluate(node, &mut get, &regions, width, height, source.data.len());
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

fn evaluate(node: &FilterNode, get: &mut impl FnMut(FilterInput, Option<bool>) -> Img, regions: &[Option<FilterRegion>], width: usize, height: usize, len: usize) -> Img {
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
        FilterOp::Morphology { input, dilate, rx, ry } => {
            let img = get(*input, Some(linear));
            morphology(img, device_rect(width, height, node.region), width, *dilate, *rx, *ry)
        }
        FilterOp::Tile { input } => {
            // The tile is the region of the input node. The input is read as it is.
            let tile = match input {
                FilterInput::Node(j) => device_rect(width, height, regions.get(usize::from(*j)).copied().flatten()),
                _ => (0, 0, width, height),
            };
            tile_image(get(*input, None), tile, width)
        }
        FilterOp::ConvolveMatrix { input, columns, rows, kernel, divisor, bias, target_x, target_y, edge, preserve_alpha } => {
            let img = get(*input, Some(linear));
            let spec = Convolve {
                columns: usize::from(*columns),
                rows: usize::from(*rows),
                kernel,
                divisor: *divisor as f32,
                bias: *bias as f32,
                target: (usize::from(*target_x), usize::from(*target_y)),
                edge: *edge,
                preserve_alpha: *preserve_alpha,
            };
            convolve(img, device_rect(width, height, node.region), width, &spec)
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

/// Copies the rectangle `(x0, y0, x1, y1)` of `img` into an image of its own.
fn crop(img: &Img, width: usize, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> Vec<f32> {
    let mut out = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        out.extend_from_slice(&img.data[(y * width + x0) * 4..(y * width + x1) * 4]);
    }
    out
}

/// A copy of `img` that is transparent except for `cells` in the rectangle
/// `(x0, y0, x1, y1)`, which takes the place of what was there.
fn paste(img: &Img, width: usize, (x0, y0, x1, y1): (usize, usize, usize, usize), cells: &[f32]) -> Img {
    let mut out = Img { data: vec![0.0; img.data.len()], linear: img.linear };
    let w = x1 - x0;
    for y in y0..y1 {
        let from = (y - y0) * w * 4;
        out.data[(y * width + x0) * 4..(y * width + x1) * 4].copy_from_slice(&cells[from..from + w * 4]);
    }
    out
}

/// One line of the min or max over a window of `len` cells that starts `back`
/// cells before the cell, cut at both ends of the line. Every value of the
/// line is entered once, so the cost does not grow with the window.
fn extreme_line(line: &[f32], out: &mut [f32], len: usize, back: usize, dilate: bool) {
    let n = line.len();
    let better = |a: f32, b: f32| if dilate { a >= b } else { a <= b };
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut next = 0;
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        let right = (i + len).saturating_sub(back + 1).min(n - 1);
        while next <= right {
            while queue.back().is_some_and(|&j| better(line[next], line[j])) {
                queue.pop_back();
            }
            queue.push_back(next);
            next += 1;
        }
        let left = i.saturating_sub(back);
        while queue.front().is_some_and(|&j| j < left) {
            queue.pop_front();
        }
        *slot = line[*queue.front().expect("the window holds the cell or a neighbor")];
    }
}

/// The min (erode) or max (dilate) of each premultiplied channel over a
/// window of `2 * ceil(r)` cells per axis, from `ceil(r)` before the pixel to
/// `ceil(r) - 1` after it, as resvg does it. A window that reaches past the
/// region covers the whole line from every pixel. The work is done on the
/// pixels of `rect` alone, so what is outside counts for nothing, and nothing
/// is left outside it. An axis with a radius of zero or less is left alone.
fn morphology(img: Img, rect: (usize, usize, usize, usize), width: usize, dilate: bool, rx: f64, ry: f64) -> Img {
    let (x0, y0, x1, y1) = rect;
    let (w, h) = (x1 - x0, y1 - y0);
    let axis = |r: f64, n: usize| -> Option<(usize, usize)> {
        if r.is_nan() || r <= 0.0 || n == 0 {
            return None;
        }
        // Past the line length every pixel sees the whole line, so more is not needed.
        let back = r.ceil().min(n as f64) as usize;
        Some((back * 2, back))
    };
    let (ax, ay) = (axis(rx, w), axis(ry, h));
    if w == 0 || h == 0 || (ax.is_none() && ay.is_none()) {
        return img;
    }
    let mut cells = crop(&img, width, rect);
    let pass = |cells: &mut [f32], line_len: usize, spec: (usize, usize)| {
        cells.par_chunks_mut(line_len * 4).for_each(|row| {
            let mut line = vec![0.0f32; line_len];
            let mut out = vec![0.0f32; line_len];
            for c in 0..4 {
                for (i, v) in line.iter_mut().enumerate() {
                    *v = row[i * 4 + c];
                }
                extreme_line(&line, &mut out, spec.0, spec.1, dilate);
                for (i, v) in out.iter().enumerate() {
                    row[i * 4 + c] = *v;
                }
            }
        });
    };
    if let Some(spec) = ax {
        pass(&mut cells, w, spec);
    }
    if let Some(spec) = ay {
        let mut turned = transpose(&cells, w, h);
        pass(&mut turned, h, spec);
        cells = transpose(&turned, h, w);
    }
    paste(&img, width, rect, &cells)
}

/// Repeats the rectangle `tile` of `img` over the whole buffer, with the
/// tile where it is. A tile with no area gives transparent black.
fn tile_image(img: Img, tile: (usize, usize, usize, usize), width: usize) -> Img {
    let (x0, y0, x1, y1) = tile;
    let (tw, th) = (x1 - x0, y1 - y0);
    let mut out = Img { data: vec![0.0; img.data.len()], linear: img.linear };
    if tw == 0 || th == 0 {
        return out;
    }
    out.data.par_chunks_mut(width * 4).enumerate().for_each(|(y, row)| {
        let sy = y0 + (y as i64 - y0 as i64).rem_euclid(th as i64) as usize;
        for x in 0..width {
            let sx = x0 + (x as i64 - x0 as i64).rem_euclid(tw as i64) as usize;
            row[x * 4..x * 4 + 4].copy_from_slice(&img.data[(sy * width + sx) * 4..(sy * width + sx) * 4 + 4]);
        }
    });
    out
}

/// What `convolve` needs besides the pixels.
struct Convolve<'a> {
    columns: usize,
    rows: usize,
    kernel: &'a [f64],
    divisor: f32,
    bias: f32,
    target: (usize, usize),
    edge: ConvolveEdge,
    preserve_alpha: bool,
}

/// `feConvolveMatrix` on the pixels of `rect`, with its edge mode deciding
/// what lies beyond them. The kernel is turned half a turn, as in a
/// convolution, so the first kernel value meets the last neighbor. With
/// `preserve_alpha` the colors are convolved on straight values and keep the
/// alpha of the pixel.
fn convolve(img: Img, rect: (usize, usize, usize, usize), width: usize, spec: &Convolve) -> Img {
    let (x0, y0, x1, y1) = rect;
    let (w, h) = (x1 - x0, y1 - y0);
    if w == 0 || h == 0 {
        return img;
    }
    let mut src = crop(&img, width, rect);
    if spec.preserve_alpha {
        for px in src.chunks_exact_mut(4) {
            let a = px[3];
            if a > 0.0 {
                for c in &mut px[..3] {
                    *c = (*c / a).clamp(0.0, 1.0);
                }
            }
        }
    }
    let (columns, rows) = (spec.columns as i64, spec.rows as i64);
    let (tx, ty) = (spec.target.0 as i64, spec.target.1 as i64);
    let mut out = vec![0.0f32; src.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut sum = [0.0f32; 4];
            for oy in 0..rows {
                for ox in 0..columns {
                    let (mut px, mut py) = (x as i64 - tx + ox, y as i64 - ty + oy);
                    match spec.edge {
                        ConvolveEdge::None => {
                            if px < 0 || px >= w as i64 || py < 0 || py >= h as i64 {
                                continue;
                            }
                        }
                        ConvolveEdge::Duplicate => {
                            px = px.clamp(0, w as i64 - 1);
                            py = py.clamp(0, h as i64 - 1);
                        }
                        ConvolveEdge::Wrap => {
                            px = px.rem_euclid(w as i64);
                            py = py.rem_euclid(h as i64);
                        }
                    }
                    let k = spec.kernel[((rows - 1 - oy) * columns + (columns - 1 - ox)) as usize] as f32;
                    let p = &src[(py as usize * w + px as usize) * 4..][..4];
                    for c in 0..4 {
                        sum[c] += p[c] * k;
                    }
                }
            }
            let raw = if spec.preserve_alpha { src[(y * w + x) * 4 + 3] } else { sum[3] / spec.divisor + spec.bias };
            let a = raw.clamp(0.0, 1.0);
            for c in 0..3 {
                let v = sum[c] / spec.divisor + spec.bias * raw;
                row[x * 4 + c] = if spec.preserve_alpha { v.clamp(0.0, 1.0) * a } else { v.clamp(0.0, a) };
            }
            row[x * 4 + 3] = a;
        }
    });
    paste(&img, width, rect, &out)
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

    fn opaque_red_at(width: u32, height: u32, cells: &[(usize, usize)]) -> Pixmap {
        let mut p = pm(width, height);
        for &(x, y) in cells {
            set(&mut p, x, y, [255, 0, 0, 255]);
        }
        p
    }

    fn alphas(p: &Pixmap) -> Vec<u8> {
        p.data().chunks_exact(4).map(|px| px[3]).collect()
    }

    fn reds(p: &Pixmap) -> Vec<u8> {
        p.data().chunks_exact(4).map(|px| px[0]).collect()
    }

    fn morph(dilate: bool, rx: f64, ry: f64, region: Option<FilterRegion>) -> FilterGraph {
        FilterGraph { nodes: vec![FilterNode { op: FilterOp::Morphology { input: SourceGraphic, dilate, rx, ry }, region, linear: false }] }
    }

    #[test]
    fn erode_and_dilate_use_a_window_of_twice_the_radius_that_leans_back() {
        // Opaque at x = 2 to 7 of 10. A radius of 1 is a window of two cells, the pixel and the one before.
        let cells: Vec<(usize, usize)> = (2..8).map(|x| (x, 0)).collect();
        let mut p = opaque_red_at(10, 1, &cells);
        apply_graph(&mut p, &morph(false, 1.0, 0.0, None));
        assert_eq!(alphas(&p), [0, 0, 0, 255, 255, 255, 255, 255, 0, 0], "erode");
        let mut q = opaque_red_at(10, 1, &cells);
        apply_graph(&mut q, &morph(true, 1.0, 0.0, None));
        assert_eq!(alphas(&q), [0, 0, 255, 255, 255, 255, 255, 255, 255, 0], "dilate");
        // A fraction rounds up, and a window over the whole row erases a shorter run.
        let mut r = opaque_red_at(10, 1, &cells);
        apply_graph(&mut r, &morph(false, 0.25, 0.0, None));
        assert_eq!(alphas(&r), [0, 0, 0, 255, 255, 255, 255, 255, 0, 0], "0.25 is a radius of 1");
        let mut s = opaque_red_at(10, 1, &cells);
        apply_graph(&mut s, &morph(false, 9999.0, 0.0, None));
        assert!(alphas(&s).iter().all(|&a| a == 0), "{:?}", alphas(&s));
    }

    #[test]
    fn morphology_works_on_each_axis_alone_and_zero_leaves_the_axis() {
        // A column of 1 by 8, opaque at y = 2 to 5.
        let cells: Vec<(usize, usize)> = (2..6).map(|y| (0, y)).collect();
        let mut p = opaque_red_at(1, 8, &cells);
        apply_graph(&mut p, &morph(true, 0.0, 1.0, None));
        assert_eq!(alphas(&p), [0, 0, 255, 255, 255, 255, 255, 0]);
        let mut q = opaque_red_at(1, 8, &cells);
        apply_graph(&mut q, &morph(false, 5.0, 0.0, None));
        assert_eq!(alphas(&q), alphas(&opaque_red_at(1, 8, &cells)), "no vertical radius changes nothing in a column");
        let mut z = opaque_red_at(1, 8, &cells);
        apply_graph(&mut z, &morph(true, 0.0, 0.0, None));
        assert_eq!(alphas(&z), alphas(&opaque_red_at(1, 8, &cells)));
    }

    #[test]
    fn morphology_sees_nothing_outside_its_region() {
        // Opaque at 0 to 5, region 2 to 7. Erode keeps what is inside, as the edge pixels do not count for the outside.
        let cells: Vec<(usize, usize)> = (0..6).map(|x| (x, 0)).collect();
        let region = Some(FilterRegion { x: 2.0, y: 0.0, width: 6.0, height: 1.0 });
        let mut p = opaque_red_at(10, 1, &cells);
        apply_graph(&mut p, &morph(false, 1.0, 0.0, region));
        assert_eq!(alphas(&p), [0, 0, 255, 255, 255, 255, 0, 0, 0, 0]);
        // A dilation at least as wide as the region fills it, and nothing spills out of it.
        let one = vec![(3, 0)];
        let mut q = opaque_red_at(10, 1, &one);
        apply_graph(&mut q, &morph(true, 50.0, 0.0, region));
        assert_eq!(alphas(&q), [0, 0, 255, 255, 255, 255, 255, 255, 0, 0]);
        // A smaller one leans back: the window of a radius of 2 reaches 2 before and 1 after.
        let mut r = opaque_red_at(10, 1, &one);
        apply_graph(&mut r, &morph(true, 2.0, 0.0, region));
        assert_eq!(alphas(&r), [0, 0, 255, 255, 255, 255, 0, 0, 0, 0]);
    }

    fn tile_graph(region: Option<FilterRegion>) -> FilterGraph {
        FilterGraph {
            nodes: vec![
                FilterNode { op: FilterOp::Offset { input: SourceGraphic, dx: 0.0, dy: 0.0 }, region, linear: false },
                FilterNode::new(FilterOp::Tile { input: Node(0) }),
            ],
        }
    }

    #[test]
    fn tile_repeats_the_region_of_its_input_over_the_layer() {
        let at = |region: FilterRegion, red: usize| {
            let mut p = opaque_red_at(9, 1, &[(red, 0)]);
            apply_graph(&mut p, &tile_graph(Some(region)));
            alphas(&p).iter().enumerate().filter(|(_, &a)| a == 255).map(|(x, _)| x).collect::<Vec<_>>()
        };
        // A tile of two cells at the origin: [transparent, red].
        assert_eq!(at(FilterRegion { x: 0.0, y: 0.0, width: 2.0, height: 1.0 }, 1), [1, 3, 5, 7]);
        // A tile of three cells that starts at 2 and reaches back before it.
        assert_eq!(at(FilterRegion { x: 2.0, y: 0.0, width: 3.0, height: 1.0 }, 3), [0, 3, 6]);
        // What is outside the tile region is not in the tile.
        assert!(at(FilterRegion { x: 0.0, y: 0.0, width: 2.0, height: 1.0 }, 5).is_empty());
    }

    #[test]
    fn tile_of_the_whole_layer_is_the_layer_and_an_empty_region_is_nothing() {
        let mut p = opaque_red_at(4, 1, &[(1, 0)]);
        apply_graph(&mut p, &FilterGraph { nodes: vec![node(FilterOp::Tile { input: SourceGraphic })] });
        assert_eq!(alphas(&p), [0, 255, 0, 0]);
        let mut q = opaque_red_at(4, 1, &[(1, 0)]);
        apply_graph(&mut q, &tile_graph(Some(FilterRegion { x: 9.0, y: 0.0, width: 2.0, height: 1.0 })));
        assert!(alphas(&q).iter().all(|&a| a == 0));
        // The tile node's own region cuts the result.
        let mut r = opaque_red_at(8, 1, &[(1, 0)]);
        let mut g = tile_graph(Some(FilterRegion { x: 0.0, y: 0.0, width: 2.0, height: 1.0 }));
        g.nodes[1].region = Some(FilterRegion { x: 0.0, y: 0.0, width: 4.0, height: 1.0 });
        apply_graph(&mut r, &g);
        assert_eq!(alphas(&r), [0, 255, 0, 255, 0, 0, 0, 0]);
    }

    fn conv(columns: u16, rows: u16, kernel: &[f64], divisor: f64, edge: ConvolveEdge, preserve_alpha: bool, region: Option<FilterRegion>) -> FilterGraph {
        FilterGraph {
            nodes: vec![FilterNode {
                op: FilterOp::ConvolveMatrix {
                    input: SourceGraphic,
                    columns,
                    rows,
                    kernel: kernel.to_vec(),
                    divisor,
                    bias: 0.0,
                    target_x: columns / 2,
                    target_y: rows / 2,
                    edge,
                    preserve_alpha,
                },
                region,
                linear: false,
            }],
        }
    }

    /// An opaque strip where only the red channel varies.
    fn red_strip(values: &[u8]) -> Pixmap {
        let mut p = pm(values.len() as u32, 1);
        for (x, v) in values.iter().enumerate() {
            set(&mut p, x, 0, [*v, 0, 0, 255]);
        }
        p
    }

    #[test]
    fn the_identity_kernel_changes_nothing_and_the_kernel_is_turned_half_a_turn() {
        let mut p = red_strip(&[10, 200, 30, 40]);
        apply_graph(&mut p, &conv(3, 3, &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 1.0, ConvolveEdge::Duplicate, false, None));
        assert_eq!(reds(&p), [10, 200, 30, 40]);
        // The first kernel value meets the neighbor on the right.
        let mut q = red_strip(&[0, 0, 0, 255, 0, 0]);
        apply_graph(&mut q, &conv(3, 1, &[1.0, 0.0, 0.0], 1.0, ConvolveEdge::None, false, None));
        assert_eq!(reds(&q), [0, 0, 255, 0, 0, 0]);
    }

    #[test]
    fn the_edge_mode_decides_what_lies_beyond_the_region() {
        let kernel = [1.0, 1.0, 1.0];
        let run = |edge| {
            let mut p = red_strip(&[255, 0, 0, 0, 0]);
            apply_graph(&mut p, &conv(3, 1, &kernel, 3.0, edge, false, None));
            (reds(&p)[0], alphas(&p)[0])
        };
        assert_eq!(run(ConvolveEdge::Duplicate), (170, 255), "the first pixel again");
        assert_eq!(run(ConvolveEdge::None), (85, 170), "transparent black, which also thins the alpha");
        assert_eq!(run(ConvolveEdge::Wrap), (85, 255), "the last pixel, which is dark");
    }

    #[test]
    fn a_wrap_turns_around_inside_the_region_and_nothing_is_left_outside_it() {
        let region = Some(FilterRegion { x: 2.0, y: 0.0, width: 4.0, height: 1.0 });
        let mut p = red_strip(&[0, 0, 0, 0, 0, 255, 0, 0]);
        // kernel [0, 0, 1] meets the left neighbor, so the picture moves right by one.
        apply_graph(&mut p, &conv(3, 1, &[0.0, 0.0, 1.0], 1.0, ConvolveEdge::Wrap, false, region));
        assert_eq!(reds(&p), [0, 0, 255, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn preserve_alpha_keeps_the_alpha_and_convolves_straight_colors() {
        let mut p = pm(3, 1);
        set(&mut p, 0, 0, [128, 0, 0, 128]);
        set(&mut p, 1, 0, [0, 0, 0, 0]);
        set(&mut p, 2, 0, [128, 0, 0, 128]);
        apply_graph(&mut p, &conv(3, 1, &[0.0, 1.0, 0.0], 1.0, ConvolveEdge::Duplicate, true, None));
        assert!(near(at(&p, 0, 0), [128, 0, 0, 128], 1), "{:?}", at(&p, 0, 0));
        assert_eq!(at(&p, 1, 0)[3], 0, "an empty pixel keeps its alpha");
        // A kernel that sums neighbors keeps the alpha of the pixel and averages the straight color.
        let mut q = pm(3, 1);
        set(&mut q, 0, 0, [255, 0, 0, 255]);
        set(&mut q, 1, 0, [64, 0, 0, 64]);
        set(&mut q, 2, 0, [255, 0, 0, 255]);
        apply_graph(&mut q, &conv(3, 1, &[0.0, 1.0, 1.0], 2.0, ConvolveEdge::Duplicate, true, None));
        assert_eq!(at(&q, 1, 0)[3], 64);
        assert!(at(&q, 1, 0)[0] >= 62 && at(&q, 1, 0)[0] <= 64, "{:?}", at(&q, 1, 0));
    }

    #[test]
    fn a_weak_alpha_pulls_the_color_down_with_it() {
        // Premultiplied color cannot exceed the alpha, so a divisor of 4 on opaque red leaves a quarter.
        let mut p = red_strip(&[255, 255, 255]);
        apply_graph(&mut p, &conv(1, 1, &[1.0], 4.0, ConvolveEdge::Duplicate, false, None));
        assert!(near(at(&p, 1, 0), [64, 0, 0, 64], 1), "{:?}", at(&p, 1, 0));
    }

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
