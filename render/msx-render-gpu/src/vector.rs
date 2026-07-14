// render/msx-render-gpu/src/vector.rs
//! Vector shape tessellation: every SVG-native element becomes triangles
//! via `lyon`, with color baked directly into each vertex and screen
//! position pre-transformed into WebGPU clip space on the CPU side.
//!
//! `tessellate_elements` is the actual workhorse now — `tessellate_scene`
//! is a thin wrapper over it. `layer.rs` calls `tessellate_elements`
//! directly (scoped to just one layer's children) to render that layer
//! into its own isolated buffer.
//!
//! `tessellate_scene_with_shaders` is a separate, `pub(crate)`-only
//! sibling of `tessellate_scene` used exclusively by `lib.rs`'s top-level
//! render path: it additionally pulls out any shape whose *fill*
//! resolves to a `Def::Shader` into its own collection (see
//! `shader.rs::PendingShaderShape`) instead of baking a flat color for
//! it. Kept deliberately separate from the crate's public
//! `tessellate_scene`/`tessellate_elements` (both re-exported from
//! `lib.rs`) rather than changing their signatures — those stay flat
//! geometry in, flat geometry out, so nothing outside this crate that
//! might already depend on that shape breaks, and `layer.rs` keeps
//! calling the plain, unchanged `tessellate_elements` (shader fills
//! inside a `Layer` intentionally still paint flat — see `shader.rs`'s
//! module doc for why that's scoped out for now, not an oversight).

use std::collections::HashMap;

use lyon::math::point;
use lyon::path::path::Builder as LyonPathBuilder;
use lyon::path::Path as LyonPath;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};

use msx_ast::{path::PathCommand, Color, Def, Element, Line, Matrix2D, Paint, Polyline, Rect, Scene, Style, Use};

use crate::shader::PendingShaderShape;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

pub struct VectorGeometry {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

pub(crate) struct Defs<'a> {
    by_id: HashMap<&'a str, &'a Def>,
}
impl<'a> Defs<'a> {
    pub(crate) fn build(defs: &'a [Def]) -> Self {
        Defs { by_id: defs.iter().map(|d| (d.id(), d)).collect() }
    }
    pub(crate) fn get(&self, id: &str) -> Option<&'a Def> {
        self.by_id.get(id).copied()
    }
}

pub(crate) struct ElementIndex<'a> {
    by_id: HashMap<&'a str, &'a Element>,
}
impl<'a> ElementIndex<'a> {
    pub(crate) fn build(elements: &'a [Element]) -> Self {
        let mut by_id = HashMap::new();
        Self::index_into(elements, &mut by_id);
        ElementIndex { by_id }
    }
    fn index_into(elements: &'a [Element], map: &mut HashMap<&'a str, &'a Element>) {
        for el in elements {
            if let Some(id) = el.id() {
                map.insert(id, el);
            }
            match el {
                Element::Group(g) => Self::index_into(&g.children, map),
                Element::Layer(l) => Self::index_into(&l.children, map),
                _ => {}
            }
        }
    }
    pub(crate) fn get(&self, id: &str) -> Option<&'a Element> {
        self.by_id.get(id).copied()
    }
}

pub fn tessellate_scene(scene: &Scene) -> VectorGeometry {
    let defs = Defs::build(&scene.defs);
    let index = ElementIndex::build(&scene.elements);
    let canvas = (scene.canvas.width as f32, scene.canvas.height as f32);
    tessellate_elements_with(&scene.elements, Matrix2D::identity(), canvas, &defs, &index, None)
}

/// `lib.rs`'s shader-aware top-level render path only — see this module's
/// doc comment for why it's kept separate from `tessellate_scene`.
pub(crate) fn tessellate_scene_with_shaders(scene: &Scene) -> (VectorGeometry, Vec<PendingShaderShape>) {
    let defs = Defs::build(&scene.defs);
    let index = ElementIndex::build(&scene.elements);
    let canvas = (scene.canvas.width as f32, scene.canvas.height as f32);
    let mut shader_shapes = Vec::new();
    let geometry = tessellate_elements_with(&scene.elements, Matrix2D::identity(), canvas, &defs, &index, Some(&mut shader_shapes));
    (geometry, shader_shapes)
}

