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
use msx_render_core::PremulColor;

pub fn rasterize_splat(pixmap: &mut Pixmap, splat: &GaussianSplat, parent: Matrix2D) {
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

    let color = (splat.color.r as f32 / 255.0, splat.color.g as f32 / 255.0, splat.color.b as f32 / 255.0);

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
    use msx_ast::Color;

    #[test]
    fn opaque_splat_paints_pixels_near_center() {
        let splat = GaussianSplat::circle(50.0, 50.0, 15.0, Color::rgb(255, 0, 0), 1.0);
        let mut pixmap = Pixmap::new(100, 100).unwrap();
        rasterize_splat(&mut pixmap, &splat, Matrix2D::identity());
        let data = pixmap.data();
        let idx = (50 * 100 + 50) * 4;
        assert!(data[idx] > 0);
        assert!(data[idx + 3] > 0);
    }

    #[test]
    fn splat_far_outside_canvas_is_a_no_op() {
        let splat = GaussianSplat::circle(-10_000.0, -10_000.0, 5.0, Color::WHITE, 1.0);
        let mut pixmap = Pixmap::new(10, 10).unwrap();
        rasterize_splat(&mut pixmap, &splat, Matrix2D::identity());
        assert!(pixmap.data().iter().all(|&b| b == 0));
    }
}
