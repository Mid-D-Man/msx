// render/msx-render-cpu/src/sdf_raster.rs
//! Per-pixel SDF evaluation, decoupled from `tiny-skia`'s own path
//! pipeline: for every pixel in a conservative bounding region, map the
//! pixel center back into the `SdfNode`'s local space (via the inverse of
//! its accumulated transform, `geom::invert_matrix`) and evaluate the tree
//! directly with `msx-sdf`'s primitive/combinator functions.
//!
//! `msx-sdf` deliberately has zero `msx-ast` dependency (see its own crate
//! docs), so the "walk an `SdfTree` and call into `msx-sdf`'s functions"
//! dispatch — `evaluate_tree` below — lives here, the one crate that
//! already depends on both.

use msx_ast::{Color, Matrix2D, Paint, SdfNode, SdfTree};
use rayon::prelude::*;
use tiny_skia::Pixmap;

use crate::geom::{apply_matrix, invert_matrix, transform_bounds};
use crate::pixel::{read_premul, write_premul};
use crate::rasterizer::{average_stop_color, Defs};
use msx_render_core::PremulColor;

pub fn rasterize_sdf(pixmap: &mut Pixmap, node: &SdfNode, parent: Matrix2D, defs: &Defs) {
    let local = node.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    let combined = parent.concat(local);
    let Some(inv) = invert_matrix(combined) else { return };

    let local_bounds = sdf_bounds(&node.tree);
    let screen_bounds = transform_bounds(local_bounds, combined);

    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let min_x = (screen_bounds.0.floor() as i32).max(0);
    let min_y = (screen_bounds.1.floor() as i32).max(0);
    let max_x = (screen_bounds.2.ceil() as i32).min(width);
    let max_y = (screen_bounds.3.ceil() as i32).min(height);
    if min_x >= max_x || min_y >= max_y {
        return;
    }

    let fill_color = paint_to_color(&node.fill, defs);
    let stroke = node.stroke.as_ref().map(|s| (paint_to_color(s, defs), node.stroke_width.unwrap_or(1.0) as f32));

    let row_bytes = (width as usize) * 4;
    let row_start = (min_y as usize) * row_bytes;
    let row_end = (max_y as usize) * row_bytes;
    let data = pixmap.data_mut();

    data[row_start..row_end].par_chunks_mut(row_bytes).enumerate().for_each(|(row_offset, row)| {
        let y = min_y + row_offset as i32;
        for x in min_x..max_x {
            let screen_p = (x as f32 + 0.5, y as f32 + 0.5);
            let local_p = apply_matrix(inv, screen_p);
            let d = evaluate_tree(&node.tree, local_p);

            let mut chosen: Option<PremulColor> = None;

            if let Some((stroke_color, width)) = stroke {
                let band = d.abs() - width * 0.5;
                if band <= 1.0 {
                    chosen = Some(antialiased(stroke_color, band));
                }
            }
            if chosen.is_none() && d <= 1.0 {
                chosen = Some(antialiased(fill_color, d));
            }

            if let Some(src) = chosen {
                let idx = (x as usize) * 4;
                let dst = read_premul(row, idx);
                write_premul(row, idx, src.over(dst));
            }
        }
    });
}

/// 1px-wide smoothstep across the SDF boundary — `d <= -1` fully opaque,
/// `d >= 1` fully transparent, linear in between. Cheap, good enough
/// antialiasing for a CPU rasterizer; supersampling or exact coverage isn't
/// worth the cost here.
fn antialiased(color: PremulColor, d: f32) -> PremulColor {
    let coverage = (1.0 - (d + 1.0) * 0.5).clamp(0.0, 1.0);
    PremulColor { r: color.r * coverage, g: color.g * coverage, b: color.b * coverage, a: color.a * coverage }
}

