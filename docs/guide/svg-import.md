# Importing SVG

The importer reads an SVG file into an MSX scene. `msx import` writes that scene as editable `.msx` source. Every other command that takes an input also accepts an `.svg` file directly, so an SVG can be rasterized, compiled or rendered without an intermediate file.

```bash
msx import icon.svg -o icon.msx
msx rasterize icon.svg -o icon.png
msx compile icon.svg -o icon.msxb
```

Anything the SVG uses that MSX cannot express is listed as a warning on stderr. The file is still written, without that feature.

## What the importer resolves

The importer does the work that the CPU and GPU renderers do not, so an imported file renders the same on the SVG, CPU and GPU backends.

- **Style.** Presentation attributes, `<style>` rules, the `style` attribute and `!important` are cascaded in CSS order. Selectors may combine a type, an id and classes. Inheritance is resolved, so every shape carries explicit values and groups carry no style.
- **Opacity and color.** `fill-opacity`, `stroke-opacity` and `currentColor` become plain colors and gradient stop alphas. The raster renderers do not read the opacity keys, so none are written.
- **Reuse.** `use` is expanded in place with the style of the `use` element. A `symbol` and a nested `svg` keep their `viewBox` mapping. Ids stay on the original shape only.
- **Size.** The canvas takes its size from `width`, `height` and `viewBox`. The mapping, including `preserveAspectRatio`, becomes one wrapper transform, so no `viewbox` block is written. The background is transparent.
- **Gradients.** MSX gradients live in the bounding box of the shape and have no transform. The importer converts `userSpaceOnUse` coordinates and `gradientTransform` into that space for each shape. A linear gradient converts exactly under any transform. A radial gradient converts exactly under uniform scale, rotation and translation. Under a stretch or skew it is approximated by a circle of the same area, and a warning says so.
- **Compositing.** `opacity` on a group becomes an isolated layer. `mix-blend-mode` becomes the blend mode of a layer for multiply, screen, overlay, darken, lighten, hard-light, soft-light, difference and exclusion. `isolation: isolate` becomes a layer. `paint-order: stroke` becomes two shapes, the stroke first.
- **Clipping.** `clip-path: url(#id)` on any element becomes a layer with a `clip_path`. `clipPathUnits` of `userSpaceOnUse` and `objectBoundingBox`, the `transform` of the `clipPath`, `clip-rule` and a `clip-path` on the `clipPath` itself (the clips nest, so both apply) are honored. Only shapes with an area, text and a `use` of one of those count, and fill, stroke and opacity inside the clip path are ignored, as in SVG. A `symbol` and a nested `svg` clip to their viewport the same way, unless `overflow` is `visible` or `auto`. The clip is added only when the content can reach past the viewport, so a symbol that fits costs no extra layer.
- **Masks.** `mask: url(#id)` on any element becomes a layer with a `mask`. `maskUnits` and `maskContentUnits` in both units, the `x`, `y`, `width` and `height` region (default 120 percent of the box, kept as a clip of the mask content when the content reaches past it), `mask-type` as an attribute or in style, and a `mask` on the `mask` itself are honored. The mask content keeps its paint, gradients, opacity and nested masks, since it is drawn as normal elements. A box based mask on a shape with no area, or a region with no area, hides the element, as in SVG.
- **Images.** `<image>` with a PNG or JPEG becomes an image element. The bytes come from a `data:` URI, with line breaks and any padding in the base64 accepted, or from a file. A file is read from the SVG's directory and embedded when the importer is given one, as `msx import` and every command that takes an SVG do. Without a directory, as in the browser converter, the path is kept and reported, and the size must then come from the SVG. The format is decided by the bytes, not the mime type. A missing `width` or `height` is taken from the image, the other side keeping its ratio. `preserveAspectRatio` is worked out by the importer: the image is placed and sized inside the box, and a `slice` that spills over the box is clipped to it. Opacity, `transform`, `clip-path` and `mask` apply as for shapes.
- **Markers.** `marker-start`, `marker-mid` and `marker-end`, and the CSS `marker` shorthand, are expanded into copies of the marker content at the vertices of the shape, placed after the shape inside one layer so the shape and its markers fade, clip and mask together. The properties are inherited, and the `marker` presentation attribute is ignored, as it is only valid in CSS. Markers go on paths, lines, polylines and polygons, and also on rectangles, circles and ellipses through their outline, which is how SVG 2 defines it. `markerWidth`, `markerHeight`, `viewBox`, `preserveAspectRatio`, `refX`, `refY`, `markerUnits` (stroke width by default), `orient` (`auto`, `auto-start-reverse` or an angle with `deg`, `grad`, `rad` or `turn`) and `overflow` are honored, and content past the marker viewport is clipped. At a vertex between two segments a marker faces the bisector of the two directions. The first vertex of a subpath faces along its first segment and the last along its last segment, also for a closed one, which is how resvg draws it. A zero stroke width scales a marker to nothing. Markers on text are ignored, as in SVG.
- **Transforms.** The `transform` attribute and the CSS `transform` property, with angle and length units, and `transform-origin`.
- **Conditions.** `switch` draws its first child that passes, and `systemLanguage` and `requiredExtensions` apply to any element. The language is English.
- **Text.** Whitespace collapses. A `tspan` with its own position becomes its own text element, which gives multi-line text.
- **Paths.** Path data is parsed leniently and written back normalized. A path with an error draws up to the error, and a path that does not start with a moveto is not drawn, as in SVG.

