// render/msx-render-core/src/tile.rs
//! The pixel buffer every renderer backend draws into, plus the tiling
//! split `msx-render-cpu` hands out across `rayon`'s thread pool.

#[derive(Debug, Clone)]
pub struct RenderTarget {
    pub width: u32,
    pub height: u32,
    pixels: Vec<u8>, // RGBA8, width * height * 4 bytes
}

impl RenderTarget {
    pub fn new(width: u32, height: u32) -> Self {
        RenderTarget { width, height, pixels: vec![0u8; (width as usize) * (height as usize) * 4] }
    }

    // `as_chunks_mut::<4>()` (clippy's own suggestion here) is a nicer,
    // compile-time-checked way to express this — but it's still gated
    // behind the unstable `slice_as_chunks` feature at this workspace's
    // rustc 1.75 floor (confirmed directly: `v.as_chunks_mut::<4>()`
    // fails with `error[E0658]: use of unstable library feature`).
    // `chunks_exact_mut` has been stable since 1.31 and is exactly as
    // correct here; this lint is a newer, pedantic style preference from
    // a clippy version well ahead of what this crate can actually
    // require yet, not a real issue with the code.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    pub fn fill(&mut self, color: [u8; 4]) {
        for px in self.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&color);
        }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = self.index_of(x, y);
        self.pixels[idx..idx + 4].copy_from_slice(&color);
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let idx = self.index_of(x, y);
        [self.pixels[idx], self.pixels[idx + 1], self.pixels[idx + 2], self.pixels[idx + 3]]
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.pixels
    }

    /// Non-overlapping horizontal bands covering the whole target, sized
    /// for handing one band to each `rayon` worker.
    pub fn tile_rows(&self, rows_per_tile: u32) -> Vec<TileRegion> {
        let rows_per_tile = rows_per_tile.max(1);
        let mut tiles = Vec::new();
        let mut y = 0;
        while y < self.height {
            let h = rows_per_tile.min(self.height - y);
            tiles.push(TileRegion { x: 0, y, width: self.width, height: h });
            y += h;
        }
        tiles
    }

    #[inline]
    fn index_of(&self, x: u32, y: u32) -> usize {
        ((y * self.width + x) * 4) as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl TileRegion {
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_target_is_zeroed() {
        let t = RenderTarget::new(4, 4);
        assert_eq!(t.get_pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(t.as_bytes().len(), 4 * 4 * 4);
    }

    #[test]
    fn set_and_get_pixel_roundtrip() {
        let mut t = RenderTarget::new(4, 4);
        t.set_pixel(2, 1, [255, 0, 128, 255]);
        assert_eq!(t.get_pixel(2, 1), [255, 0, 128, 255]);
        assert_eq!(t.get_pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn out_of_bounds_set_is_ignored_not_panicking() {
        let mut t = RenderTarget::new(2, 2);
        t.set_pixel(50, 50, [1, 2, 3, 4]);
        assert_eq!(t.get_pixel(50, 50), [0, 0, 0, 0]);
    }

    #[test]
    fn fill_sets_every_pixel() {
        let mut t = RenderTarget::new(3, 3);
        t.fill([10, 20, 30, 255]);
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(t.get_pixel(x, y), [10, 20, 30, 255]);
            }
        }
    }

    #[test]
    fn tile_rows_covers_full_height_without_overlap() {
        let t = RenderTarget::new(10, 10);
        let tiles = t.tile_rows(4);
        assert_eq!(tiles.len(), 3); // 4 + 4 + 2
        assert_eq!(tiles[0].y, 0);
        assert_eq!(tiles[0].height, 4);
        assert_eq!(tiles[2].y, 8);
        assert_eq!(tiles[2].height, 2);
    }
      }
