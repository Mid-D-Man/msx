// render/msx-render-cpu/src/effects.rs
//! CPU post-process passes for `Layer` effects, operating directly on a
//! `tiny_skia::Pixmap`'s premultiplied byte buffer.
//!
//! `gaussian_blur` follows the SVG definition of `feGaussianBlur`: three box
//! blurs of the width the specification gives for a standard deviation of 2 or
//! more, and a true Gaussian kernel below that. The radius of an effect is that
//! standard deviation, in pixels, with transparent pixels outside the buffer.
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

/// Gaussian blur with standard deviation `sigma` in pixels, the way the SVG
/// specification describes it: three box blurs for `sigma` of 2 and more, a true
/// Gaussian kernel below that. Pixels outside the buffer count as transparent,
/// and the values stay in `f32` until the end so faint sources survive.
pub fn gaussian_blur(pixmap: &mut Pixmap, sigma: f32) {
    if sigma.is_nan() || sigma <= 0.0 {
        return;
    }
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    let mut buf: Vec<f32> = pixmap.data().iter().map(|&b| b as f32).collect();

    blur_rows(&mut buf, width, sigma);
    let mut turned = transpose(&buf, width, height);
    blur_rows(&mut turned, height, sigma);
    let buf = transpose(&turned, height, width);

    for (dst, &src) in pixmap.data_mut().iter_mut().zip(buf.iter()) {
        *dst = src.round().clamp(0.0, 255.0) as u8;
    }
}

/// Blurs every row of an RGBA `f32` buffer that is `width` pixels wide.
fn blur_rows(buf: &mut [f32], width: usize, sigma: f32) {
    if width == 0 {
        return;
    }
    buf.par_chunks_mut(width * 4).for_each(|row| blur_line(row, width, sigma));
}

fn transpose(src: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let from = (y * width + x) * 4;
            let to = (x * height + y) * 4;
            out[to..to + 4].copy_from_slice(&src[from..from + 4]);
        }
    }
    out
}

fn blur_line(row: &mut [f32], n: usize, sigma: f32) {
    let mut tmp = vec![0.0f32; row.len()];
    if sigma < 2.0 {
        let radius = (sigma * 3.0).ceil() as i32;
        let weights: Vec<f32> = (-radius..=radius).map(|k| (-((k * k) as f32) / (2.0 * sigma * sigma)).exp()).collect();
        let total: f32 = weights.iter().sum();
        for x in 0..n as i32 {
            let mut sum = [0.0f32; 4];
            for (k, w) in (-radius..=radius).zip(&weights) {
                let sx = x + k;
                if sx < 0 || sx >= n as i32 {
                    continue;
                }
                let idx = sx as usize * 4;
                for c in 0..4 {
                    sum[c] += row[idx + c] * w;
                }
            }
            let out = x as usize * 4;
            for c in 0..4 {
                tmp[out + c] = sum[c] / total;
            }
        }
        row.copy_from_slice(&tmp);
        return;
    }

    // The SVG box blur: d is the box width, odd boxes are centered, even ones
    // alternate left and right with a centered box one wider in between.
    let d = (sigma * 1.879_971_2 + 0.5).floor().max(1.0) as usize;
    let passes: [(usize, usize); 3] = if d % 2 == 1 {
        [((d - 1) / 2, (d - 1) / 2); 3]
    } else {
        [(d / 2, d / 2 - 1), (d / 2 - 1, d / 2), (d / 2, d / 2)]
    };
    for (left, right) in passes {
        box_pass(row, &mut tmp, n, left, right);
        row.copy_from_slice(&tmp);
    }
}

