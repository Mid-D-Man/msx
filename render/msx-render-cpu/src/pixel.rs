// render/msx-render-cpu/src/pixel.rs
//! Tiny helpers for reading/writing a single premultiplied RGBA8 pixel
//! directly in a `tiny_skia::Pixmap`'s raw byte buffer — used by
//! `sdf_raster.rs`/`splat_raster.rs`, which write pixels directly rather
//! than going through `tiny-skia`'s path-fill pipeline.

use msx_render_core::PremulColor;

pub fn read_premul(data: &[u8], idx: usize) -> PremulColor {
    PremulColor {
        r: data[idx] as f32 / 255.0,
        g: data[idx + 1] as f32 / 255.0,
        b: data[idx + 2] as f32 / 255.0,
        a: data[idx + 3] as f32 / 255.0,
    }
}

pub fn write_premul(data: &mut [u8], idx: usize, c: PremulColor) {
    data[idx] = (c.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    data[idx + 1] = (c.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    data[idx + 2] = (c.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    data[idx + 3] = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_bytes() {
        let mut buf = [0u8; 4];
        let c = PremulColor { r: 0.5, g: 0.25, b: 0.75, a: 1.0 };
        write_premul(&mut buf, 0, c);
        let back = read_premul(&buf, 0);
        assert!((back.r - 0.5).abs() < 0.01);
        assert!((back.g - 0.25).abs() < 0.01);
        assert!((back.b - 0.75).abs() < 0.01);
        assert!((back.a - 1.0).abs() < 0.01);
    }
                 }
