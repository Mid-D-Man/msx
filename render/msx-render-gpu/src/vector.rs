// render/msx-render-gpu/src/vector.rs
//! Vector shape tessellation: every SVG-native element becomes triangles
//! via `lyon`, with color baked directly into each vertex and screen
//! position pre-transformed into WebGPU clip space on the CPU side.
//!
//! `tessellate_elements` is the actual workhorse now — `tessellate_scene`
//! is a thin wrapper over it. It stays public and unchanged (flat
//! geometry in, flat geometry out) so nothing outside this crate that
//! might already depend on that shape breaks.
//!
//! `tessellate_scene_with_shaders` is a `pub(crate)`-only, whole-scene
//! convenience sibling of `tessellate_scene`: it additionally pulls out
//! any shape whose *fill* resolves to a `Def::Shader` into its own
//! collection (see `shader.rs::PendingShaderShape`) instead of baking a
//! flat color for it. It's kept for tests and any future whole-scene
//! convenience call, but it is NOT what the real top-level render path
//! calls anymore — see the next paragraph.
//!
//! `tessellate_elements_with_shaders` is the same idea, but takes an
//! arbitrary `&[Element]` slice plus an already-built `Defs` instead of
//! a whole `Scene` — this is the one actually on the hot path now, for
//! BOTH the top level and every `Layer`. `layer::render_ordered`
//! (`layer.rs`) walks a sibling list in real paint order and calls this
//! once per contiguous non-`Layer` run, rather than once for the whole
//! scene up front; `lib.rs`'s top-level render and `layer.rs`'s
//! `render_layer` both go through `render_ordered` now, so both end up
//! calling this same function, just on different element slices. It
//! always takes the *real* scene `Defs` (not an empty one — a `Layer`'s
//! children can reference the same top-level `defs` as everything else,
//! gradients included) — `layer.rs` used to call the plain
//! `tessellate_elements` with an empty `Defs`, which is why shader fills
//! (and gradient/`Paint::Ref` fills in general) silently painted flat
//! for anything inside a `Layer`; both were fixed by the same earlier
//! change, not two separate ones.

use std::collections::HashMap;

use lyon::math::point;
use lyon::path::path::Builder as LyonPathBuilder;
use lyon::path::Path as LyonPath;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule as LyonFillRule, FillTessellator, FillVertex,
    StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};

use msx_ast::{
    path::PathCommand, Color, Def, Element, FillRule, Line, Matrix2D, Paint, Polyline, Rect,
    Scene, Style, Use,
};

use crate::gradient::{self, Bbox, GradientDesc, NO_GRADIENT};
use crate::shader::PendingShaderShape;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    /// Baked color. Ignored by the fragment stage when `grad` is a real index.
    pub color: [f32; 4],
    /// Position in `objectBoundingBox` units, used only by gradient vertices.
    pub uv: [f32; 2],
    /// Index into `VectorGeometry::gradients`, or `NO_GRADIENT` (`u32::MAX`)
    /// for a flat vertex.
    pub grad: u32,
}

impl Vertex {
    /// A flat-colored vertex.
    pub fn flat(position: [f32; 2], color: [f32; 4]) -> Self {
        Vertex { position, color, uv: [0.0, 0.0], grad: NO_GRADIENT }
    }
}

