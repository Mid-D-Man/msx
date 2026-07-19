// render/msx-render-cpu/src/splat_raster.rs
//! Per-pixel Gaussian splat rasterization: scan a conservative region
//! around each splat (sized by `msx-splat::effective_radius`) and
//! accumulate its contribution into whatever's already in the pixmap.
//!
//! Splats have no `transform` field in `msx-ast` — their position/shape
//! are already fully expressed by `x,y,sigma_x,sigma_y,rotation` — so only
//! the *ambient* (group/layer) transform applies here.

use msx_ast::{GaussianSplat, Matrix2D};
use rayon::prelude::*;
use tiny_skia::Pixmap;

use crate::geom::{apply_matrix, invert_matrix, transform_bounds};
use crate::pixel::{read_premul, write_premul};
use crate::rasterizer::Defs;
use crate::sdf_raster::paint_to_flat_color;
use msx_render_core::PremulColor;

pub fn rasterize_splat(pixmap: &mut Pixmap, splat: &GaussianSplat, parent: Matrix2D, defs: &Defs) {
    let Some(inv) = invert_matrix(parent) else { return };

    let r = msx_splat::effective_radius(splat, 0.02) as f64;
    let local_bounds = ((splat.x - r) as f32, (splat.y - r) as f32, (splat.x + r) as f32, (splat.y + r) as f32);
    let screen_bounds = transform_bounds(local_bounds, parent);

    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let min_x = (screen_bounds.0.floor() as i32).max(0);
    let min_y = (screen_bounds.1.floor() as i32).max(0);
    let max_x = (screen_bounds.2.ceil() as i32).min(width);
    let max_y = (screen_bounds.3.ceil() as i32).min(height);
    if min_x >= max_x || min_y >= max_y {
        return;
    }

    // `fill`, when set, takes priority over the plain `color` field — same
    // resolution (flat / gradient-average / shader-fallback) every other
    // element's `Paint::Ref` gets here, via the same shared helper
    // `sdf_raster.rs` uses. `color` is still consulted when `fill` is
    // `None` (every splat that existed before this field did, and any
    // that still just use `color` today) — untouched from this function's
    // original, `fill`-less behavior. Deliberately the STRAIGHT resolver,
    // not `paint_to_color`'s premultiplied one — a splat's alpha comes
    // entirely from its own Gaussian falloff below, applied per pixel;
    // folding a paint's own alpha in twice would double-darken the edges.
    let resolved = match &splat.fill {
        Some(paint) => paint_to_flat_color(paint, defs),
        None => splat.color,
    };
    let color = (resolved.r as f32 / 255.0, resolved.g as f32 / 255.0, resolved.b as f32 / 255.0);

    let row_bytes = (width as usize) * 4;
    let row_start = (min_y as usize) * row_bytes;
    let row_end = (max_y as usize) * row_bytes;
    let data = pixmap.data_mut();

    data[row_start..row_end].par_chunks_mut(row_bytes).enumerate().for_each(|(row_offset, row)| {
        let y = min_y + row_offset as i32;
        for x in min_x..max_x {
            let screen_p = (x as f32 + 0.5, y as f32 + 0.5);
            let (lx, ly) = apply_matrix(inv, screen_p);
            let alpha = msx_splat::evaluate_opacity(splat, glam::Vec2::new(lx, ly));
            if alpha <= 1.0 / 255.0 {
                continue;
            }

            let idx = (x as usize) * 4;
            let src = PremulColor::from_straight(color.0, color.1, color.2, alpha.clamp(0.0, 1.0));
            let dst = read_premul(row, idx);
            write_premul(row, idx, src.over(dst));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Color, Def, LinearGradient, Paint, Stop};

    #[test]
    fn opaque_splat_paints_pixels_near_center() {
        let splat = GaussianSplat::circle(50.0, 50.0, 15.0, Color::rgb(255, 0, 0), 1.0);
        let mut pixmap = Pixmap::new(100, 100).unwrap();
        let defs = Defs::build(&[]);
        rasterize_splat(&mut pixmap, &splat, Matrix2D::identity(), &defs);
        let data = pixmap.data();
        let idx = (50 * 100 + 50) * 4;
        assert!(data[idx] > 0);
        assert!(data[idx + 3] > 0);
    }

    #[test]
    fn splat_far_outside_canvas_is_a_no_op() {
        let splat = GaussianSplat::circle(-10_000.0, -10_000.0, 5.0, Color::WHITE, 1.0);
        let mut pixmap = Pixmap::new(10, 10).unwrap();
        let defs = Defs::build(&[]);
        rasterize_splat(&mut pixmap, &splat, Matrix2D::identity(), &defs);
        assert!(pixmap.data().iter().all(|&b| b == 0));
    }

    /// `fill`, when set, must actually change what's painted — not just
    /// parse and decode without touching the render path. Uses a
    /// deliberately high-contrast gradient (opaque red → opaque blue) so
    /// its average-color fallback (the CPU renderer's real behavior for
    /// any gradient ref, see `paint_to_flat_color`'s doc comment) is
    /// visibly distinct from the splat's own plain `color` field (green)
    /// — this fails loudly if `fill` were accidentally ignored and
    /// `color` painted instead.
    #[test]
    fn splat_fill_overrides_the_plain_color_field() {
        let gradient = LinearGradient::new("g".to_string(), 0.0, 0.0, 10.0, 0.0, vec![
            Stop::new(0.0, Color::rgb(255, 0, 0)),
            Stop::new(1.0, Color::rgb(0, 0, 255)),
        ]);
        let defs_vec = vec![Def::LinearGradient(gradient)];
        let defs = Defs::build(&defs_vec);

        let mut splat = GaussianSplat::circle(50.0, 50.0, 15.0, Color::rgb(0, 255, 0), 1.0);
        splat.fill = Some(Paint::Ref("url(#g)".to_string()));

        let mut pixmap = Pixmap::new(100, 100).unwrap();
        rasterize_splat(&mut pixmap, &splat, Matrix2D::identity(), &defs);
        let data = pixmap.data();
        let idx = (50 * 100 + 50) * 4;
        // Average of opaque red (255,0,0) and opaque blue (0,0,255) is
        // (127/128, 0, 127/128) — green (the splat's own `color`) would
        // show a near-zero red/blue and a high green channel instead, the
        // opposite pattern.
        assert!(data[idx] > 80, "red channel should reflect the gradient average, not be near-zero as it would be with the plain green `color`");
        assert!(data[idx + 1] < 40, "green channel should be near-zero — `color` must not be used when `fill` is set");
        assert!(data[idx + 2] > 80, "blue channel should reflect the gradient average, same reasoning as red");
    }
}
