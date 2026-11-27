# SVG parity

This page tracks how much of SVG an imported file keeps. A feature ends up in one of three states. It is **native** when the MSX format and the renderers have it. It is **expanded** when the importer rewrites it into native features, so nothing is lost. It is a **gap** when the importer reports it as a warning and draws the file without it. The importer never drops a feature without saying so, with the exceptions marked "not read" below.

Progress is measured with `scripts/svg-compare/`. The four set icon corpus (Feather, Bootstrap Icons, Simple Icons, Twemoji) matched 53 of 54 files at its last run, the last being close, and has not been rerun since. The last run of the resvg tests, at version 34, matched 919 of the 996 files in the shapes, painting, paint server, structure and masking tests: all 133 `shapes`, 296 of 319 `painting`, 154 of 172 `paint-servers`, 245 of 271 `structure` and 91 of 101 `masking`. Among them are all 21 `style` tests, all 42 `use` tests, all 21 `mix-blend-mode` tests, 49 of 56 `image` tests (the rest use GIF, WebP or compressed SVG), 49 of 56 `clipPath` tests, 40 of 43 `mask` tests and 29 of 40 `pattern` tests. Several more in these groups agree with `rsvg-convert` and not with the resvg picture. Of the 418 `filters` tests, 274 match, among them all 9 `feDropShadow`, all 9 `feOffset`, all 11 `feBlend`, all 22 `feComponentTransfer`, all 3 `feMerge`, all 21 `enable-background`, 71 of 80 `filter`, 43 of 44 `filter-functions`, 19 of 20 `feComposite`, 12 of 13 `feGaussianBlur` and 14 of 16 `feColorMatrix` (the two misses use extreme `saturate` coefficients). Most of the rest use primitives that have no node yet: 1 of 29 `feImage`, 1 of 51 lighting and light source tests, 0 of 19 `feTurbulence`, 9 of 25 `feConvolveMatrix`, 0 of 7 `feTile`, 11 of 17 `feMorphology` and 1 of 3 `feDisplacementMap`. The 387 `text` tests match 225 by the pixel metric, which cannot judge text: the references use Noto Sans and the renderers embed DejaVu Sans. Another 156 are close (4 to 10 percent of the pixels differ) and 4 differ strongly. Text is judged by side by side renders instead. The run counts 1414 files outside `text` and matches 1193 of them (84 percent), or 1240 (88 percent) with the files where only `rsvg-convert` agrees. Nearly every file that does not match uses a feature listed as a gap here.

## Shapes and paths

| SVG feature | State | Notes |
|---|---|---|
| `rect` with `rx` and `ry`, `circle`, `ellipse`, `line`, `polyline`, `polygon` | Native | All three renderers |
| `path` with every command, relative forms and arcs | Native | Arcs become cubic curves in the importer, `S` and `T` reflect the previous control point on every renderer |

## Paint and stroke

| SVG feature | State | Notes |
|---|---|---|
| `fill`, `stroke`, widths, `fill-rule`, `opacity` | Native | `fill-opacity`, `stroke-opacity` and `currentColor` are folded into colors |
| Dash array and offset, caps, joins, miter limit | Native | |
| `visibility`, `display` | Native | |
| `paint-order` | Expanded | A stroke first order becomes two shapes |
| `vector-effect` | Gap | Reported |
| `shape-rendering` | Not read | A rendering hint |

## Gradients and patterns

| SVG feature | State | Notes |
|---|---|---|
| `linearGradient` | Native | Units, spread and transform as written, from binary version 3 |
| `radialGradient` | Native | Circle, focal point, focal radius `fr` and cone as written, under any units and transform |
| `gradientUnits`, `gradientTransform`, `spreadMethod` | Native | Carried as they are from binary version 3 |
| Focal point outside the circle, `fr` | Gap | Clamped onto the circle, `fr` ignored |
| Conic gradient | Native, MSX only | CPU and GPU draw it, the SVG output cannot |
| `pattern` on a fill | Expanded | Tiles clipped to the shape, with units, `viewBox`, `patternTransform`, `href` inheritance and nesting. See [Importing SVG](svg-import.md) |
| `pattern` on a stroke | Gap | Fallback color if given. The outline of a stroke is not available to clip tiles with |

## Compositing and effects

