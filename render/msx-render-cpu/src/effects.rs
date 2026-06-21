// render/msx-render-cpu/src/effects.rs
//! CPU post-process passes for `Layer` effects, operating directly on a
//! `tiny_skia::Pixmap`'s premultiplied byte buffer.
//!
//! `gaussian_blur` is 3 passes of a separable box blur — a standard,
//! widely-used approximation (the central limit theorem says repeated box
//! convolution converges toward a Gaussian shape). The per-pass radius
//! below is a commonly-cited approximation, not an exact variance match —
//! good enough for a CPU-fallback blur, not meant to be bit-identical to a
//! true Gaussian kernel.
//!
//! `drop_shadow`/`outer_glow` build a same-size offscreen buffer, flood it
//! with the effect's tint wherever the source has alpha, blur it, then
//! composite the *original* source back on top (so the effect reads as
//! "behind" the shape). `inner_glow`/`inner_shadow` invert the alpha first
//! (so the effect grows in *from* the edges instead of out), then clip the
//! result back to the original silhouette and merge it *over* the source
//! (so it stays visible despite the shape's own opacity) — the same
//! mechanism the SVG filter version of these effects (`msx-render-svg`)
//! uses, just on raw pixels instead of `<feComponentTransfer>`/`<feComposite>`.

use msx_ast::{Color, Effect};
use rayon::prelude::*;
use tiny_skia::Pixmap;

use crate::pixel::{read_premul, write_premul};
use msx_render_core::PremulColor;

pub fn apply_effects(pixmap: &mut Pixmap, effects: &[Effect]) {
    for effect in effects {
        match effect {
            Effect::Blur { radius } => gaussian_blur(pixmap, *radius as f32),
            Effect::DropShadow { offset_x, offset_y, blur_radius, color, opacity } => {
                drop_shadow(pixmap, *offset_x as f32, *offset_y as f32, *blur_radius as f32, *color, *opacity as f32);
            }
            Effect::InnerShadow { offset_x, offset_y, blur_radius, color, opacity } => {
                inner_shadow(pixmap, *offset_x as f32, *offset_y as f32, *blur_radius as f32, *color, *opacity as f32);
            }
            Effect::OuterGlow { color, blur_radius, spread, opacity } => {
                outer_glow(pixmap, *color, *blur_radius as f32, *spread as f32, *opacity as f32);
            }
            Effect::InnerGlow { color, blur_radius, opacity } => {
                inner_glow(pixmap, *color, *blur_radius as f32, *opacity as f32);
            }
        }
    }
}

// ── Blur ─────────────────────────────────────────────────────────────────────

pub fn gaussian_blur(pixmap: &mut Pixmap, sigma: f32) {
    if sigma <= 0.0 {
        return;
    }
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    let radius = ((sigma * 1.88) as i32).max(1);

    let mut buf_a = pixmap.data().to_vec();
    let mut buf_b = vec![0u8; buf_a.len()];

    for _ in 0..3 {
        box_blur_horizontal(&buf_a, &mut buf_b, width, height, radius);
        box_blur_vertical(&buf_b, &mut buf_a, width, height, radius);
    }

    pixmap.data_mut().copy_from_slice(&buf_a);
}

fn box_blur_horizontal(src: &[u8], dst: &mut [u8], width: usize, height: usize, radius: i32) {
    if radius <= 0 {
        dst.copy_from_slice(src);
        return;
    }
    let row_bytes = width * 4;
    dst.par_chunks_mut(row_bytes).enumerate().for_each(|(y, dst_row)| {
        let src_row = &src[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..width {
            let mut sum = [0i64; 4];
            let mut count = 0i64;
            for dx in -radius..=radius {
                let sx = (x as i32 + dx).clamp(0, width as i32 - 1) as usize;
                let idx = sx * 4;
                for c in 0..4 {
                    sum[c] += src_row[idx + c] as i64;
                }
                count += 1;
            }
            let out_idx = x * 4;
            for c in 0..4 {
                dst_row[out_idx + c] = (sum[c] / count).clamp(0, 255) as u8;
            }
        }
    });
}

fn box_blur_vertical(src: &[u8], dst: &mut [u8], width: usize, height: usize, radius: i32) {
    if radius <= 0 {
        dst.copy_from_slice(src);
        return;
    }
    let row_bytes = width * 4;
    dst.par_chunks_mut(row_bytes).enumerate().for_each(|(y, dst_row)| {
        for x in 0..width {
            let mut sum = [0i64; 4];
            let mut count = 0i64;
            for dy in -radius..=radius {
                let sy = (y as i32 + dy).clamp(0, height as i32 - 1) as usize;
                let idx = (sy * width + x) * 4;
                for c in 0..4 {
                    sum[c] += src[idx + c] as i64;
                }
                count += 1;
            }
            let out_idx = x * 4;
            for c in 0..4 {
                dst_row[out_idx + c] = (sum[c] / count).clamp(0, 255) as u8;
            }
        }
    });
}

// ── Morphology (for OuterGlow's `spread`) ───────────────────────────────────

/// Max-alpha dilation within `radius` — O(width·height·radius²), fine for
/// the small `spread` values these effects realistically use; not fine for
/// a large radius, which isn't a use case this effect targets anyway.
fn dilate(pixmap: &mut Pixmap, radius: i32) {
    if radius <= 0 {
        return;
    }
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    let src = pixmap.data().to_vec();
    let dst = pixmap.data_mut();

    for y in 0..height {
        for x in 0..width {
            let mut best: Option<(u8, u8, u8, u8)> = None;
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= width || sy >= height {
                        continue;
                    }
                    let idx = ((sy * width + sx) * 4) as usize;
                    let a = src[idx + 3];
                    if best.map(|(_, _, _, ba)| a > ba).unwrap_or(true) {
                        best = Some((src[idx], src[idx + 1], src[idx + 2], a));
                    }
                }
            }
            if let Some((r, g, b, a)) = best {
                let idx = ((y * width + x) * 4) as usize;
                dst[idx] = r;
                dst[idx + 1] = g;
                dst[idx + 2] = b;
                dst[idx + 3] = a;
            }
        }
    }
}