/// Entry point for `layer.rs`: tessellate just one layer's children,
/// with their own local `Defs`/`ElementIndex` (cross-boundary `use`/
/// gradient-ref resolution into the outer scene isn't supported — see
/// `layer.rs`'s module doc).
pub fn tessellate_elements(elements: &[Element], base_transform: Matrix2D, canvas: (f32, f32)) -> VectorGeometry {
    let defs = Defs::build(&[]);
    let index = ElementIndex::build(elements);
    tessellate_elements_with(elements, base_transform, canvas, &defs, &index, None)
}

fn tessellate_elements_with(
    elements: &[Element],
    base_transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    index: &ElementIndex,
    mut shader_shapes: Option<&mut Vec<PendingShaderShape>>,
) -> VectorGeometry {
    let mut buffers: VertexBuffers<Vertex, u32> = VertexBuffers::new();
    for element in elements {
        tessellate_element(element, base_transform, canvas, defs, index, &mut buffers, shader_shapes.as_deref_mut());
    }
    VectorGeometry { vertices: buffers.vertices, indices: buffers.indices }
}

fn tessellate_element(
    element: &Element,
    transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    index: &ElementIndex,
    buffers: &mut VertexBuffers<Vertex, u32>,
    mut shader_shapes: Option<&mut Vec<PendingShaderShape>>,
) {
    match element {
        Element::Rect(r) => {
            let path = rect_path(r);
            fill_and_stroke(&path, &r.style, combine(transform, r.transform.as_ref()), canvas, defs, buffers, shader_shapes);
        }
        Element::Circle(c) => {
            let path = ellipse_path(c.cx, c.cy, c.r, c.r);
            fill_and_stroke(&path, &c.style, combine(transform, c.transform.as_ref()), canvas, defs, buffers, shader_shapes);
        }
        Element::Ellipse(e) => {
            let path = ellipse_path(e.cx, e.cy, e.rx, e.ry);
            fill_and_stroke(&path, &e.style, combine(transform, e.transform.as_ref()), canvas, defs, buffers, shader_shapes);
        }
        Element::Line(l) => stroke_only_line(l, combine(transform, l.transform.as_ref()), canvas, buffers),
        Element::Polyline(p) | Element::Polygon(p) => {
            let path = polyline_path(p);
            fill_and_stroke(&path, &p.style, combine(transform, p.transform.as_ref()), canvas, defs, buffers, shader_shapes);
        }
        Element::Path(p) => {
            let path = msx_path_to_lyon(p);
            fill_and_stroke(&path, &p.style, combine(transform, p.transform.as_ref()), canvas, defs, buffers, shader_shapes);
        }
        Element::Text(_) => {
            // No font shaping/rasterization here either — same gap as
            // msx-render-cpu, same reason (no font backend wired in).
        }
        Element::Group(g) => {
            let combined = combine(transform, g.transform.as_ref());
            for child in &g.children {
                tessellate_element(child, combined, canvas, defs, index, buffers, shader_shapes.as_deref_mut());
            }
        }
        Element::Use(u) => tessellate_use(u, transform, canvas, defs, index, buffers, shader_shapes),
        Element::Sdf(_) | Element::Splat(_) => {
            // sdf.rs / splat.rs handle these in their own passes.
        }
        Element::Layer(_) => {
            // layer.rs handles these via its own isolated render +
            // composite — never flattened into the shared vector pass.
        }
    }
}

fn tessellate_use(
    u: &Use,
    parent: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    index: &ElementIndex,
    buffers: &mut VertexBuffers<Vertex, u32>,
    shader_shapes: Option<&mut Vec<PendingShaderShape>>,
) {
    let id = u.href.strip_prefix('#').unwrap_or(&u.href);
    let Some(target) = index.get(id) else { return };
    let offset = Matrix2D::translate(u.x, u.y);
    let local = u.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    tessellate_element(target, parent.concat(offset.concat(local)), canvas, defs, index, buffers, shader_shapes);
}