fn paint_to_color(paint: &Paint, defs: &Defs) -> PremulColor {
    match paint {
        Paint::Color(c) => PremulColor::from_color(*c),
        Paint::CurrentColor => PremulColor::from_color(Color::BLACK),
        Paint::None => PremulColor::TRANSPARENT,
        Paint::Ref(reference) => {
            // Same resolution `rasterizer.rs::resolve_paint` uses for
            // every other shape type: a real tiny-skia gradient shader
            // isn't wired up yet, so a gradient ref paints its stops'
            // average color, and a shader ref paints its declared
            // `fallback_color` — either way, something sane rather than
            // silently invisible. An SDF's fill/stroke can reference a
            // def exactly the same way any other shape's can; this used
            // to unconditionally return TRANSPARENT for any `Paint::Ref`
            // here regardless of what it pointed at, independent of
            // whether `rasterizer.rs`'s handling for other shapes agreed.
            match reference.strip_prefix("url(#").and_then(|s| s.strip_suffix(')')) {
                Some(id) => match defs.get(id) {
                    Some(def) => PremulColor::from_color(average_stop_color(def)),
                    None => PremulColor::TRANSPARENT,
                },
                None => PremulColor::TRANSPARENT,
            }
        }
    }
}

fn evaluate_tree(tree: &SdfTree, p: (f32, f32)) -> f32 {
    let v = glam::Vec2::new(p.0, p.1);
    match tree {
        SdfTree::Circle { cx, cy, r } => msx_sdf::circle(v, glam::Vec2::new(*cx as f32, *cy as f32), *r as f32),
        SdfTree::Box { x, y, width, height, corner_radius } => {
            let half = glam::Vec2::new(*width as f32, *height as f32) * 0.5;
            let center = glam::Vec2::new(*x as f32, *y as f32) + half;
            msx_sdf::rounded_box(v, center, half, *corner_radius as f32)
        }
        SdfTree::Line { x1, y1, x2, y2, thickness } => msx_sdf::segment(
            v,
            glam::Vec2::new(*x1 as f32, *y1 as f32),
            glam::Vec2::new(*x2 as f32, *y2 as f32),
            *thickness as f32,
        ),
        SdfTree::Ring { cx, cy, r, thickness } => {
            msx_sdf::ring(v, glam::Vec2::new(*cx as f32, *cy as f32), *r as f32, *thickness as f32)
        }
        SdfTree::Arc { cx, cy, r, angle_start, angle_end, thickness } => msx_sdf::arc(
            v,
            glam::Vec2::new(*cx as f32, *cy as f32),
            *r as f32,
            *angle_start as f32,
            *angle_end as f32,
            *thickness as f32,
        ),
        SdfTree::Union(children) => children.iter().map(|c| evaluate_tree(c, p)).fold(f32::INFINITY, msx_sdf::union),
        SdfTree::SmoothUnion { children, k } => children
            .iter()
            .map(|c| evaluate_tree(c, p))
            .fold(f32::INFINITY, |acc, d| msx_sdf::smooth_union(acc, d, *k as f32)),
        SdfTree::Subtract { a, b } => msx_sdf::subtract(evaluate_tree(a, p), evaluate_tree(b, p)),
        SdfTree::SmoothSubtract { a, b, k } => {
            msx_sdf::smooth_subtract(evaluate_tree(a, p), evaluate_tree(b, p), *k as f32)
        }
        SdfTree::Intersect { a, b } => msx_sdf::intersect(evaluate_tree(a, p), evaluate_tree(b, p)),
        SdfTree::SmoothIntersect { a, b, k } => {
            msx_sdf::smooth_intersect(evaluate_tree(a, p), evaluate_tree(b, p), *k as f32)
        }
        SdfTree::Offset { child, amount } => msx_sdf::offset(evaluate_tree(child, p), *amount as f32),
    }
}

