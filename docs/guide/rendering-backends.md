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
| Text | yes | not drawn | not drawn |
| Image | yes | yes | yes |
| Group, use | yes | yes | yes |
| Group style reaching its children | yes | no | no |
| Linear and radial gradients | interpolated | one flat color | one flat color |
| Conic gradient | not drawn | one flat color | one flat color |
| Shader fills | `fallback_color` | `fallback_color` | runs the WGSL |
| Signed distance fields | omitted | yes | yes |
| Gaussian splats | approximated | yes | yes |
| Layer opacity and blend modes | yes, see below | yes | yes |
| Layer effects | all five | all five | `blur` only |
| Layer `clip` | not enforced | not documented | not documented |
| Viewbox | yes | no | no |
| Keyframe animation | static frames | GIF | GIF |

The flat color for a gradient is the average of its stops. The SVG renderer is the only one that interpolates, so a gradient looks right in SVG output and flat in PNG output.

## Notes on each renderer

### SVG

- Every style key is written into the output, which makes SVG the reference for what a style means.
- Layers become groups with `mix-blend-mode` and a generated filter chain for effects. The blend modes `subtract` and `divide` have no CSS counterpart and are written as `normal`.
- An `sdf` element is replaced by a comment. Splats become a three stop radial gradient that reaches out to 2.4 sigma.
- A conic gradient has no SVG form. A comment is written and the fill is not drawn.

### CPU

- The rasterizer applies stroke width, dashes, caps, joins, the miter limit and the fill rule. It does not read `fill_opacity` or `stroke_opacity`.
- Compositing for layers uses the shared blend functions in `msx-render-core`, which cover all twelve modes.
- Text is skipped on purpose, because no font shaping is wired in.

### GPU

- A shader fill runs for real on shapes, SDF nodes, splats and shapes inside layers. Shader strokes fall back to the flat color, and a shape's `opacity` is not multiplied into the shader output.
- When the shader file cannot be read, the GPU renderer paints `fallback_color` and prints a line on the standard error stream.
- Layers composite in an isolated buffer with all twelve blend modes, and `blur` is the one effect implemented.
- A machine with no adapter gets a clear error from the CLI and not a crash.

## Choosing a renderer

- Use **SVG** for scenes that use text, for gradients that must interpolate, and for output that has to stay vector.
- Use the **CPU** rasterizer for PNG and GIF output that needs no special hardware. It covers SDF shapes, splats, layers and all five effects.
- Use the **GPU** renderer when a scene has shader fills, or when blur on large layers needs speed.

The samples page shows the same set of files through these renderers, and the build report lists the result of each CI run.
