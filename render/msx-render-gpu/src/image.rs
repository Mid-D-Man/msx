// render/msx-render-gpu/src/image.rs
//! `Element::Image` rendering: decode via the `image` crate (same
//! decoder `msx-render-cpu` uses, same auto format-sniffing — this crate
//! doesn't need `msx_ast::ImageFormat::sniff` any more than
//! `msx-render-cpu` does; that only exists for `msx-render-svg`'s `data:`
//! URI MIME type, see `media.rs`'s own module doc), upload as a
//! `Rgba8Unorm` texture (straight alpha, never premultiplied — this
//! crate's OTHER textures, the per-layer offscreen buffers
//! `layer.rs`/`composite.wgsl` sample, ARE premultiplied, which is
//! exactly why they use a different blend state; see `image.wgsl`'s own
//! header comment), position via a real vertex buffer whose four corners
//! are already-transformed clip-space coordinates computed on the CPU —
//! the same "transform on the CPU, upload pre-positioned vertices, no
//! matrix multiply in the vertex shader" approach `vector.rs`'s own
//! `vertex_from_point` already uses, not a uniform-matrix vertex shader.
use std::path::Path;

use wgpu::util::DeviceExt;

use msx_ast::{Element, Image, MediaSource, Matrix2D};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ImageVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

/// Mirrors `layer.rs`'s `CompositeParams` exactly — same field, same
/// 32-byte total size, same reason: WGSL's `vec3<f32>` inside a uniform
/// block needs a 16-byte-aligned start offset, not just its own 12
/// bytes, so the struct rounds up to 32 bytes total on the WGSL side and
/// this Rust type has to match that exact byte count or `create_bind_group`
/// fails a real wgpu validation check — see `CompositeParams`'s own doc
/// comment for the full arithmetic and the real bug this class of
/// mismatch caused there. Reusing the identical, already-debugged shape
/// here instead of re-deriving it.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ImageParams {
    opacity: f32,
    _pad: [f32; 7],
}