// ── Compositing helper ───────────────────────────────────────────────────────

/// Writes "`src` over `dst`" into `dst`, in place.
fn composite_source_over(dst: &mut Pixmap, src: &Pixmap) {
    let src_data = src.data();
    let dst_data = dst.data_mut();
    for (d, s) in dst_data.chunks_mut(4).zip(src_data.chunks(4)) {
        let backdrop = read_premul(d, 0);
        let source = read_premul(s, 0);
        write_premul(d, 0, source.over(backdrop));
    }
}

// ── Drop shadow / outer glow (effect renders BEHIND the source) ────────────

pub fn drop_shadow(pixmap: &mut Pixmap, offset_x: f32, offset_y: f32, blur_radius: f32, color: Color, opacity: f32) {
    let width = pixmap.width();
    let height = pixmap.height();
    let Some(mut shadow) = Pixmap::new(width, height) else { return };

    let tint = (color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0);
    let op = opacity.clamp(0.0, 1.0);

    {
        let src = pixmap.data();
        let dst = shadow.data_mut();
        let (w, h) = (width as i32, height as i32);
        let dx = offset_x.round() as i32;
        let dy = offset_y.round() as i32;

        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = (x - dx, y - dy);
                if sx < 0 || sy < 0 || sx >= w || sy >= h {
                    continue;
                }
                let src_idx = ((sy * w + sx) * 4) as usize;
                let src_alpha = src[src_idx + 3] as f32 / 255.0;
                if src_alpha <= 0.0 {
                    continue;
                }
                let idx = ((y * w + x) * 4) as usize;
                write_premul(dst, idx, PremulColor::from_straight(tint.0, tint.1, tint.2, src_alpha * op));
            }
        }
    }

    gaussian_blur(&mut shadow, blur_radius.max(0.0));
    composite_source_over(&mut shadow, pixmap); // source over shadow, into shadow
    *pixmap = shadow;
}

pub fn outer_glow(pixmap: &mut Pixmap, color: Color, blur_radius: f32, spread: f32, opacity: f32) {
    let width = pixmap.width();
    let height = pixmap.height();
    let Some(mut glow) = Pixmap::new(width, height) else { return };

    let tint = (color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0);
    let op = opacity.clamp(0.0, 1.0);

    {
        let src = pixmap.data();
        let dst = glow.data_mut();
        for (d, s) in dst.chunks_mut(4).zip(src.chunks(4)) {
            let src_alpha = s[3] as f32 / 255.0;
            if src_alpha > 0.0 {
                write_premul(d, 0, PremulColor::from_straight(tint.0, tint.1, tint.2, src_alpha * op));
            }
        }
    }

    if spread > 0.0 {
        dilate(&mut glow, spread.round() as i32);
    }
    gaussian_blur(&mut glow, blur_radius.max(0.0));

    composite_source_over(&mut glow, pixmap); // source over glow, into glow
    *pixmap = glow;
}

// ── Inner glow / inner shadow (effect renders OVER the source, clipped) ────

pub fn inner_glow(pixmap: &mut Pixmap, color: Color, blur_radius: f32, opacity: f32) {
    let width = pixmap.width();
    let height = pixmap.height();
    let Some(mut mask) = Pixmap::new(width, height) else { return };

    {
        let src = pixmap.data();
        let dst = mask.data_mut();
        for (d, s) in dst.chunks_mut(4).zip(src.chunks(4)) {
            let inv_alpha = 1.0 - (s[3] as f32 / 255.0);
            write_premul(d, 0, PremulColor::from_straight(1.0, 1.0, 1.0, inv_alpha));
        }
    }

    gaussian_blur(&mut mask, blur_radius.max(0.0));

    let tint = (color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0);
    let op = opacity.clamp(0.0, 1.0);
    let src_alpha: Vec<u8> = pixmap.data().chunks(4).map(|p| p[3]).collect();

    {
        let mask_data = mask.data_mut();
        for (i, chunk) in mask_data.chunks_mut(4).enumerate() {
            let glow_strength = chunk[3] as f32 / 255.0;
            let clip_alpha = src_alpha[i] as f32 / 255.0;
            let final_alpha = glow_strength * clip_alpha * op;
            write_premul(chunk, 0, PremulColor::from_straight(tint.0, tint.1, tint.2, final_alpha));
        }
    }

    composite_source_over(pixmap, &mask); // glow over the original source, into pixmap directly
}