pub struct VectorGeometry {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Gradient records the vertices index into (see `gradient.rs`).
    pub gradients: Vec<GradientDesc>,
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

/// Public, unchanged entry point: tessellate a set of elements with an
/// empty `Defs` (no gradient/shader-ref resolution) and no shader-shape
/// collection. Kept exactly as it was for any external caller that
/// already depends on this shape; `layer.rs` uses
/// `tessellate_elements_with_shaders` below instead.
pub fn tessellate_elements(elements: &[Element], base_transform: Matrix2D, canvas: (f32, f32)) -> VectorGeometry {
    let defs = Defs::build(&[]);
    let index = ElementIndex::build(elements);
    tessellate_elements_with(elements, base_transform, canvas, &defs, &index, None)
}

/// `layer.rs`'s shader-aware entry point: tessellate one layer's children
/// against the *real* scene `defs` the caller passes in (so gradient and
/// shader-def refs on shapes inside a `Layer` resolve exactly like they
/// do for top-level shapes), and collect any shape whose fill resolves
/// to a `Def::Shader` separately, exactly like
/// `tessellate_scene_with_shaders` does for a whole `Scene`.
#[cfg(test)]
pub(crate) fn tessellate_elements_with_shaders(
    elements: &[Element],
    base_transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
) -> (VectorGeometry, Vec<PendingShaderShape>) {
    let index = ElementIndex::build(elements);
    tessellate_elements_with_shaders_in(elements, base_transform, canvas, defs, &index)
}

/// Same as `tessellate_elements_with_shaders`, but resolves `use` targets
/// through a caller-supplied `index`. `layer.rs` passes one index built
/// over the whole sibling list, because it draws that list as several
/// sub-runs and a `use` may point at an element in a neighboring sub-run.
pub(crate) fn tessellate_elements_with_shaders_in(
    elements: &[Element],
    base_transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    index: &ElementIndex,
) -> (VectorGeometry, Vec<PendingShaderShape>) {
    let mut shader_shapes = Vec::new();
    let geometry = tessellate_elements_with(elements, base_transform, canvas, defs, index, Some(&mut shader_shapes));
    (geometry, shader_shapes)
}

/// One run's flat geometry plus the gradient records its vertices index into.
struct Batch {
    buffers: VertexBuffers<Vertex, u32>,
    gradients: Vec<GradientDesc>,
}

impl Batch {
    fn push_gradient(&mut self, desc: GradientDesc) -> u32 {
        self.gradients.push(desc);
        (self.gradients.len() - 1) as u32
    }
}

fn tessellate_elements_with(
    elements: &[Element],
    base_transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    index: &ElementIndex,
    mut shader_shapes: Option<&mut Vec<PendingShaderShape>>,
) -> VectorGeometry {
    let mut batch = Batch { buffers: VertexBuffers::new(), gradients: Vec::new() };
    for element in elements {
        tessellate_element(element, base_transform, canvas, defs, index, &mut batch, shader_shapes.as_deref_mut());
    }
    VectorGeometry { vertices: batch.buffers.vertices, indices: batch.buffers.indices, gradients: batch.gradients }
}

fn tessellate_element(
    element: &Element,
    transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    index: &ElementIndex,
    batch: &mut Batch,
    mut shader_shapes: Option<&mut Vec<PendingShaderShape>>,
) {
    match element {
        Element::Rect(r) => {
            let path = rect_path(r);
            fill_and_stroke(&path, &r.style, combine(transform, r.transform.as_ref()), canvas, defs, batch, shader_shapes, None);
        }
        Element::Circle(c) => {
            let path = ellipse_path(c.cx, c.cy, c.r, c.r);
            fill_and_stroke(&path, &c.style, combine(transform, c.transform.as_ref()), canvas, defs, batch, shader_shapes, None);
        }
        Element::Ellipse(e) => {
            let path = ellipse_path(e.cx, e.cy, e.rx, e.ry);
            fill_and_stroke(&path, &e.style, combine(transform, e.transform.as_ref()), canvas, defs, batch, shader_shapes, None);
        }
        Element::Line(l) => stroke_only_line(l, combine(transform, l.transform.as_ref()), canvas, defs, batch),
        Element::Polyline(p) | Element::Polygon(p) => {
            let path = polyline_path(p);
            fill_and_stroke(&path, &p.style, combine(transform, p.transform.as_ref()), canvas, defs, batch, shader_shapes, None);
        }
        Element::Path(p) => {
            let path = msx_path_to_lyon(p);
            fill_and_stroke(&path, &p.style, combine(transform, p.transform.as_ref()), canvas, defs, batch, shader_shapes, None);
        }
        Element::Text(t) => {
            // Glyph outlines from the embedded face, tessellated like any
            // path, so fills, strokes, gradients and shader fills all apply.
            // The text box is the gradient bounding box.
            if let Some((path, bounds)) = msx_text::to_path_with_bounds(t) {
                let lyon_path = msx_path_to_lyon(&path);
                let text_box = Some(Bbox::from_text_box(bounds));
                fill_and_stroke(&lyon_path, &path.style, combine(transform, path.transform.as_ref()), canvas, defs, batch, shader_shapes, text_box);
            }
        }
        Element::Group(g) => {
            let combined = combine(transform, g.transform.as_ref());
            for child in &g.children {
                tessellate_element(child, combined, canvas, defs, index, batch, shader_shapes.as_deref_mut());
            }
        }
        Element::Use(u) => tessellate_use(u, transform, canvas, defs, index, batch, shader_shapes),
        Element::Sdf(_) | Element::Splat(_) => {
            // sdf.rs / splat.rs handle these in their own passes.
        }
        Element::Image(_) => {
            // image.rs's ImagePipeline handles these in its own pass.
        }
        Element::Layer(_) => {
            // layer.rs handles these via its own isolated render +
            // composite, never flattened into the shared vector pass.
        }
    }
}

fn tessellate_use(
    u: &Use,
    parent: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    index: &ElementIndex,
    batch: &mut Batch,
    shader_shapes: Option<&mut Vec<PendingShaderShape>>,
) {
    let id = u.href.strip_prefix('#').unwrap_or(&u.href);
    let Some(target) = index.get(id) else { return };
    let offset = Matrix2D::translate(u.x, u.y);
    let local = u.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    tessellate_element(target, parent.concat(offset.concat(local)), canvas, defs, index, batch, shader_shapes);
}

fn combine(parent: Matrix2D, local: Option<&msx_ast::Transform>) -> Matrix2D {
    match local {
        None => parent,
        Some(t) => parent.concat(t.to_matrix()),
    }
}

/// msx-ast's `FillRule` and lyon's own `FillRule` are structurally
/// identical (`NonZero`/`EvenOdd`) but distinct types — msx-ast's is the
/// wire/DixScript-facing one (`FillRule::parse`, `to_svg`, `to_byte`),
/// lyon's is what `FillOptions::with_fill_rule` actually takes.
fn to_lyon_fill_rule(fr: FillRule) -> LyonFillRule {
    match fr {
        FillRule::NonZero => LyonFillRule::NonZero,
        FillRule::EvenOdd => LyonFillRule::EvenOdd,
    }
}

// ── Fill + stroke ────────────────────────────────────────────────────────────

/// `text_box` overrides the path bounds as the gradient bounding box; only
/// text sets it.
#[allow(clippy::too_many_arguments)]
fn fill_and_stroke(
    path: &LyonPath,
    style: &Style,
    transform: Matrix2D,
    canvas: (f32, f32),
    defs: &Defs,
    batch: &mut Batch,
    shader_shapes: Option<&mut Vec<PendingShaderShape>>,
    text_box: Option<Bbox>,
) {
    let opacity = style.opacity.unwrap_or(1.0);
    // `FillOptions::default()` is lyon's own default, which is `EvenOdd`,
    // NOT the SVG-spec default (`NonZero`). Wire the rule explicitly so
    // self-intersecting shapes and overlapping same-direction contours
    // match the other renderers.
    let fill_rule = to_lyon_fill_rule(style.fill_rule.unwrap_or(FillRule::NonZero));
    let bbox = text_box.or_else(|| Bbox::from_path(path));

    if let Some(fill) = style.fill.as_ref().filter(|p| !p.is_none()) {
        // A fill that resolves to a real Def::Shader is pulled out into its
        // own draw, executed for real by shader.rs, but only when a
        // collector was provided. The plain public `tessellate_scene` and
        // `tessellate_elements` pass `None`, so shader fills through those
        // two still fall back to the flat `fallback_color`.
        let routed_to_shader = if let Some(shapes) = shader_shapes {
            if let Some(shader_def) = resolve_shader_def(fill, defs) {
                let (vertices, indices) = fill_path_positions(path, fill_rule, transform, canvas);
                shapes.push(PendingShaderShape { shader: shader_def.clone(), vertices, indices });
                true
            } else {
                false
            }
        } else {
            false
        };

        if !routed_to_shader {
            match resolve_paint(fill, opacity, defs, bbox) {
                Some(ResolvedPaint::Flat(rgba)) => fill_path(path, fill_rule, rgba, transform, canvas, &mut batch.buffers),
                Some(ResolvedPaint::Gradient(desc, b)) => {
                    let grad = batch.push_gradient(*desc);
                    fill_path_gradient(path, fill_rule, grad, b, transform, canvas, &mut batch.buffers);
                }
                None => {}
            }
        }
    }

    stroke_with_paint(path, style, transform, canvas, defs, batch, bbox);
}

fn stroke_with_paint(path: &LyonPath, style: &Style, transform: Matrix2D, canvas: (f32, f32), defs: &Defs, batch: &mut Batch, bbox: Option<Bbox>) {
    let Some(stroke) = style.stroke.as_ref().filter(|p| !p.is_none()) else { return };
    let width = style.stroke_width.unwrap_or(1.0) as f32;
    if width <= 0.0 {
        return;
    }
    let opacity = style.opacity.unwrap_or(1.0);
    match resolve_paint(stroke, opacity, defs, bbox) {
        Some(ResolvedPaint::Flat(rgba)) => stroke_path(path, rgba, width, transform, canvas, &mut batch.buffers),
        Some(ResolvedPaint::Gradient(desc, b)) => {
            let grad = batch.push_gradient(*desc);
            stroke_path_gradient(path, width, grad, b, transform, canvas, &mut batch.buffers);
        }
        None => {}
    }
}

/// A paint ready to tessellate with.
enum ResolvedPaint {
    Flat([f32; 4]),
    /// Boxed gradient record plus the bounding box its `uv` is measured against.
    Gradient(Box<GradientDesc>, Bbox),
}

/// Resolve a fill or stroke paint. A gradient reference with an empty
/// bounding box paints nothing, as in SVG. Shader defs and anything that is
/// not a gradient resolve to a flat color through `paint_to_rgba`.
fn resolve_paint(paint: &Paint, opacity: f64, defs: &Defs, bbox: Option<Bbox>) -> Option<ResolvedPaint> {
    if let Paint::Ref(reference) = paint {
        let id = reference.strip_prefix("url(#")?.strip_suffix(')')?;
        let def = defs.get(id)?;
        match gradient::resolve(def, bbox, opacity as f32) {
            Some(gradient::Resolved::Gradient(desc)) => return bbox.map(|b| ResolvedPaint::Gradient(desc, b)),
            Some(gradient::Resolved::Nothing) => return None,
            None => {}
        }
    }
    paint_to_rgba(paint, opacity, defs).map(ResolvedPaint::Flat)
}

/// If `paint` is a `url(#id)` reference that resolves to a real
/// `Def::Shader` (not a gradient), returns it. Used to decide whether a
/// fill should be routed to `shader.rs` for real WGSL execution instead
/// of the flat `fallback_color`/average-stop-color treatment every other
/// paint reference gets. `pub(crate)`, not private — `sdf.rs` reuses this
/// exact function for the same decision on SDF node fills rather than
/// duplicating it.
pub(crate) fn resolve_shader_def<'a>(paint: &Paint, defs: &Defs<'a>) -> Option<&'a msx_ast::ShaderDef> {
    let Paint::Ref(reference) = paint else { return None };
    let id = reference.strip_prefix("url(#")?.strip_suffix(')')?;
    match defs.get(id)? {
        Def::Shader(shader) => Some(shader),
        _ => None,
    }
}