fn sdf_bounds(tree: &SdfTree) -> (f32, f32, f32, f32) {
    match tree {
        SdfTree::Circle { cx, cy, r } => circle_bounds(*cx, *cy, *r),
        SdfTree::Box { x, y, width, height, .. } => (*x as f32, *y as f32, (*x + *width) as f32, (*y + *height) as f32),
        SdfTree::Line { x1, y1, x2, y2, thickness } => {
            let pad = *thickness as f32 * 0.5;
            (x1.min(*x2) as f32 - pad, y1.min(*y2) as f32 - pad, x1.max(*x2) as f32 + pad, y1.max(*y2) as f32 + pad)
        }
        // Arc uses the full ring's bounds rather than the tighter angular
        // sector — conservative (never under-covers), and the wasted
        // pixels outside the actual sector correctly evaluate as "outside"
        // via `msx_sdf::arc` anyway.
        SdfTree::Ring { cx, cy, r, thickness } | SdfTree::Arc { cx, cy, r, thickness, .. } => {
            circle_bounds(*cx, *cy, r + thickness * 0.5)
        }
        SdfTree::Union(children) => union_bounds(children.iter().map(sdf_bounds)),
        SdfTree::SmoothUnion { children, k } => pad_bounds(union_bounds(children.iter().map(sdf_bounds)), *k as f32),
        SdfTree::Subtract { a, .. } => sdf_bounds(a),
        SdfTree::SmoothSubtract { a, k, .. } => pad_bounds(sdf_bounds(a), *k as f32),
        SdfTree::Intersect { a, b } => intersect_bounds(sdf_bounds(a), sdf_bounds(b)),
        SdfTree::SmoothIntersect { a, b, k } => pad_bounds(intersect_bounds(sdf_bounds(a), sdf_bounds(b)), *k as f32),
        SdfTree::Offset { child, amount } => pad_bounds(sdf_bounds(child), (*amount as f32).max(0.0)),
    }
}

fn circle_bounds(cx: f64, cy: f64, r: f64) -> (f32, f32, f32, f32) {
    ((cx - r) as f32, (cy - r) as f32, (cx + r) as f32, (cy + r) as f32)
}

fn union_bounds(mut iter: impl Iterator<Item = (f32, f32, f32, f32)>) -> (f32, f32, f32, f32) {
    let first = iter.next().unwrap_or((0.0, 0.0, 0.0, 0.0));
    iter.fold(first, |acc, b| (acc.0.min(b.0), acc.1.min(b.1), acc.2.max(b.2), acc.3.max(b.3)))
}

fn intersect_bounds(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3))
}

fn pad_bounds(b: (f32, f32, f32, f32), amount: f32) -> (f32, f32, f32, f32) {
    (b.0 - amount, b.1 - amount, b.2 + amount, b.3 + amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_inside_evaluates_negative() {
        let tree = SdfTree::Circle { cx: 0.0, cy: 0.0, r: 10.0 };
        assert!(evaluate_tree(&tree, (0.0, 0.0)) < 0.0);
        assert!(evaluate_tree(&tree, (50.0, 0.0)) > 0.0);
    }

    #[test]
    fn bounds_cover_a_simple_circle() {
        let tree = SdfTree::Circle { cx: 10.0, cy: 10.0, r: 5.0 };
        assert_eq!(sdf_bounds(&tree), (5.0, 5.0, 15.0, 15.0));
    }

    #[test]
    fn union_bounds_cover_both_children() {
        let a = sdf_bounds(&SdfTree::Circle { cx: 0.0, cy: 0.0, r: 5.0 });
        let b = sdf_bounds(&SdfTree::Circle { cx: 100.0, cy: 0.0, r: 5.0 });
        assert_eq!(union_bounds([a, b].into_iter()), (-5.0, -5.0, 105.0, 5.0));
    }

    #[test]
    fn antialias_is_opaque_well_inside_and_transparent_well_outside() {
        let c = PremulColor::from_straight(1.0, 0.0, 0.0, 1.0);
        assert!((antialiased(c, -5.0).a - 1.0).abs() < 1e-4);
        assert!(antialiased(c, 5.0).a.abs() < 1e-4);
    }
                }
