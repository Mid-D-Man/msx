# Element overview

The `elements::` array holds the drawable tree. Every entry is an object with a `type` field that selects one of fourteen element types. Entries paint in order, so later ones cover earlier ones.

## Common fields

Every element type except `splat` accepts an optional `id` and an optional [`transform`](transforms.md). A `splat` accepts `id` only. The shape elements, `text` and `image` take a [`style`](style-and-paint.md). A `group` takes an optional `style` for its children. An `sdf` node and a `splat` carry their own fill fields, and a `layer` carries opacity, a blend mode and effects.

## The fourteen types

| Type | Draws | Required fields |
|---|---|---|
| `rect` | Rectangle, optionally rounded | `x`, `y`, `width`, `height` |
| `circle` | Circle | `cx`, `cy`, `r` |
| `ellipse` | Ellipse | `cx`, `cy`, `rx`, `ry` |
| `line` | Straight segment | `x1`, `y1`, `x2`, `y2` |
| `polyline` | Open chain of segments | `points` |
| `polygon` | Closed chain of segments | `points` |
| `path` | Lines, curves and arcs from path data | `d` |
| `text` | A line of text | `x`, `y`, `content` |
| `group` | Child elements under one transform | none |
| `use` | A second drawing of an element with an `id` | `href` |
| `image` | An embedded or referenced PNG or JPEG | `x`, `y`, `width`, `height`, and one of `data` or `source_ref` |
| `layer` | Child elements composited as one unit | none |
| `sdf` | A signed distance field shape tree | `tree` |
| `splat` | A Gaussian blob | `x`, `y`, `sigma_x` |

A missing required field stops the parse with a message such as `elements[3].r: required`.

## Where each element renders

| Element | SVG | CPU raster | GPU raster |
|---|---|---|---|
| `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path` | yes | yes | yes |
| `text` | yes | not drawn | not drawn |
| `group` | yes, including its style | yes, without its style | yes, without its style |
| `use` | yes | yes | yes |
| `image` | yes | yes | yes |
| `layer` | yes, `clip` not enforced | yes | yes |
| `sdf` | a comment only | yes | yes |
| `splat` | approximated with a radial gradient | yes | yes |

The SVG renderer has no way to express a signed distance field shape, so an `sdf` element is omitted from SVG output and replaced by a comment. The CPU and GPU renderers evaluate it per pixel and are the reference. The [Rendering backends](rendering-backends.md) page covers the full set of differences, including gradients and shaders.

## Reading the next pages

- Geometry: [Basic shapes](shapes.md), [Paths, polylines and polygons](paths.md) and [Text](text.md)
- Structure: [Groups and use](groups-and-use.md) and [Layers](layers.md)
- Procedural shapes: [Signed distance fields](sdf.md) and [Gaussian splats](splats.md)
- Media: [Images and audio](images-and-audio.md)