pub fn inner_shadow(pixmap: &mut Pixmap, offset_x: f32, offset_y: f32, blur_radius: f32, color: Color, opacity: f32) {
    let width = pixmap.width();
    let height = pixmap.height();
    let Some(mut inv_mask) = Pixmap::new(width, height) else { return };

    {
        let src = pixmap.data();
        let dst = inv_mask.data_mut();
        for (d, s) in dst.chunks_mut(4).zip(src.chunks(4)) {
            let inv_alpha = 1.0 - (s[3] as f32 / 255.0);
            write_premul(d, 0, PremulColor::from_straight(1.0, 1.0, 1.0, inv_alpha));
        }
    }

    let Some(mut offset_mask) = Pixmap::new(width, height) else { return };
    {
        let src = inv_mask.data();
        let dst = offset_mask.data_mut();
        let (w, h) = (width as i32, height as i32);
        let dx = offset_x.round() as i32;
        let dy = offset_y.round() as i32;
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = (x - dx, y - dy);
                if sx < 0 || sy < 0 || sx >= w || sy >= h {
                    continue;
                }
                let src_idx = ((sy * w + sx) * 4) as usize;
                let dst_idx = ((y * w + x) * 4) as usize;
                dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
            }
        }
    }

    gaussian_blur(&mut offset_mask, blur_radius.max(0.0));

    let tint = (color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0);
    let op = opacity.clamp(0.0, 1.0);
    let src_alpha: Vec<u8> = pixmap.data().chunks(4).map(|p| p[3]).collect();

    {
        let mask_data = offset_mask.data_mut();
        for (i, chunk) in mask_data.chunks_mut(4).enumerate() {
            let shadow_strength = chunk[3] as f32 / 255.0;
            let clip_alpha = src_alpha[i] as f32 / 255.0;
            let final_alpha = shadow_strength * clip_alpha * op;
            write_premul(chunk, 0, PremulColor::from_straight(tint.0, tint.1, tint.2, final_alpha));
        }
    }

    composite_source_over(pixmap, &offset_mask);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_pixel(p: &mut Pixmap, x: usize, y: usize, color: [u8; 4]) {
        let w = p.width() as usize;
        let idx = (y * w + x) * 4;
        p.data_mut()[idx..idx + 4].copy_from_slice(&color);
    }

    #[test]
    fn blur_with_zero_sigma_is_a_no_op() {
        let mut p = Pixmap::new(10, 10).unwrap();
        p.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let before = p.data().to_vec();
        gaussian_blur(&mut p, 0.0);
        assert_eq!(p.data(), before.as_slice());
    }

    #[test]
    fn blur_softens_a_hard_edge() {
        let mut p = Pixmap::new(20, 20).unwrap();
        for y in 0..20usize {
            for x in 0..10usize {
                solid_pixel(&mut p, x, y, [255, 0, 0, 255]);
            }
        }
        gaussian_blur(&mut p, 3.0);
        let data = p.data();
        let boundary_idx = (10 * 20 + 10) * 4;
        let alpha = data[boundary_idx + 3];
        assert!(alpha > 0 && alpha < 255, "expected a soft edge, got alpha={alpha}");
    }

    #[test]
    fn drop_shadow_extends_alpha_beyond_the_original_shape() {
        let mut p = Pixmap::new(40, 40).unwrap();
        solid_pixel(&mut p, 20, 20, [0, 0, 255, 255]);
        drop_shadow(&mut p, 5.0, 5.0, 3.0, Color::BLACK, 0.8);
        let data = p.data();
        let shadow_idx = (24 * 40 + 24) * 4;
        assert!(data[shadow_idx + 3] > 0);
    }

    #[test]
    fn inner_glow_stays_within_the_original_silhouette() {
        let mut p = Pixmap::new(30, 30).unwrap();
        for y in 5..25usize {
            for x in 5..25usize {
                solid_pixel(&mut p, x, y, [50, 50, 50, 255]);
            }
        }
        inner_glow(&mut p, Color::WHITE, 4.0, 1.0);
        let data = p.data();
        // Outside the original silhouette must still be fully transparent
        // — the glow is clipped, not spilling outward.
        let outside_idx = (1 * 30 + 1) * 4;
        assert_eq!(data[outside_idx + 3], 0);
    }
  }