fn stroke_only_line(l: &Line, transform: Matrix2D, canvas: (f32, f32), defs: &Defs, batch: &mut Batch) {
    let mut builder = LyonPath::builder();
    builder.begin(point(l.x1 as f32, l.y1 as f32));
    builder.line_to(point(l.x2 as f32, l.y2 as f32));
    builder.end(false);
    let path = builder.build();

    // A line has no fill. Its gradient bounding box has no area when the
    // line is perfectly horizontal or vertical, so a gradient stroke on
    // one paints nothing, as in SVG.
    let bbox = Bbox::from_path(&path);
    stroke_with_paint(&path, &l.style, transform, canvas, defs, batch, bbox);
}

fn fill_path(
    path: &LyonPath,
    fill_rule: LyonFillRule,
    rgba: [f32; 4],
    transform: Matrix2D,
    canvas: (f32, f32),
    buffers: &mut VertexBuffers<Vertex, u32>,
) {
    let mut tess = FillTessellator::new();
    let _ = tess.tessellate_path(
        path,
        &FillOptions::default().with_fill_rule(fill_rule),
        &mut BuffersBuilder::new(buffers, |v: FillVertex| {
            vertex_from_point(v.position().x, v.position().y, rgba, transform, canvas)
        }),
    );
}

/// Position-only sibling of `fill_path`, for shapes whose fill routes to
/// `shader.rs` instead of being baked flat — there's no per-vertex color
/// to compute here, the user's fragment shader supplies it entirely.
fn fill_path_positions(
    path: &LyonPath,
    fill_rule: LyonFillRule,
    transform: Matrix2D,
    canvas: (f32, f32),
) -> (Vec<[f32; 2]>, Vec<u32>) {
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let mut tess = FillTessellator::new();
    let _ = tess.tessellate_path(
        path,
        &FillOptions::default().with_fill_rule(fill_rule),
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
            let (tx, ty) = apply_matrix(transform, (v.position().x, v.position().y));
            to_clip_space(tx, ty, canvas.0, canvas.1)
        }),
    );
    (buffers.vertices, buffers.indices)
}