pub struct ImagePipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl ImagePipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("msx image shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/image.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("msx image bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("msx image pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("msx image pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    // Straight alpha in, standard source-over blend — see
                    // this crate's own module doc and image.wgsl's header
                    // for why this is deliberately NOT
                    // PREMULTIPLIED_ALPHA_BLENDING (that's `layer.rs`'s
                    // pipeline, for a fundamentally different kind of
                    // source texture). Matches `VectorPipeline`'s own
                    // choice for the same underlying reason: straight,
                    // non-premultiplied color data on the way in.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Linear, not the plain default (which wgpu resolves to Nearest)
        // — unlike `layer.rs`'s composite sampler (source and
        // destination are the same resolution, so filtering barely
        // matters), an image here is routinely scaled to an arbitrary
        // `width`/`height` that has nothing to do with its native pixel
        // size, where Nearest would look visibly blocky.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("msx image sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        ImagePipeline { pipeline, bind_group_layout, sampler }
    }

    /// Draws every `Element::Image` in `elements` (NOT recursive into
    /// `Group`/`Layer` — same contract as `SdfPipeline`/`SplatPipeline`'s
    /// own `draw_all_elements`, called once per contiguous non-Layer run
    /// by `layer::render_ordered`, which is what actually handles
    /// Group/Layer recursion).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_all_elements(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        elements: &[Element],
        transform: Matrix2D,
        canvas: (f32, f32),
        base_dir: &Path,
    ) {
        for el in elements {
            if let Element::Image(img) = el {
                self.draw_one(device, queue, view, img, transform, canvas, base_dir);
            }
        }
    }

    fn draw_one(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        img: &Image,
        parent: Matrix2D,
        canvas: (f32, f32),
        base_dir: &Path,
    ) {
        let bytes: std::borrow::Cow<[u8]> = match &img.source {
            MediaSource::Embedded(bytes) => std::borrow::Cow::Borrowed(bytes.as_slice()),
            MediaSource::FileRef(path) => {
                // Same `base_dir` convention as `shader_base_dir` for
                // `Def::Shader::source_ref`, and `msx-render-cpu`'s own
                // identical `render_image` — not the process's current
                // working directory.
                let full_path = base_dir.join(path);
                match std::fs::read(&full_path) {
                    Ok(bytes) => std::borrow::Cow::Owned(bytes),
                    Err(e) => {
                        eprintln!("msx-render-gpu: couldn't read image file {}: {e}", full_path.display());
                        return;
                    }
                }
            }
        };

        let decoded = match ::image::load_from_memory(&bytes) {
            Ok(d) => d.to_rgba8(),
            Err(e) => {
                eprintln!("msx-render-gpu: couldn't decode image ({} bytes): {e}", bytes.len());
                return;
            }
        };
        let (iw, ih) = decoded.dimensions();
        if iw == 0 || ih == 0 {
            return;
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msx image texture"),
            size: wgpu::Extent3d { width: iw, height: ih, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Straight alpha, not Srgb — this crate doesn't do
            // color-managed blending anywhere else either (Vertex colors
            // and the offscreen render target are both plain
            // `Rgba8Unorm`, treated as raw numbers throughout), so
            // treating the decoded bytes the same way keeps this
            // consistent with every other color value in the pipeline
            // rather than introducing the only gamma-aware texture in
            // the whole renderer.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // `TexelCopyTextureInfo`/`TexelCopyBufferLayout` — wgpu 26's
        // renamed types (see target.rs's own module doc, confirmed
        // against real wgpu 26.0.1 source there), not the older
        // `ImageCopyTexture`/`ImageDataLayout` names.
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &decoded,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * iw),
                rows_per_image: Some(ih),
            },
            wgpu::Extent3d { width: iw, height: ih, depth_or_array_layers: 1 },
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let params = ImageParams { opacity: img.style.opacity.unwrap_or(1.0) as f32, _pad: [0.0; 7] };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx image params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msx image bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: params_buffer.as_entire_binding() },
            ],
        });

        let (vertices, indices) = quad_vertices(img, parent, canvas);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx image vertex buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msx image index buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("msx image encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("msx image pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..6, 0, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

/// The image's four corners, transformed on the CPU into final clip-space
/// positions — mirrors `vector.rs`'s own `apply_matrix`/`to_clip_space`
/// exactly (duplicated here rather than made `pub(crate)` and imported,
/// since it's two small, pure, already-proven-correct functions and this
/// keeps that module's own visibility untouched).
///
/// Local-space corners are the unit square (0,0)..(1,1) — `combined`
/// already folds in the `width`/`height` scale directly (`Matrix2D::
/// scale(img.width, img.height)`), unlike `msx-render-cpu`'s equivalent
/// (`image.rs` there), which scales relative to the DECODED image's own
/// native pixel dimensions because it hands a native-size `Pixmap` to
/// `draw_pixmap`. This function builds geometry directly instead, so
/// there's no "native size" concept to route through at all — the GPU
/// sampler resolves native-texel-to-screen-pixel density on its own from
/// however many UV texels a given triangle's screen-space footprint
/// covers.
///
/// UV corners pair directly with local corners with NO flip — local
/// space already has Y growing downward (matching `width`/`height`
/// throughout this whole crate), which already matches UV's own V-grows-
/// downward convention. This is NOT the same situation as
/// `composite.wgsl`'s hardcoded fullscreen quad, which pairs CLIP-space
/// corners (Y grows upward) with UVs and needs an explicit flip for
/// exactly that reason — clip space and local space differ here, but
/// local space and UV space don't.
fn quad_vertices(img: &Image, parent: Matrix2D, canvas: (f32, f32)) -> ([ImageVertex; 4], [u32; 6]) {
    let (tl_x, tl_y) = img.anchor.top_left_for(img.x, img.y, img.width, img.height);
    let local = img.transform.as_ref().map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
    let placement = Matrix2D::translate(tl_x, tl_y).concat(Matrix2D::scale(img.width, img.height));
    let combined = parent.concat(local.concat(placement));

    let corners: [((f32, f32), (f32, f32)); 4] = [
        ((0.0, 0.0), (0.0, 0.0)), // top-left
        ((1.0, 0.0), (1.0, 0.0)), // top-right
        ((1.0, 1.0), (1.0, 1.0)), // bottom-right
        ((0.0, 1.0), (0.0, 1.0)), // bottom-left
    ];

    let vertices = corners.map(|(local_pos, uv)| {
        let (px, py) = apply_matrix(combined, local_pos);
        ImageVertex { position: to_clip_space(px, py, canvas.0, canvas.1), uv: [uv.0, uv.1] }
    });

    (vertices, [0, 1, 2, 0, 2, 3])
}

fn apply_matrix(m: Matrix2D, p: (f32, f32)) -> (f32, f32) {
    (m.a as f32 * p.0 + m.c as f32 * p.1 + m.e as f32, m.b as f32 * p.0 + m.d as f32 * p.1 + m.f as f32)
}

fn to_clip_space(x: f32, y: f32, width: f32, height: f32) -> [f32; 2] {
    [(x / width) * 2.0 - 1.0, 1.0 - (y / height) * 2.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same reasoning, same test shape, as `layer.rs`'s
    /// `composite_params_matches_the_wgsl_struct_size` — cheap, needs no
    /// GPU adapter, would have caught the exact bug class that test
    /// itself was written to catch.
    #[test]
    fn image_params_matches_the_wgsl_struct_size() {
        assert_eq!(std::mem::size_of::<ImageParams>(), 32);
    }

    #[test]
    fn top_left_image_no_transform_maps_to_the_four_canvas_corners_of_its_own_footprint() {
        let img = Image::new(MediaSource::FileRef("x.png".into()), 10.0, 20.0, 30.0, 40.0);
        let (v, _) = quad_vertices(&img, Matrix2D::identity(), (100.0, 100.0));

        // Pixel-space top-left (10,20) on a 100x100 canvas -> clip (-0.8, 0.6).
        assert!((v[0].position[0] - (-0.8)).abs() < 1e-5);
        assert!((v[0].position[1] - 0.6).abs() < 1e-5);
        assert_eq!(v[0].uv, [0.0, 0.0]);

        // Pixel-space bottom-right (40,60) -> clip (-0.2, -0.2).
        assert!((v[2].position[0] - (-0.2)).abs() < 1e-5);
        assert!((v[2].position[1] - (-0.2)).abs() < 1e-5);
        assert_eq!(v[2].uv, [1.0, 1.0]);
    }

    #[test]
    fn quad_indices_form_two_triangles_covering_all_four_corners() {
        let img = Image::new(MediaSource::FileRef("x.png".into()), 0.0, 0.0, 10.0, 10.0);
        let (_, indices) = quad_vertices(&img, Matrix2D::identity(), (100.0, 100.0));
        let used: std::collections::HashSet<u32> = indices.iter().copied().collect();
        assert_eq!(used, [0, 1, 2, 3].into_iter().collect());
    }
}