fn combine(parent: Matrix2D, local: Option<&msx_ast::Transform>) -> Matrix2D {
    match local {
        None => parent,
        Some(t) => parent.concat(t.to_matrix()),
    }
}

// ── Fill + stroke ────────────────────────────────────────────────────────────

fn fill_and_stroke(
    path: &LyonPath,
    style: &Style,
    transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    buffers: &mut VertexBuffers<Vertex, u32>,
    shader_shapes: Option<&mut Vec<PendingShaderShape>>,
) {
    let opacity = style.opacity.unwrap_or(1.0);

    if let Some(fill) = style.fill.as_ref().filter(|p| !p.is_none()) {
        // A fill that resolves to a real Def::Shader gets pulled out into
        // its own draw, executed for real by shader.rs — but only when a
        // collector was actually provided (the top-level scene path via
        // tessellate_scene_with_shaders). Every other caller
        // (tessellate_scene, tessellate_elements, and therefore
        // layer.rs) still has no collector, so shader fills there keep
        // falling through to the unchanged flat-fallback_color behavior
        // via paint_to_rgba/average_stop_color below — see this module's
        // doc comment and shader.rs's "known gaps" for why.
        //
        // A direct move (not `.as_deref_mut()`) is correct and sufficient
        // here specifically because `shader_shapes` is used exactly once,
        // right here, and never again for the rest of this function call —
        // unlike `tessellate_elements_with`'s loop, which genuinely needs
        // a fresh reborrow every iteration since it calls into this same
        // collector repeatedly across multiple sibling elements.
        let routed_to_shader = if let Some(shapes) = shader_shapes {
            if let Some(shader_def) = resolve_shader_def(fill, defs) {
                let (vertices, indices) = fill_path_positions(path, transform, canvas);
                shapes.push(PendingShaderShape { shader: shader_def.clone(), vertices, indices });
                true
            } else {
                false
            }
        } else {
            false
        };

        if !routed_to_shader {
            if let Some(rgba) = paint_to_rgba(fill, opacity, defs) {
                fill_path(path, rgba, transform, canvas, buffers);
            }
        }
    }

    if let Some(stroke) = style.stroke.as_ref().filter(|p| !p.is_none()) {
        let width = style.stroke_width.unwrap_or(1.0) as f32;
        if width > 0.0 {
            if let Some(rgba) = paint_to_rgba(stroke, opacity, defs) {
                stroke_path(path, rgba, width, transform, canvas, buffers);
            }
        }
    }
}

/// If `paint` is a `url(#id)` reference that resolves to a real
/// `Def::Shader` (not a gradient), returns it. Used to decide whether a
/// fill should be routed to `shader.rs` for real WGSL execution instead
/// of the flat `fallback_color`/average-stop-color treatment every other
/// paint reference gets.
fn resolve_shader_def<'a>(paint: &Paint, defs: &Defs<'a>) -> Option<&'a msx_ast::ShaderDef> {
    let Paint::Ref(reference) = paint else { return None };
    let id = reference.strip_prefix("url(#")?.strip_suffix(')')?;
    match defs.get(id)? {
        Def::Shader(shader) => Some(shader),
        _ => None,
    }
}

fn stroke_only_line(l: &Line, transform: Matrix2D, canvas: (f32, f32), buffers: &mut VertexBuffers<Vertex, u32>) {
    let Some(stroke) = l.style.stroke.as_ref().filter(|p| !p.is_none()) else { return };
    let width = l.style.stroke_width.unwrap_or(1.0) as f32;
    if width <= 0.0 {
        return;
    }
    let Paint::Color(c) = stroke else { return };
    let opacity = l.style.opacity.unwrap_or(1.0);
    let a = (c.a as f64 / 255.0) * opacity.clamp(0.0, 1.0);
    let rgba = [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, a as f32];

    let mut builder = LyonPath::builder();
    builder.begin(point(l.x1 as f32, l.y1 as f32));
    builder.line_to(point(l.x2 as f32, l.y2 as f32));
    builder.end(false);
    let path = builder.build();

    stroke_path(&path, rgba, width, transform, canvas, buffers);
}

