// render/msx-render-gpu/src/target.rs
//! Offscreen render target: a plain `Rgba8Unorm` texture this crate's
//! pipelines draw into, plus the CPU readback path that turns it into the
//! `msx_render_core::RenderTarget` every renderer in this project hands
//! back to its caller.
//!
//! No `Surface` here — that's `apps/msx-viewer`'s concern once it wires up
//! a GPU path; this crate only ever renders off-screen.
//!
//! Every type/field/method name below is confirmed against wgpu 26.0.1's
//! actual source (gfx-rs/wgpu tag `wgpu-v26.0.1`), not assumed — this
//! generation renamed the texture/buffer copy descriptors from
//! `ImageCopyTexture`/`ImageCopyBuffer`/`ImageDataLayout` to
//! `TexelCopyTextureInfo`/`TexelCopyBufferInfo`/`TexelCopyBufferLayout`,
//! and uses `Device::poll(PollType) -> Result<PollStatus, PollError>`
//! rather than the older `Maintain`-based signature.
//!
//! This file didn't exist before this fix — `lib.rs` declared `mod
//! target;` but the file was never committed, so the crate failed to
//! compile at all (`E0583: file not found for module 'target'`) before
//! anything else here even got a chance to run.

use msx_render_core::RenderTarget;

pub struct OffscreenTarget {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl OffscreenTarget {
    /// `usage` is the caller's responsibility, not a hardcoded default —
    /// this constructor is shared by two genuinely different needs:
    /// `lib.rs`'s top-level scene target only ever gets rendered into and
    /// then copied out for CPU readback (`RENDER_ATTACHMENT | COPY_SRC`),
    /// while `layer.rs`'s per-layer buffer additionally needs to be
    /// *sampled* by the composite shader afterward (`TEXTURE_BINDING` on
    /// top of that). A single hardcoded usage value here previously
    /// covered only the first case — layer compositing failed with a real
    /// wgpu validation error the moment it actually ran against a real
    /// adapter for the first time ("Usage flags ... do not contain
    /// required usage flags TextureUsages(TEXTURE_BINDING)"), since
    /// nothing about the *type* of a missing usage flag is wrong, only
    /// its runtime validity for a specific later operation — exactly the
    /// kind of gap only a real GPU adapter's driver can catch, no matter
    /// how carefully the surrounding Rust is type-checked.
    pub fn new(device: &wgpu::Device, width: u32, height: u32, usage: wgpu::TextureUsages) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msx offscreen target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        OffscreenTarget { texture, view, width, height }
    }

    /// Exposes the backing texture, not just `view` — needed by
    /// `layer.rs`'s non-Normal blend-mode path, which has to
    /// `copy_texture_to_texture` this target's CURRENT contents into a
    /// separate, readable "backdrop" texture before drawing into it
    /// again (a render pass can't sample the texture it's currently
    /// writing to — no framebuffer-fetch in WebGPU — so the backdrop the
    /// blend shader reads has to be a real, separate copy). A `TextureView`
    /// alone can't be the source of a texture-to-texture copy;
    /// `copy_texture_to_texture` needs the actual `Texture` handle on
    /// both ends. The field stays private — this is the one place
    /// outside this struct that needs it, not a reason to make it `pub`
    /// outright.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Copies the texture to a CPU-readable buffer, blocks until the copy
    /// completes, and unpacks it into a `RenderTarget`.
    ///
    /// wgpu requires `bytes_per_row` in a texture↔buffer copy to be a
    /// multiple of `COPY_BYTES_PER_ROW_ALIGNMENT` (256). Our actual row
    /// width (`width * 4` for Rgba8) usually isn't a multiple of that, so
    /// the staging buffer is allocated padded, and each row is copied out
    /// separately — dropping the padding — into the tightly-packed
    /// `RenderTarget` buffer underneath.
    pub fn read_back(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> RenderTarget {
        let unpadded_bytes_per_row = self.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let buffer_size = u64::from(padded_bytes_per_row) * u64::from(self.height);
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("msx offscreen readback buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx offscreen readback encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Standard synchronous wgpu readback idiom: map_async fires a
        // callback rather than returning a future directly, so a channel
        // bridges it back to this blocking call — paired with
        // `Device::poll(PollType::Wait)`, which is what actually drives
        // that callback to fire (without it, the channel would just hang).
        let (tx, rx) = std::sync::mpsc::channel();
        staging_buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device.poll(wgpu::PollType::Wait).expect("device.poll failed during GPU readback");
        rx.recv().expect("map_async callback never fired").expect("buffer mapping failed");

        let mut target = RenderTarget::new(self.width, self.height);
        {
            let mapped = staging_buffer.slice(..).get_mapped_range();
            let dst = target.as_bytes_mut();
            for row in 0..self.height as usize {
                let src_start = row * padded_bytes_per_row as usize;
                let src_end = src_start + unpadded_bytes_per_row as usize;
                let dst_start = row * unpadded_bytes_per_row as usize;
                let dst_end = dst_start + unpadded_bytes_per_row as usize;
                dst[dst_start..dst_end].copy_from_slice(&mapped[src_start..src_end]);
            }
        }
        staging_buffer.unmap();

        target
    }
}
