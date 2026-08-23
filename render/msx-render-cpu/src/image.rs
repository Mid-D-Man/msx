// render/msx-render-cpu/src/image.rs
//! `Element::Image` rasterization.
//!
//! Decoding here leans entirely on the `image` crate's OWN format
//! sniffing (`image::load_from_memory` figures out PNG vs JPEG from the
//! bytes itself) — unlike `msx-render-svg`, which has to know the format
//! explicitly to build a `data:{mime};base64,...` URI, nothing here ever
//! needs `msx_ast::ImageFormat::sniff` at all. That's the split described
//! in `media.rs`'s own module doc: format detection exists because SVG's
//! `data:` URI needs it, not because every consumer does.
use std::path::Path;

use msx_ast::{Image, Matrix2D, MediaSource};
use tiny_skia::{Pixmap, PixmapPaint, Transform as SkTransform};

pub(crate) fn render_image(pixmap: &mut Pixmap, img: &Image, parent: Matrix2D, base_dir: &Path) {
    let bytes: std::borrow::Cow<[u8]> = match &img.source {
        MediaSource::Embedded(bytes) => std::borrow::Cow::Borrowed(bytes.as_slice()),
        MediaSource::FileRef(path) => {
            // Resolved against `base_dir`, the same convention
            // `msx-render-gpu`'s own `shader_base_dir` already
            // established for `Def::Shader::source_ref` — NOT the
            // process's current working directory, which would silently
            // break the moment this is invoked from anywhere other than
            // wherever the `.msx` file happens to live.
            let full_path = base_dir.join(path);
            match std::fs::read(&full_path) {
                Ok(bytes) => std::borrow::Cow::Owned(bytes),
                Err(e) => {
                    // Same "always paint something sane, never crash the
                    // whole render over one bad reference" principle
                    // every other renderer already follows for a broken
                    // `Def::Shader::source_ref` or a broken `Use::href`.
                    eprintln!("msx-render-cpu: couldn't read image file {}: {e}", full_path.display());
                    return;
                }
            }
        }
    };

    let decoded = match ::image::load_from_memory(&bytes) {
        Ok(d) => d.to_rgba8(),
        Err(e) => {
            eprintln!("msx-render-cpu: couldn't decode image ({} bytes): {e}", bytes.len());
            return;
        }
    };

    let (iw, ih) = decoded.dimensions();
    if iw == 0 || ih == 0 {
        return;
    }

    let Some(mut src) = Pixmap::new(iw, ih) else { return };
    {
        // `image`'s RgbaImage is STRAIGHT alpha; `tiny_skia::Pixmap`
        // always stores PREMULTIPLIED alpha — a byte-for-byte copy here
        // would be silently wrong (translucent pixels would come out too
        // bright/saturated), not just a missed optimization.
        let dst = src.data_mut();
        for (i, px) in decoded.pixels().enumerate() {
            let [r, g, b, a] = px.0;
            let af = a as f32 / 255.0;
            dst[i * 4]     = (r as f32 * af).round() as u8;
            dst[i * 4 + 1] = (g as f32 * af).round() as u8;
            dst[i * 4 + 2] = (b as f32 * af).round() as u8;
            dst[i * 4 + 3] = a;
        }
    }

    // Same `Anchor::top_left_for` helper every renderer calls — see that
    // function's own doc comment for why (so "center"/"bottom_right"/
    // etc. can't drift apart between SVG/CPU/GPU).
    let (tl_x, tl_y) = img.anchor.top_left_for(img.x, img.y, img.width, img.height);
    let sx = img.width / iw as f64;
    let sy = img.height / ih as f64;

    // Working from innermost (applied first, to the raw decoded pixmap)
    // to outermost: scale native pixel size -> target width/height, then
    // position at the anchor-adjusted top-left, then the element's own
    // `transform` (same `combine(parent, local)` pattern every other
    // shape uses — see rasterizer.rs — just with the extra
    // scale+translate stage baked in first, since `draw_pixmap` needs
    // that expressed as a transform rather than baked into path
    // geometry the way `build_rect_path` bakes x/y/width/height in).
    let local = img.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    let placement = Matrix2D::translate(tl_x, tl_y).concat(Matrix2D::scale(sx, sy));
    let combined = parent.concat(local.concat(placement));

    let transform = SkTransform::from_row(
        combined.a as f32, combined.b as f32,
        combined.c as f32, combined.d as f32,
        combined.e as f32, combined.f as f32,
    );

    let paint = PixmapPaint {
        opacity: img.style.opacity.unwrap_or(1.0) as f32,
        ..Default::default()
    };

    pixmap.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::Anchor;

    /// A real, valid, tiny (2x2) PNG — hand-assembled once, reused by
    /// every test below. Encoded through the `image` crate's OWN writer
    /// (already a real dependency of this crate either way) rather than
    /// a hand-typed byte literal, which for PNG specifically (correct
    /// per-chunk CRC32s, zlib-compressed IDAT) would be fragile to get
    /// right by hand and easy to silently get wrong.
    fn tiny_test_png() -> Vec<u8> {
        #[rustfmt::skip]
        let pixels: [u8; 16] = [
            255, 0, 0, 255,   255, 0, 0, 255,
            0,   0, 0, 0,     0,   0, 0, 0,
        ];
        let img = ::image::RgbaImage::from_raw(2, 2, pixels.to_vec()).unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        ::image::DynamicImage::ImageRgba8(img)
            .write_to(&mut bytes, ::image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn embedded_png_renders_opaque_pixels_at_the_expected_position() {
        let mut pixmap = Pixmap::new(20, 20).unwrap();
        let img = Image::new(MediaSource::Embedded(tiny_test_png()), 0.0, 0.0, 10.0, 10.0);

        render_image(&mut pixmap, &img, Matrix2D::identity(), Path::new("."));

        // Top-left 5x5 block (the scaled-up top row of the 2x2 source)
        // should now be opaque red — nearest-neighbor-ish scaling means
        // exact edges can be soft, so this checks well inside the block.
        let px = pixmap.pixel(2, 2).unwrap();
        assert_eq!((px.red(), px.green(), px.blue(), px.alpha()), (255, 0, 0, 255));
    }

    #[test]
    fn transparent_source_pixels_leave_backdrop_untouched() {
        let mut pixmap = Pixmap::new(20, 20).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        let before = pixmap.pixel(2, 8).unwrap();

        let img = Image::new(MediaSource::Embedded(tiny_test_png()), 0.0, 0.0, 10.0, 10.0);
        render_image(&mut pixmap, &img, Matrix2D::identity(), Path::new("."));

        // Bottom half of the source is fully transparent — the backdrop
        // underneath it should survive completely unchanged.
        let after = pixmap.pixel(2, 8).unwrap();
        assert_eq!((before.red(), before.green(), before.blue(), before.alpha()),
                   (after.red(), after.green(), after.blue(), after.alpha()));
    }

    #[test]
    fn opacity_scales_the_composited_alpha() {
        let mut half = Pixmap::new(10, 10).unwrap();
        let mut img = Image::new(MediaSource::Embedded(tiny_test_png()), 0.0, 0.0, 10.0, 10.0);
        img.style.opacity = Some(0.5);
        render_image(&mut half, &img, Matrix2D::identity(), Path::new("."));

        let px = half.pixel(2, 2).unwrap();
        // Premultiplied storage means alpha ~128 AND the color channels
        // scale down together — checking alpha alone is the robust part.
        assert!(px.alpha() > 100 && px.alpha() < 150, "expected roughly-halved alpha, got {}", px.alpha());
    }

    #[test]
    fn center_anchor_positions_the_image_around_its_own_midpoint() {
        let mut a = Pixmap::new(20, 20).unwrap();
        let mut b = Pixmap::new(20, 20).unwrap();

        // TopLeft image at (5,5) sized 10x10 covers the exact same pixels
        // as a Center-anchored image at (10,10) sized 10x10 — same
        // "center anchor offsets by half extent" math already verified
        // directly in msx-ast, confirmed here end-to-end through actual
        // pixel output instead of just the coordinate math in isolation.
        let img_a = Image::new(MediaSource::Embedded(tiny_test_png()), 5.0, 5.0, 10.0, 10.0);
        let img_b = Image::new(MediaSource::Embedded(tiny_test_png()), 10.0, 10.0, 10.0, 10.0).with_anchor(Anchor::Center);

        render_image(&mut a, &img_a, Matrix2D::identity(), Path::new("."));
        render_image(&mut b, &img_b, Matrix2D::identity(), Path::new("."));

        assert_eq!(a.data(), b.data());
    }

    #[test]
    fn missing_file_ref_does_not_panic_and_paints_nothing() {
        let mut pixmap = Pixmap::new(10, 10).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(1, 2, 3, 255));
        let before = pixmap.data().to_vec();

        let img = Image::new(MediaSource::FileRef("does/not/exist.png".to_string()), 0.0, 0.0, 5.0, 5.0);
        render_image(&mut pixmap, &img, Matrix2D::identity(), Path::new("."));

        assert_eq!(pixmap.data(), before.as_slice());
    }

    #[test]
    fn garbage_bytes_do_not_panic_and_paint_nothing() {
        let mut pixmap = Pixmap::new(10, 10).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(1, 2, 3, 255));
        let before = pixmap.data().to_vec();

        let img = Image::new(MediaSource::Embedded(b"not a real image".to_vec()), 0.0, 0.0, 5.0, 5.0);
        render_image(&mut pixmap, &img, Matrix2D::identity(), Path::new("."));

        assert_eq!(pixmap.data(), before.as_slice());
    }
}
