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
- **Transforms.** The `transform` attribute and the CSS `transform` property, with angle and length units, and `transform-origin`.
- **Conditions.** `switch` draws its first child that passes, and `systemLanguage` and `requiredExtensions` apply to any element. The language is English.
- **Text.** Whitespace collapses. A `tspan` with its own position becomes its own text element, which gives multi-line text.
- **Paths.** Path data is parsed leniently and written back normalized. A path with an error draws up to the error, and a path that does not start with a moveto is not drawn, as in SVG.

## What is reported and not imported

| SVG feature | Result |
|---|---|
| `clip-path`, `mask`, `filter` | Drawn without them |
| `pattern` paint | Fallback color if given, otherwise none |
| `marker`, `marker-start`, `marker-mid`, `marker-end` | Drawn without markers |
| `image`, `foreignObject` | Skipped |
| `textPath` | Its text is skipped |
| `spreadMethod` of `reflect` or `repeat` | Imported as `pad` |
| `fr` on a radial gradient, a focal point outside the circle | `fr` ignored, focal point moved onto the circle |
| Viewport clipping of a `symbol` or nested `svg` | Content is not clipped |
| `context-fill`, `context-stroke` | Painted as none |
| Blend modes beyond the nine above | Drawn normally |
| CSS selectors with combinators, attributes or pseudo-classes | The rule is skipped |
| Spans with different styles on one text line | The style of the line start is used |
| A user-space gradient on a shape with no bounding box area | The average color of its stops |

Files that are not UTF-8 are rejected with an error. Elements are capped at 200,000 shapes, so an exponentially nested `use` chain cannot run away. A `use` that refers back to itself is skipped.

## Checking an import

`scripts/svg-compare/` renders SVG files with the importer and the CPU renderer and compares each against reference renderers. The script `compare.py` takes a mode and a folder, `summarize.py` groups the result, and `sidebyside.py` writes a sheet of the importer output, the reference and the difference for chosen files. The scripts need `rsvg-convert`, Pillow and NumPy, and use the resvg test suite as an optional second reference.
