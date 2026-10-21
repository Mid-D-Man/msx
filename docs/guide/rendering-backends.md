# Rendering backends

MSX has three renderers. They share the scene model, so one file can go to any of them, but they do not draw every feature. This page lists what each one does today.

| Renderer | Crate | Output | Needs |
|---|---|---|---|
| SVG | `msx-render-svg` | SVG text | nothing |
| CPU | `msx-render-cpu` | PNG, animated GIF | nothing |
| GPU | `msx-render-gpu` | PNG, animated GIF | the `gpu` Cargo feature and a GPU adapter, which can be a software one |

The CPU renderer rasterizes with `tiny-skia`, and evaluates signed distance fields and splats per pixel. The GPU renderer tessellates shapes with `lyon` and draws through `wgpu`.

## Support matrix

| Feature | SVG | CPU | GPU |
|---|---|---|---|
| Rect, circle, ellipse, line, polyline, polygon, path | yes | yes | yes |
| Text | yes | yes, DejaVu Sans only | yes, DejaVu Sans only |
| Image | yes | yes | yes |
| Group, use | yes | yes | yes, but a `use` of an `sdf` node, splat or image draws nothing |
| Group style reaching its children | yes | no | no |
| Linear and radial gradients | interpolated | interpolated | interpolated |
| Conic gradient | not drawn | yes | yes |
| Shader fills | `fallback_color` | `fallback_color` | runs the WGSL |
| Signed distance fields | omitted | yes | yes |
| Gaussian splats | approximated | yes | yes |
| Layer opacity and blend modes | yes, see below | yes | yes |
| Layer effects | all five | all five | `blur` only |
| Layer `clip` | not enforced | not documented | not documented |
| Layer `clip_path` | `clipPath` element | antialiased mask | mask texture, hard edges |
| Layer `mask` | `mask` element | buffer scaled by luminance or alpha | texture multiplied by luminance or alpha |
| Viewbox | yes | no | no |
| Keyframe animation | static frames | GIF | GIF |

Where a gradient is not interpolated, the paint is the average of its stops. That applies to gradient fills of `sdf` nodes and splats on the CPU and GPU renderers.

## Notes on each renderer

### SVG

- Every style key is written into the output, which makes SVG the reference for what a style means.
- Layers become groups with `mix-blend-mode` and a generated filter chain for effects. The blend modes `subtract` and `divide` have no CSS counterpart and are written as `normal`.
- An `sdf` element is replaced by a comment. Splats become a three stop radial gradient that reaches out to 2.4 sigma.
- A conic gradient has no SVG form. A comment is written and the fill is not drawn.

### CPU

- The rasterizer applies stroke width, dashes, caps, joins, the miter limit and the fill rule. It does not read `fill_opacity` or `stroke_opacity`.
- Compositing for layers uses the shared blend functions in `msx-render-core`, which cover all twelve modes.
- Text is drawn as glyph outlines from an embedded DejaVu Sans face, through the same path pipeline as other shapes. `font_family` is ignored and there is no OpenType shaping. See [Text](text.md).
- Linear and radial gradients are `tiny-skia` shaders mapped through the bounding box. Conic gradients have no `tiny-skia` shader, so the renderer rasterizes the shape coverage and colors each covered pixel itself.

### GPU

- Elements draw in document order. Neighboring elements of one kind (vector shape, shader fill, `sdf` node, splat, image) share one draw, and a new draw starts when the kind changes.
- Shapes, text and gradients share one vertex batch. A gradient is evaluated per fragment from its bounding box, so it interpolates on fills and strokes, and a gradient holds at most 32 stops. A longer list is resampled to 32 evenly spaced stops, which softens any hard stop past that limit.
- Text is drawn from the same embedded DejaVu Sans outlines as on the CPU renderer. Shape and text edges are not antialiased.
- A shader fill runs for real on shapes, SDF nodes, splats and shapes inside layers. Shader strokes fall back to the flat color, and a shape's `opacity` is not multiplied into the shader output.
- When the shader file cannot be read, the GPU renderer paints `fallback_color` and prints a line on the standard error stream.
- Layers composite in an isolated buffer with all twelve blend modes, and `blur` is the one effect implemented.
- A machine with no adapter gets a clear error from the CLI and not a crash.

## Choosing a renderer

- Use **SVG** for output that has to stay vector, and for text that should use the viewer's own fonts.
- Use the **CPU** rasterizer for PNG and GIF output that needs no special hardware. It covers text, gradients including conic, SDF shapes, splats, layers and all five effects.
- Use the **GPU** renderer when a scene has shader fills, or when blur on large layers needs speed.

The samples page shows the same set of files through these renderers, and the build report lists the result of each CI run.