| SVG feature | State | Notes |
|---|---|---|
| Group `opacity` | Expanded | An isolated layer |
| `mix-blend-mode` | Expanded | All sixteen CSS keywords |
| `isolation` | Expanded | A layer |
| `clip-path` with `url(#id)` | Expanded | A layer with a clip path. Both units, `clip-rule`, `transform` and nested clip paths work on all three renderers |
| `clip-path` as a basic shape such as `circle()` | Gap | Reported |
| `clip-path` on a child inside a `clipPath` | Gap | Reported, the child is not clipped |
| `mask` with `url(#id)` | Expanded | A layer with a mask. Both units, the region, `mask-type`, nested masks and any paint in the mask work on all three renderers |
| `mask` in a reference cycle, `color-interpolation: linearRGB` | Gap | Reported |
| `filter` with `feGaussianBlur` (one or two deviations), `feColorMatrix`, `feDropShadow`, `feOffset`, and the functions `blur()`, `drop-shadow()`, `grayscale()`, `sepia()`, `saturate()`, `hue-rotate()`, `brightness()`, `contrast()`, `invert()` and `opacity()` | Expanded | A layer with effects and a region clip. Units, `xlink:href`, chains and lists work. A color matrix and a blur with two deviations are binary version 3 |
| `filter` with `feFlood`, `feComposite`, `feMerge`, `feBlend`, `feComponentTransfer`, named results, `SourceAlpha` and primitive regions | Expanded | One `graph` layer effect, binary version 3. Drawn by the SVG and CPU renderers. A filter that works as a chain stays a chain |
| `filter` with `feImage`, `feTile`, `feMorphology`, `feTurbulence`, `feConvolveMatrix`, `feDisplacementMap` or lighting | Gap | Reported, drawn without that filter. They need graph nodes the format does not have |
| A filter region that is smaller than the content | Gap | The filter region clips the result, and resvg also clips the source before the effects, so a blur at the edge of a small region is harder here |
| `marker-start`, `marker-mid`, `marker-end`, `marker` in CSS | Expanded | Copies of the marker content at the vertices, on paths, lines, polylines, polygons, rectangles and ellipses. All marker attributes work, and the viewport clips |

## Structure

