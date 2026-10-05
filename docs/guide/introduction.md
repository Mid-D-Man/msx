# Introduction

MSX (MidStroke eXtension) is a vector graphics format with two layers. The source layer is a DixScript file that people write and read. The binary layer is a compact encoding of the evaluated scene, built for moving graphics between tools. Both layers describe the same thing: a canvas, a list of elements, optional defs such as gradients and shaders, and optional keyframe animation.

## Why a source format built on DixScript

SVG is XML, and complex or repetitive graphics in XML are written by tooling rather than by hand. An MSX source file is a DixScript file, the same format used for project configuration. DixScript brings functions (QuickFuncs), enums and string interpolation to the file, so a scene can generate its own shapes.

| SVG | MSX source |
|---|---|
| The same `<circle>` is copied fifty times | One QuickFunc is defined and called fifty times |
| Reusable components need script or a build step | QuickFuncs compose and are evaluated when the file is loaded |
| Gradient and clip ids are kept unique by hand | Defs are plain objects, so ids are data |
| Verbose text, no compression of its own | A typed binary form that MBFA compresses further |

## The two layers

The **source layer** is what authors edit. It is evaluated by the DixScript runtime first, so QuickFunc calls and interpolated strings are already resolved before MSX reads anything.

The **binary layer** is a typed stream: a 32 byte header, a string pool, a def section, an element stream and an optional animation section. A compiled file is self-contained. It renders to SVG or PNG without the original source, and every `msx` command accepts either form. The [binary format](binary-format.md) page describes the layout.

## From source to pixels

A scene moves through the same stages for every renderer:

1. `msx-parser` evaluates the DixScript source into a `Scene`, the in-memory tree defined by `msx-ast`.
2. For an animated scene, `msx-anim` resolves the keyframes at one point in time and returns a static `Scene`.
3. A renderer turns the scene into output: SVG text, a CPU raster, or a GPU raster.
4. Separately, `msx-binary` converts a `Scene` to the compact binary form and back.

## What MSX adds beyond SVG

- **Signed distance field shapes**: shape trees with smooth boolean operations, evaluated per pixel. See [Signed distance fields](sdf.md).
- **Gaussian splats**: soft elliptical blobs. See [Gaussian splats](splats.md).
- **Layers**: isolated compositing buffers with blend modes, effects and paint order. See [Layers](layers.md).
- **Shader fills**: WGSL fragment shaders as paints, executed by the GPU renderer. See [Shaders](shaders.md).
- **Keyframe animation**: tracks over translate, scale, rotate, opacity and layer order. See [Keyframe animation](animation.md).
- **Embedded media**: images in the element tree and audio as a def. See [Images and audio](images-and-audio.md).

## The workspace

MSX is a multi-crate Rust workspace.

| Crate | Role |
|---|---|
| `core/msx-ast` | Element tree, paints, defs, transforms and animation data types |
| `core/msx-anim` | Keyframe timeline resolution |
| `core/msx-parser` | DixScript to `Scene` |
| `core/msx-binary` | `Scene` to and from the compact binary, with optional MBFA compression |
| `render/msx-render-svg` | `Scene` to SVG |
| `render/msx-render-cpu` | `Scene` to a PNG raster on the CPU |
| `render/msx-render-gpu` | `Scene` to a PNG or GIF through wgpu, behind the `gpu` Cargo feature |
| `primitives/msx-sdf` | Signed distance field evaluation |
| `primitives/msx-splat` | Gaussian splat evaluation and compositing |
| `apps/msx-cli` | The `msx` command line tool |
| `apps/msx-viewer` | A native window viewer |
| `web/msx-wasm` | The WebAssembly build behind the playground |

## Scope

MSX is a source format for parametric vector graphics, a compact interchange format that includes animation data, and a compression target for MBFA. It is not a raster format. SVG is an export target, not the runtime format. Pattern fills, clip paths and masks (beyond a layer's own `clip` flag) and font embedding do not exist yet.

The [Quick start](quick-start.md) builds the tools and renders a first scene. [How a scene fits together](scene-anatomy.md) explains the structure that every other page builds on.