fn fill_path_gradient(path: &LyonPath, fill_rule: LyonFillRule, grad: u32, bbox: Bbox, transform: Matrix2D, canvas: (f32, f32), buffers: &mut VertexBuffers<Vertex, u32>) {
    let mut tess = FillTessellator::new();
    let _ = tess.tessellate_path(
        path,
        &FillOptions::default().with_fill_rule(fill_rule),
        &mut BuffersBuilder::new(buffers, |v: FillVertex| gradient_vertex(v.position().x, v.position().y, grad, bbox, transform, canvas)),
    );
}

fn stroke_path_gradient(path: &LyonPath, width: f32, grad: u32, bbox: Bbox, transform: Matrix2D, canvas: (f32, f32), buffers: &mut VertexBuffers<Vertex, u32>) {
    let mut tess = StrokeTessellator::new();
    let options = StrokeOptions::default().with_line_width(width);
    let _ = tess.tessellate_path(
        path,
        &options,
        &mut BuffersBuilder::new(buffers, |v: StrokeVertex| gradient_vertex(v.position().x, v.position().y, grad, bbox, transform, canvas)),
    );
}

/// A vertex whose color comes from gradient record `grad`. `uv` is measured
/// from the untransformed local position, so it is exact under any transform.
fn gradient_vertex(x: f32, y: f32, grad: u32, bbox: Bbox, transform: Matrix2D, canvas: (f32, f32)) -> Vertex {
    let (tx, ty) = apply_matrix(transform, (x, y));
    Vertex { position: to_clip_space(tx, ty, canvas.0, canvas.1), color: [1.0, 1.0, 1.0, 1.0], uv: bbox.uv(x, y), grad }
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
    Vertex::flat(to_clip_space(tx, ty, canvas.0, canvas.1), color)
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
        // Same transparent fallback as msx-render-svg's splat.rs and
        // msx-render-cpu's rasterizer.rs — Audio has no visual
        // representation, this function is only ever reached via a
        // Paint::Ref that happens to point at a non-visual def.
        Def::Audio(_) => return Color::rgba(0, 0, 0, 0),
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
    // Control points of the previous curve, for the reflection that S and T
    // use. They only carry over to the very next command.
    let mut prev_c: Option<(f32, f32)> = None;
    let mut prev_q: Option<(f32, f32)> = None;
    let reflect = |ctl: Option<(f32, f32)>, cur: (f32, f32)| ctl.map_or(cur, |c| (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1));

    macro_rules! ensure_open {
        () => {
            if !open {
                b.begin(point(current.0, current.1));
                open = true;
            }
        };
    }

    for cmd in &p.commands {
        let (pc, pq) = (prev_c.take(), prev_q.take());
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
                prev_c = Some((c2.x as f32, c2.y as f32));
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::QuadBezier { c, to } => {
                ensure_open!();
                b.quadratic_bezier_to(point(c.x as f32, c.y as f32), point(to.x as f32, to.y as f32));
                prev_q = Some((c.x as f32, c.y as f32));
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::SmoothCubic { c2, to } => {
                ensure_open!();
                let c1 = reflect(pc, current);
                b.cubic_bezier_to(point(c1.0, c1.1), point(c2.x as f32, c2.y as f32), point(to.x as f32, to.y as f32));
                prev_c = Some((c2.x as f32, c2.y as f32));
                current = (to.x as f32, to.y as f32);
            }
            PathCommand::SmoothQuad { to } => {
                ensure_open!();
                let c = reflect(pq, current);
                b.quadratic_bezier_to(point(c.0, c.1), point(to.x as f32, to.y as f32));
                prev_q = Some(c);
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
                prev_c = Some((c2p.x, c2p.y));
                current = to_pt;
            }
            PathCommand::RelQuadBezier { c, to } => {
                ensure_open!();
                let cp = point(current.0 + c.x as f32, current.1 + c.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                b.quadratic_bezier_to(cp, point(to_pt.0, to_pt.1));
                prev_q = Some((cp.x, cp.y));
                current = to_pt;
            }
            PathCommand::RelSmoothCubic { c2, to } => {
                ensure_open!();
                let c2p = point(current.0 + c2.x as f32, current.1 + c2.y as f32);
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                let c1 = reflect(pc, current);
                b.cubic_bezier_to(point(c1.0, c1.1), c2p, point(to_pt.0, to_pt.1));
                prev_c = Some((c2p.x, c2p.y));
                current = to_pt;
            }
            PathCommand::RelSmoothQuad { to } => {
                ensure_open!();
                let to_pt = (current.0 + to.x as f32, current.1 + to.y as f32);
                let c = reflect(pq, current);
                b.quadratic_bezier_to(point(c.0, c.1), point(to_pt.0, to_pt.1));
                prev_q = Some(c);
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
    /// entry points (kept unchanged for any external caller already
    /// depending on their exact pre-existing shape) must keep flat-filling
    /// a shader ref via `average_stop_color`'s `fallback_color` path,
    /// since there's no collector to divert it to. `layer.rs` itself no
    /// longer uses this path — see `tessellate_elements_with_shaders_diverts_a_shader_ref_into_the_collector`
    /// below for its actual (shader-aware) behavior now.
    #[test]
    fn plain_tessellate_scene_still_flat_fills_shader_refs_no_collector_available() {
        let mut scene = Scene::new(Canvas::new(20.0, 20.0, Color::BLACK));
        scene.defs.push(Def::Shader(ShaderDef::new("plasma_1", "shaders/plasma.wgsl", Color::rgb(0x6b, 0x46, 0xff))));
        scene.elements.push(rect(shader_style()));

        let geometry = tessellate_scene(&scene);

        assert!(!geometry.vertices.is_empty(), "with no collector, a shader ref must still flat-fill via fallback_color");
    }

    /// `layer.rs` calls `tessellate_elements_with_shaders` now (not the
    /// plain `tessellate_elements`), passing the *real* scene defs — this
    /// is the fix for "shader fills inside a Layer paint flat". Proven
    /// directly here at the tessellation level (no GPU adapter needed):
    /// a shader-ref shape passed through this function, with a `Defs`
    /// that actually contains the referenced `Def::Shader`, must land in
    /// `shader_shapes`, not the flat geometry batch.
    #[test]
    fn tessellate_elements_with_shaders_diverts_a_shader_ref_into_the_collector() {
        let defs_vec = vec![Def::Shader(ShaderDef::new("plasma_1", "shaders/plasma.wgsl", Color::rgb(0x6b, 0x46, 0xff)))];
        let defs = Defs::build(&defs_vec);
        let elements = vec![rect(shader_style())];

        let (geometry, shader_shapes) = tessellate_elements_with_shaders(&elements, Matrix2D::identity(), (20.0, 20.0), &defs);

        assert!(geometry.vertices.is_empty(), "a shader-ref shape must NOT land in the flat batch once a collector is available");
        assert_eq!(shader_shapes.len(), 1, "the shader-ref shape must be diverted into shader_shapes instead");
    }
                

    #[test]
    fn smooth_curves_reflect_the_previous_control_point() {
        use lyon::algorithms::aabb::bounding_box;
        let lyon_bottom = |d: &str| {
            let path = msx_ast::Path { commands: msx_ast::path::parse_d(d).unwrap(), d_raw: d.to_string(), id: None, transform: None, style: Style::default() };
            bounding_box(msx_path_to_lyon(&path).iter()).max.y
        };
        // The reflected quad control (50, 250) pulls the second T segment down
        // to about y = 167. Left at the current point it would stay a line.
        assert!(lyon_bottom("M 30 30 T 40 140 T 170 30") > 160.0);
        // S reflects (100, 20) about (100, 100) to (100, 180).
        assert!(lyon_bottom("M 20 100 C 20 20 100 20 100 100 S 180 180 180 100") > 155.0);
        assert!(lyon_bottom("m 20 100 c 0 -80 80 -80 80 0 s 80 80 80 0") > 155.0);
        assert!(lyon_bottom("m 20 100 q 40 -80 80 0 t 80 0") > 100.0 - 1.0);
    }
}