| SVG feature | State | Notes |
|---|---|---|
| `g`, `defs`, `use`, `symbol`, nested `svg` | Expanded | Placement, `viewBox` and viewport clipping are kept, and `overflow` is honored |
| `viewBox`, `preserveAspectRatio` | Expanded | One wrapper transform |
| No `width`, `height` or `viewBox` | Expanded | The canvas is the extent of the content, as in resvg |
| Files that are not UTF-8 | Expanded | UTF-16 and the encodings named in [Importing SVG](svg-import.md#encodings) |
| `transform`, CSS `transform`, `transform-origin` | Expanded | `transform-box` other than `view-box` is a gap |
| `switch`, `systemLanguage`, `requiredExtensions` | Expanded | The language is English |
| `<style>` rules | Expanded | Types, ids, classes, attribute selectors and `:first-child`, joined by descendant and child combinators, with summed specificity. Sibling combinators and other pseudo-classes are skipped |
| `image` with PNG or JPEG | Expanded | From a data URI or a file, with `preserveAspectRatio`, a clip for `slice`, opacity, transform, clip and mask |
| `image` with SVG | Expanded | The inner document is imported with its own ids, styles and gradients and placed like a bitmap, with `preserveAspectRatio`, a clip to its canvas and a clip for `slice`. Animation inside it is drawn as the static frame |
| `image` with GIF, WebP or `.svgz` | Gap | Reported. MSX holds PNG and JPEG, and these would need a decoder and a PNG transcode |
| `context-fill`, `context-stroke` | Partial | Colors, opacity and nesting through `use` and markers work. A gradient is fitted to the shape painted, not to the context element, and a pattern on a stroke is a gap |
| `foreignObject` | Gap | Reported |
| `animateTransform` of `translate`, `scale` and `rotate`, `animate` of `opacity`, `x`, `y`, `cx`, `cy`, `r` on an unstroked circle and the opacity of a single paint, with `begin`, `dur`, repeats, `end`, `fill`, `values`, `keyTimes`, `calcMode` and `keySplines` | Expanded | Baked into MSX tracks on one scene timeline, with a wrapper group or layer per animation. See [Importing SVG](svg-import.md#animation) |
| `set` of the same attributes, `set` and `animate` of `visibility` and `display`, `animateMotion` with `path`, `mpath`, `values`, `keyPoints` and `rotate` | Expanded | A constant track, an opacity layer that shows or hides the element, and sampled translate and rotate tracks. See [Importing SVG](svg-import.md#animation) |
| `animate` and `animateColor` of `fill`, `stroke`, `stroke-width` and `stroke-dashoffset` on a shape | Expanded | Style channel tracks on the shape, binary version 3. See [Style animation](svg-import.md#style-animation) |
| `animate` of `width`, `height`, `rx`, `ry`, `r` (stroked or not), `x1` to `y2` and `stroke-dasharray` with one or two entries | Expanded | Geometry channel tracks on the shape, binary version 3. See [Begin that names another animation](svg-import.md#begin-that-names-another-animation) |
| `begin` with `id.begin`, `id.end` and offsets, in rings of animations that start each other | Expanded | Solved to start times and baked as a repeat with a period. Starts that are not an even repeat are reported |
| `animate` of paths and points, `skewX` and `skewY`, `animateMotion` with `calcMode` discrete or spline, `begin` on an event | Gap | Reported, the static frame is drawn. MSX tracks have no channel for them |
| CSS `animation` and `@keyframes` with `transform` (translate, scale and rotate), `opacity`, `fill-opacity`, `stroke-opacity`, `fill`, `stroke`, `stroke-width` and `stroke-dashoffset` | Expanded | Baked into tracks like the SMIL animations, with delay, iterations, direction, fill mode, `cubic-bezier()`, `steps()` and keyword easing, and `transform-origin` with `transform-box: fill-box`. See [Importing SVG](svg-import.md#animation) |
| CSS animation with `paused`, skews, matrices, mismatched `transform` lists, or other keyframe properties | Gap | Reported, the static frame is drawn |

## Text

| SVG feature | State | Notes |
|---|---|---|
| `text`, `tspan` with positions, `dx`, `dy`, anchor, size and weight | Native | Spans of different style are separate text elements placed one after another. Lists of per-glyph values, `rotate` and kerning across spans are open |
| `font-family` | Not read | Text uses the embedded DejaVu Sans subset |
| `font-style`, `text-decoration`, `letter-spacing` and the `font` shorthand | Native | Kept in the binary from version 3 on. A `none` does not remove a line an ancestor set. Italic is a synthesized slant |
| `writing-mode` | Native | Vertical text turns Latin glyphs a quarter turn clockwise. Upright CJK glyphs and `text-orientation` are open |
| `word-spacing` | Native | Read on any element and inherited; a length, `em`, `mm` and so on, or a percentage of the viewport, or `normal`. Stored as `word_spacing`, in the second presence byte of the style extension block. Follows the space, the no-break space and the Ethiopic and Aegean word separators |
| Different styles inside one line | Gap | The style of the line start is used |
| `textPath` with `href` or `path`, `startOffset`, `dx` and `dy`, on a path, `rect`, `circle`, `ellipse`, `line`, `polyline` or `polygon` | Native | Text on a path, binary version 3. Open: decoration lines and `rotate` on the path, glyphs flowing on to a second subpath, and the exact pen position after a text that runs past the end |
| `baseline-shift` | Native | Read on `tspan` and `textPath` (`sub`, `super`, length, percentage, `inherit`, nested shifts add up) and baked into the text position, so no format change. `sub` and `super` use fixed fractions of the font size |
| `alignment-baseline` | Native | Read on `text`, `tspan` and `textPath` and set as the baseline of each piece, so no format change. The offsets come from the embedded face, so they differ from the Noto references by the font gap (up to about 5 px at 36 px). Open: vertical text, and nested spans use the innermost value |
| `dominant-baseline` | Native | Kept in the binary from version 3 on. Applies to text on a path too, across the path |

## Renderers

| Behavior | SVG | CPU | GPU |
|---|---|---|---|
| Style inherited from a group | Yes | No | No |
| `viewbox` block | Yes | No | No |
| Layer effects | All | All | Blur, blur per axis and color matrix. Not the graph, the shadows and the glows |
| Edge antialiasing | Yes | Yes | No |

Imported files do not depend on the rows marked No, because the importer writes explicit style on every shape and a single wrapper transform.

## Order of work

The remaining gaps share a few causes, so they are closed in this order.

1. Done: a clip path on layers. It closes `clip-path` and the `symbol` and nested `svg` viewport clipping. What is left of it is the clip on a clip path child and the basic shapes. Masks followed on the same layer machinery and are done too.
2. Done: gradient units, a gradient matrix, spread modes, focal points and `fr` in the format (binary version 3).
3. Patterns. Done for fills by tile expansion in the importer. What is left is a real pattern def in the format, which would also give strokes and avoid one group per tile.
4. Done: markers, expanded by the importer into copies at the vertices.
5. Done for PNG, JPEG and SVG images. What is left is GIF, WebP and `.svgz`, which need a decoder.
6. Done for blur (also per axis), color matrix, drop shadow, offset, the CSS color functions and, as a filter graph, flood, composite, merge, blend, component transfer, named results, standard inputs and primitive regions. What is left is new graph nodes: `feImage`, `feTile`, `feMorphology`, `feTurbulence`, `feConvolveMatrix`, `feDisplacementMap` and lighting, the graph on the GPU renderer, and clipping the source graphic to the filter region before the effects run.
7. Done: text styling, vertical text and `textPath`. `baseline-shift`, `alignment-baseline` and `word-spacing` followed. What is left is per-glyph lists and `rotate`, upright CJK glyphs, and shaping.
8. Done: a binary version 3 with slots for `fill_opacity`, `stroke_opacity` and `dominant_baseline`, written only for a scene that uses one. CPU and GPU now honor the two opacities.
9. Done for SMIL transforms and opacity, and the writer now writes animated scenes. CSS `@keyframes` followed. `set`, `animateMotion` and `steps()` followed. Style channels for fill and stroke color, stroke width and dash offset followed (binary version 3). What is left is the properties that have no track channel yet, such as rect width and height, `rx`, `ry`, the radius of a stroked circle, `d` and `points`.