fn fill_path(path: &LyonPath, rgba: [f32; 4], transform: Matrix2D, canvas: (f32, f32), buffers: &mut VertexBuffers<Vertex, u32>) {
    let mut tess = FillTessellator::new();
    let _ = tess.tessellate_path(
        path,
        &FillOptions::default(),
        &mut BuffersBuilder::new(buffers, |v: FillVertex| {
            vertex_from_point(v.position().x, v.position().y, rgba, transform, canvas)
        }),
    );
}

/// Position-only sibling of `fill_path`, for shapes whose fill routes to
/// `shader.rs` instead of being baked flat — there's no per-vertex color
/// to compute here, the user's fragment shader supplies it entirely.
fn fill_path_positions(path: &LyonPath, transform: Matrix2D, canvas: (f32, f32)) -> (Vec<[f32; 2]>, Vec<u32>) {
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let mut tess = FillTessellator::new();
    let _ = tess.tessellate_path(
        path,
        &FillOptions::default(),
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
            let (tx, ty) = apply_matrix(transform, (v.position().x, v.position().y));
            to_clip_space(tx, ty, canvas.0, canvas.1)
        }),
    );
    (buffers.vertices, buffers.indices)
}

fn stroke_path(path: &LyonPath, rgba: [f32; 4], width: f32, transform: Matrix2D, canvas: (f32, f32), buffers: &mut VertexBuffers<Vertex, u32>) {
    let mut tess = StrokeTessellator::new();
    // BUGFIX: lyon's `StrokeOptions` is `#[non_exhaustive]` (confirmed
    // against the published lyon_tessellation 1.0.20 source), so the old
    // `StrokeOptions { line_width: width, ..StrokeOptions::default() }`
    // struct-update syntax doesn't compile at all from outside the crate —
    // non_exhaustive blocks that even with `..Default::default()` filling
    // every other field. The crate's own builder method is the intended
    // replacement.
    let options = StrokeOptions::default().with_line_width(width);
    let _ = tess.tessellate_path(
        path,
        &options,
        &mut BuffersBuilder::new(buffers, |v: StrokeVertex| {
            vertex_from_point(v.position().x, v.position().y, rgba, transform, canvas)
        }),
    );
}

fn vertex_from_point(x: f32, y: f32, color: [f32; 4], transform: Matrix2D, canvas: (f32, f32)) -> Vertex {
    let (tx, ty) = apply_matrix(transform, (x, y));
    Vertex { position: to_clip_space(tx, ty, canvas.0, canvas.1), color }
}

fn apply_matrix(m: Matrix2D, p: (f32, f32)) -> (f32, f32) {
    (m.a as f32 * p.0 + m.c as f32 * p.1 + m.e as f32, m.b as f32 * p.0 + m.d as f32 * p.1 + m.f as f32)
}

fn to_clip_space(x: f32, y: f32, width: f32, height: f32) -> [f32; 2] {
    [(x / width) * 2.0 - 1.0, 1.0 - (y / height) * 2.0]
}

fn paint_to_rgba(paint: &Paint, opacity: f64, defs: &Defs) -> Option<[f32; 4]> {
    let color = match paint {
        Paint::None => return None,
        Paint::Color(c) => *c,
        Paint::CurrentColor => Color::BLACK,
        Paint::Ref(reference) => {
            let id = reference.strip_prefix("url(#")?.strip_suffix(')')?;
            average_stop_color(defs.get(id)?)
        }
    };
    let a = (color.a as f64 / 255.0) * opacity.clamp(0.0, 1.0);
    Some([color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0, a as f32])
}