## What is reported and not imported

| SVG feature | Result |
|---|---|
| `filter` | Drawn without it |
| `mask` that is not `url(#id)`, a mask in a reference cycle, or `color-interpolation: linearRGB` on a mask | Drawn without it, or read as sRGB |
| `clip-path` that is not `url(#id)`, such as `circle()` | Drawn without it |
| `clip-path` on a child inside a `clipPath` | Ignored |
| A `clip-path` that points at a missing element, at something that is not a `clipPath`, or in a cycle | Drawn without it |
| `<animate>`, `<animateTransform>`, `<animateMotion>`, `<set>`, CSS `animation` and `@keyframes` | The static frame is drawn |
| `pattern` paint | Fallback color if given, otherwise none |
| A marker that points at a missing element, at something that is not a `marker`, or in a reference cycle | Drawn without it |
| `marker-*` values that are not `url(#id)` | Drawn without markers |
| `foreignObject` | Skipped |
| `image` that is SVG, GIF or WebP, or points at a web address | Skipped, because MSX holds only PNG and JPEG |
| `image-rendering` other than `auto` and `optimizeQuality` | Drawn with smooth scaling |
| `textPath` | Its text is skipped |
| `spreadMethod` of `reflect` or `repeat` | Imported as `pad` |
| `fr` on a radial gradient, a focal point outside the circle | `fr` ignored, focal point moved onto the circle |
| `context-fill`, `context-stroke` | Painted as none |
| Blend modes beyond the nine above | Drawn normally |
| CSS selectors with combinators, attributes or pseudo-classes | The rule is skipped |
| Spans with different styles on one text line | The style of the line start is used |
| `text-decoration`, `letter-spacing`, `word-spacing`, `font-style`, `writing-mode`, `vector-effect` | Ignored, with a warning when the value is not the initial one |
| A user-space gradient on a shape with no bounding box area | The average color of its stops |

Files that are not UTF-8 are rejected with an error. Elements are capped at 200,000 shapes, so an exponentially nested `use` chain cannot run away. A `use` that refers back to itself is skipped.

## In the browser

The site has a [converter page](/convert/) that runs the importer in a WebAssembly build. An SVG is pasted, dropped on the editor or opened from disk. The page shows the `.msx` source the importer wrote, the original drawn by the browser next to the converted scene, and every warning. The source can be copied or downloaded, and the compiled binary can be downloaded too. The conversion is the same code as `msx import`, and the preview is drawn from the written source parsed back, so the preview shows the file as saved.

The status of each SVG feature is on the [SVG parity](svg-parity.md) page.

## Checking an import

`scripts/svg-compare/` renders SVG files with the importer and the CPU renderer and compares each against reference renderers. The script `compare.py` takes a mode and a folder, `summarize.py` groups the result, and `sidebyside.py` writes a sheet of the importer output, the reference and the difference for chosen files. The scripts need `rsvg-convert`, Pillow and NumPy, and use the resvg test suite as an optional second reference.