/// One box blur over a line: each pixel becomes the mean of the window from
/// `left` pixels before it to `right` pixels after it, with transparent
/// pixels outside the line. A running sum keeps it linear in the line length.
fn box_pass(src: &[f32], dst: &mut [f32], n: usize, left: usize, right: usize) {
    let size = (left + right + 1) as f32;
    for c in 0..4 {
        let mut sum = 0.0f32;
        for k in 0..=right.min(n.saturating_sub(1)) {
            sum += src[k * 4 + c];
        }
        for x in 0..n {
            dst[x * 4 + c] = sum / size;
            let add = x + right + 1;
            if add < n {
                sum += src[add * 4 + c];
            }
            if x >= left {
                sum -= src[(x - left) * 4 + c];
            }
        }
    }
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
        // Same `(row * width + col) * 4` pixel-index formula as the sibling
        // tests above — row happens to be 1 here, which trips clippy's
        // identity_op lint on `1 * 30`; kept as-is for consistency with them.
        #[allow(clippy::identity_op)]
        let outside_idx = (1 * 30 + 1) * 4;
        assert_eq!(data[outside_idx + 3], 0);
    }

    fn alpha_at(p: &Pixmap, x: usize, y: usize) -> u8 {
        p.data()[(y * p.width() as usize + x) * 4 + 3]
    }

    /// Left half opaque, right half empty, `at` the column of the edge.
    fn half_plane(size: usize, edge: usize) -> Pixmap {
        let mut p = Pixmap::new(size as u32, size as u32).unwrap();
        for y in 0..size {
            for x in 0..edge {
                solid_pixel(&mut p, x, y, [255, 255, 255, 255]);
            }
        }
        p
    }

    /// Standard normal upper tail, 1 - phi(z), by the Abramowitz and Stegun
    /// approximation, scaled to 0..255.
    fn tail(z: f32) -> f32 {
        let t = 1.0 / (1.0 + 0.231_641_9 * z.abs());
        let poly = t * (0.319_381_54 + t * (-0.356_563_78 + t * (1.781_478 + t * (-1.821_256 + t * 1.330_274_5))));
        let upper = (-(z * z) / 2.0).exp() / (2.0 * std::f32::consts::PI).sqrt() * poly;
        255.0 * if z >= 0.0 { upper } else { 1.0 - upper }
    }

    #[test]
    fn box_blur_edge_follows_the_gaussian_for_large_sigma() {
        let mut p = half_plane(80, 40);
        gaussian_blur(&mut p, 4.0);
        // Pixel centers sit half a pixel from the edge between columns 39 and 40.
        for (x, dist) in [(36usize, -3.5f32), (39, -0.5), (40, 0.5), (44, 4.5)] {
            let want = tail(dist / 4.0);
            let got = alpha_at(&p, x, 40) as f32;
            assert!((got - want).abs() < 9.0, "x={x}: got {got}, want about {want}");
        }
    }

    #[test]
    fn kernel_blur_edge_follows_the_gaussian_for_small_sigma() {
        let mut p = half_plane(40, 20);
        gaussian_blur(&mut p, 1.0);
        for (x, dist) in [(18usize, -1.5f32), (19, -0.5), (20, 0.5), (21, 1.5)] {
            let want = tail(dist);
            let got = alpha_at(&p, x, 20) as f32;
            assert!((got - want).abs() < 9.0, "x={x}: got {got}, want about {want}");
        }
    }

    #[test]
    fn blur_keeps_the_total_alpha_of_a_shape_away_from_the_border() {
        let mut p = Pixmap::new(60, 60).unwrap();
        for y in 25..35usize {
            for x in 25..35usize {
                solid_pixel(&mut p, x, y, [255, 255, 255, 255]);
            }
        }
        let before: u32 = p.data().chunks_exact(4).map(|px| px[3] as u32).sum();
        gaussian_blur(&mut p, 3.0);
        let after: u32 = p.data().chunks_exact(4).map(|px| px[3] as u32).sum();
        assert!((after as f32 - before as f32).abs() / (before as f32) < 0.02, "{before} -> {after}");
    }

    #[test]
    fn pixels_outside_the_buffer_count_as_transparent() {
        let mut p = Pixmap::new(40, 40).unwrap();
        p.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        gaussian_blur(&mut p, 4.0);
        let edge = alpha_at(&p, 0, 20);
        assert!((100..160).contains(&edge), "an edge pixel of a filled buffer is about half covered, got {edge}");
        assert_eq!(alpha_at(&p, 20, 20), 255);
    }

    #[test]
    fn a_huge_sigma_empties_the_buffer_and_stays_fast() {
        let mut p = Pixmap::new(200, 200).unwrap();
        for y in 20..180usize {
            for x in 20..180usize {
                solid_pixel(&mut p, x, y, [46, 139, 87, 255]);
            }
        }
        let start = std::time::Instant::now();
        gaussian_blur(&mut p, 1000.0);
        assert!(start.elapsed().as_secs() < 5);
        assert!(p.data().chunks_exact(4).all(|px| px[3] <= 2));
    }

    #[test]
    fn a_tiny_sigma_changes_nothing() {
        let mut p = half_plane(20, 10);
        let before = p.data().to_vec();
        gaussian_blur(&mut p, 0.01);
        assert_eq!(p.data(), before.as_slice());
    }
}