/// Average color of a def's gradient stops — or, for a shader def, its
/// author-declared `fallback_color`. This vector-shader evaluator doesn't
/// execute WGSL either (that's `source_ref`'s job, deferred until there's
/// an actual WGSL-executing render path), so shader-filled shapes paint
/// flat with that fallback here too, same as msx-render-cpu. `pub(crate)`
/// so `sdf.rs` shares this instead of re-implementing its own.
pub(crate) fn average_stop_color(def: &Def) -> Color {
    let stops: &[msx_ast::Stop] = match def {
        Def::LinearGradient(g) => &g.stops,
        Def::RadialGradient(g) => &g.stops,
        Def::ConicGradient(g) => &g.stops,
        Def::Shader(s) => return s.fallback_color,
    };
    if stops.is_empty() {
        return Color::BLACK;
    }
    let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
    for s in stops {
        r += s.color.r as u32;
        g += s.color.g as u32;
        b += s.color.b as u32;
        a += s.color.a as u32;
    }
    let n = stops.len() as u32;
    Color::rgba((r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8)
}

// ── Shape → lyon path ────────────────────────────────────────────────────────

fn rect_path(r: &Rect) -> LyonPath {
    let mut b = LyonPath::builder();
    let (x, y, w, h) = (r.x as f32, r.y as f32, r.width as f32, r.height as f32);
    let rx = r.rx.map(|v| v as f32).unwrap_or(0.0).clamp(0.0, w / 2.0);
    let ry = r.ry.map(|v| v as f32).unwrap_or(rx).clamp(0.0, h / 2.0);

    if rx <= 0.0 || ry <= 0.0 {
        b.begin(point(x, y));
        b.line_to(point(x + w, y));
        b.line_to(point(x + w, y + h));
        b.line_to(point(x, y + h));
        b.end(true);
    } else {
        const K: f32 = 0.552_284_8;
        let (kx, ky) = (rx * K, ry * K);
        b.begin(point(x + rx, y));
        b.line_to(point(x + w - rx, y));
        b.cubic_bezier_to(point(x + w - rx + kx, y), point(x + w, y + ry - ky), point(x + w, y + ry));
        b.line_to(point(x + w, y + h - ry));
        b.cubic_bezier_to(point(x + w, y + h - ry + ky), point(x + w - rx + kx, y + h), point(x + w - rx, y + h));
        b.line_to(point(x + rx, y + h));
        b.cubic_bezier_to(point(x + rx - kx, y + h), point(x, y + h - ry + ky), point(x, y + h - ry));
        b.line_to(point(x, y + ry));
        b.cubic_bezier_to(point(x, y + ry - ky), point(x + rx - kx, y), point(x + rx, y));
        b.end(true);
    }
    b.build()
}

fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> LyonPath {
    let (cx, cy, rx, ry) = (cx as f32, cy as f32, rx as f32, ry as f32);
    const K: f32 = 0.552_284_8;
    let (kx, ky) = (rx * K, ry * K);
    let mut b = LyonPath::builder();
    b.begin(point(cx + rx, cy));
    b.cubic_bezier_to(point(cx + rx, cy + ky), point(cx + kx, cy + ry), point(cx, cy + ry));
    b.cubic_bezier_to(point(cx - kx, cy + ry), point(cx - rx, cy + ky), point(cx - rx, cy));
    b.cubic_bezier_to(point(cx - rx, cy - ky), point(cx - kx, cy - ry), point(cx, cy - ry));
    b.cubic_bezier_to(point(cx + kx, cy - ry), point(cx + rx, cy - ky), point(cx + rx, cy));
    b.end(true);
    b.build()
}

fn polyline_path(p: &Polyline) -> LyonPath {
    let mut b = LyonPath::builder();
    let mut points = p.points.iter();
    let Some(first) = points.next() else { return b.build() };
    b.begin(point(first.x as f32, first.y as f32));
    for pt in points {
        b.line_to(point(pt.x as f32, pt.y as f32));
    }
    b.end(p.closed);
    b.build()
}

fn msx_path_to_lyon(p: &msx_ast::Path) -> LyonPath {
    let mut b = LyonPath::builder();
    let mut current = (0.0f32, 0.0f32);
    let mut start = (0.0f32, 0.0f32);
    let mut open = false;

    macro_rules! ensure_open {
        () => {
            if !open {
                b.begin(point(current.0, current.1));
                open = true;
            }
        };
    }

    for cmd in &p.commands {
        match cmd {
            PathCommand::MoveTo(pt) => {
                if open { b.end(false); }
                current = (pt.x as f32, pt.y as f32);
                start = current;
                b.begin(point(current.0, current.1));
                open = true;
            }
            PathCommand::LineTo(pt) => {
                ensure_open!();
                current = (pt.x as f32, pt.y as f32);
                b.line_to(point(current.0, current.1));
            }
            PathCommand::HLineTo(x) => {
                ensure_open!();
                current = (*x as f32, current.1);
                b.line_to(point(current.0, current.1));
            }
            PathCommand::VLineTo(y) => {
                ensure_open!();
                current = (current.0, *y as f32);
                b.line_to(point(current.0, current.1));
            }
            PathCommand::CubicBezier { c1, c2, to } => {
                ensure_open!();
                b.cubic_bezier_to(point(c1.x as f32, c1.y as f32), point(c2.x as f32, c2.y as f32), point(to.x as f32, to.y as f32));
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::QuadBezier { c, to } => {
                ensure_open!();
                b.quadratic_bezier_to(point(c.x as f32, c.y as f32), point(to.x as f32, to.y as f32));
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::SmoothCubic { c2, to } => {
                ensure_open!();
                b.cubic_bezier_to(point(current.0, current.1), point(c2.x as f32, c2.y as f32), point(to.x as f32, to.y as f32));
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::SmoothQuad { to } => {
                ensure_open!();
                b.quadratic_bezier_to(point(current.0, current.1), point(to.x as f32, to.y as f32));
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, to } => {
                ensure_open!();
                let to_pt = (to.x as f32, to.y as f32);
                append_arc(&mut b, current, (*rx as f32, *ry as f32), *x_rotation as f32, *large_arc, *sweep, to_pt);
                current = to_pt;
            }
            PathCommand::ClosePath => {
                if open { b.end(true); open = false; }
                current = start;
            }
            PathCommand::RelMoveTo(pt) => {
                if open { b.end(false); }
                current = (current.0 + pt.x as f32, current.1 + pt.y as f32);
                start = current;
                b.begin(point(current.0, current.1));
                open = true;
            }
            PathCommand::RelLineTo(pt) => {
                ensure_open!();
                current = (current.0 + pt.x as f32, current.1 + pt.y as f32);
                b.line_to(point(current.0, current.1));
            }
            PathCommand::RelHLineTo(x) => {
                ensure_open!();
                current = (current.0 + *x as f32, current.1);
                b.line_to(point(current.0, current.1));
            }
            PathCommand::RelVLineTo(y) => {
                ensure_open!();
                current = (current.0, current.1 + *y as f32);
                b.line_to(point(current.0, current.1));
            }
            PathCommand::RelCubicBezier { c1, c2, to } => {
                ensure_open!();
                let c1p = point(current.0 + c1.x as f32, current.1 + c1.y as f32);
                let c2p = point(current.0 + c2.x as f32, current.1 + c2.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                b.cubic_bezier_to(c1p, c2p, point(to_pt.0, to_pt.1));
                current = to_pt;
            }
            PathCommand::RelQuadBezier { c, to } => {
                ensure_open!();
                let cp = point(current.0 + c.x as f32, current.1 + c.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                b.quadratic_bezier_to(cp, point(to_pt.0, to_pt.1));
                current = to_pt;
            }
            PathCommand::RelSmoothCubic { c2, to } => {
                ensure_open!();
                let c2p = point(current.0 + c2.x as f32, current.1 + c2.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                b.cubic_bezier_to(point(current.0, current.1), c2p, point(to_pt.0, to_pt.1));
                current = to_pt;
            }
            PathCommand::RelSmoothQuad { to } => {
                ensure_open!();
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                b.quadratic_bezier_to(point(current.0, current.1), point(to_pt.0, to_pt.1));
                current = to_pt;
            }
            PathCommand::RelArc { rx, ry, x_rotation, large_arc, sweep, to } => {
                ensure_open!();
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                append_arc(&mut b, current, (*rx as f32, *ry as f32), *x_rotation as f32, *large_arc, *sweep, to_pt);
                current = to_pt;
            }
        }
    }

    if open {
        b.end(false);
    }
    b.build()
}

/// Same SVG-arc-to-cubic-bezier construction as
/// `msx-render-cpu::rasterizer::append_arc`, targeting a `lyon`
/// `path::Builder` — see that function's doc comment for the derivation.
fn append_arc(b: &mut LyonPathBuilder, from: (f32, f32), radii: (f32, f32), x_rotation_deg: f32, large_arc: bool, sweep: bool, to: (f32, f32)) {
    let (x1, y1) = from;
    let (x2, y2) = to;
    let (mut rx, mut ry) = radii;

    if rx.abs() < 1e-6 || ry.abs() < 1e-6 || ((x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6) {
        b.line_to(point(x2, y2));
        return;
    }
    rx = rx.abs();
    ry = ry.abs();

    let phi = x_rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    let dx2 = (x1 - x2) / 2.0;
    let dy2 = (y1 - y2) / 2.0;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;

    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }

    let sign = if large_arc != sweep { 1.0 } else { -1.0 };
    let num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let coef = if den > 1e-9 { sign * (num / den).sqrt() } else { 0.0 };
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * -(ry * x1p / rx);

    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;

    let angle_between = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };

    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle_between((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }

    let segments = (delta.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let delta_seg = delta / segments as f32;
    let alpha = (4.0 / 3.0) * (delta_seg / 4.0).tan();

    let rotate = |px: f32, py: f32| -> (f32, f32) {
        (cos_phi * (px - cx) - sin_phi * (py - cy) + cx, sin_phi * (px - cx) + cos_phi * (py - cy) + cy)
    };

    let mut theta = theta1;
    for _ in 0..segments {
        let (sin_t, cos_t) = theta.sin_cos();
        let theta_next = theta + delta_seg;
        let (sin_tn, cos_tn) = theta_next.sin_cos();

        let p1 = (cx + rx * cos_t, cy + ry * sin_t);
        let p2 = (cx + rx * cos_tn, cy + ry * sin_tn);
        let d1 = (-rx * sin_t, ry * cos_t);
        let d2 = (-rx * sin_tn, ry * cos_tn);

        let c1 = (p1.0 + alpha * d1.0, p1.1 + alpha * d1.1);
        let c2 = (p2.0 - alpha * d2.0, p2.1 - alpha * d2.1);

        let c1r = rotate(c1.0, c1.1);
        let c2r = rotate(c2.0, c2.1);
        let p2r = rotate(p2.0, p2.1);

        b.cubic_bezier_to(point(c1r.0, c1r.1), point(c2r.0, c2r.1), point(p2r.0, p2r.1));
        theta = theta_next;
    }
        }

#[cfg(test)]
mod shader_routing_tests {
    use super::*;
    use msx_ast::{Canvas, Rect, ShaderDef, ShaderUniform, ShaderUniformValue};

    fn shader_style() -> Style {
        Style {
            fill: Some(Paint::Ref("url(#plasma_1)".to_string())),
            stroke: Some(Paint::None),
            stroke_width: Some(0.0),
            opacity: Some(1.0),
            ..Default::default()
        }
    }

    fn flat_style() -> Style {
        Style {
            fill: Some(Paint::Color(Color::rgb(10, 20, 30))),
            stroke: Some(Paint::None),
            stroke_width: Some(0.0),
            opacity: Some(1.0),
            ..Default::default()
        }
    }

    fn rect(style: Style) -> Element {
        Element::Rect(Rect { x: 0.0, y: 0.0, width: 20.0, height: 20.0, rx: None, ry: None, id: None, transform: None, style })
    }

    /// A rect whose fill resolves to a real `Def::Shader` must be pulled
    /// entirely out of the flat vertex batch and into the shader-shapes
    /// collector — not drawn twice, not left behind as a flat fallback
    /// fill (that fallback path is `lib.rs`'s job, only when the
    /// shader's `source_ref` fails to resolve at render time).
    #[test]
    fn shader_filled_rect_is_routed_to_the_shader_collector_not_the_flat_batch() {
        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));
        scene.defs.push(Def::Shader(
            ShaderDef::new("plasma_1", "shaders/plasma.wgsl", Color::rgb(0x6b, 0x46, 0xff))
                .with_uniforms(vec![ShaderUniform::new("speed", ShaderUniformValue::Float(1.5))]),
        ));
        scene.elements.push(rect(shader_style()));

        let (geometry, shader_shapes) = tessellate_scene_with_shaders(&scene);

        assert!(geometry.vertices.is_empty(), "shader-filled rect must not also land in the flat batch");
        assert_eq!(shader_shapes.len(), 1);
        assert_eq!(shader_shapes[0].shader.id, "plasma_1");
        assert!(!shader_shapes[0].vertices.is_empty(), "the shape still needs real tessellated geometry to draw with");
        assert!(!shader_shapes[0].indices.is_empty());
    }

    /// An ordinary flat-colored rect in the same scene must be completely
    /// unaffected — still lands in the normal batch, never touches the
    /// shader collector.
    #[test]
    fn flat_filled_rect_is_unaffected_by_shader_routing() {
        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));
        scene.elements.push(rect(flat_style()));

        let (geometry, shader_shapes) = tessellate_scene_with_shaders(&scene);

        assert!(!geometry.vertices.is_empty());
        assert!(shader_shapes.is_empty());
    }

    /// A gradient ref (not a shader def) must keep resolving through the
    /// ordinary flat-fill path — `resolve_shader_def` has to distinguish
    /// `Def::Shader` from every other `Def` variant, not just check "is
    /// this a `Paint::Ref` at all".
    #[test]
    fn gradient_ref_is_not_mistaken_for_a_shader_ref() {
        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));
        scene.defs.push(Def::LinearGradient(msx_ast::LinearGradient {
            id: "grad_1".to_string(),
            x1: 0.0, y1: 0.0, x2: 1.0, y2: 0.0,
            stops: vec![msx_ast::Stop::new(0.0, Color::rgb(255, 0, 0))],
        }));
        let style = Style { fill: Some(Paint::Ref("url(#grad_1)".to_string())), ..flat_style() };
        scene.elements.push(rect(style));

        let (geometry, shader_shapes) = tessellate_scene_with_shaders(&scene);

        assert!(!geometry.vertices.is_empty(), "gradient-filled rect must still land in the flat batch");
        assert!(shader_shapes.is_empty());
    }

    /// The plain, non-shader-aware `tessellate_scene`/`tessellate_elements`
    /// entry points (what `layer.rs` and any external caller actually
    /// use) must keep their exact pre-existing behavior: a shader ref
    /// still resolves via `average_stop_color`'s `fallback_color` path
    /// and lands in the flat batch, since there's no collector to divert
    /// it to. This is the "shader fills inside a Layer still paint flat"
    /// gap, demonstrated directly rather than just asserted in a comment.
    #[test]
    fn plain_tessellate_scene_still_flat_fills_shader_refs_no_collector_available() {
        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));
        scene.defs.push(Def::Shader(ShaderDef::new("plasma_1", "shaders/plasma.wgsl", Color::rgb(0x6b, 0x46, 0xff))));
        scene.elements.push(rect(shader_style()));

        let geometry = tessellate_scene(&scene);

        assert!(!geometry.vertices.is_empty(), "with no collector, a shader ref must still flat-fill via fallback_color");
    }
                  }
