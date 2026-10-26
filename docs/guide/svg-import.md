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

- **Style.** Presentation attributes, `<style>` rules, the `style` attribute and `!important` are cascaded in CSS order. Selectors may combine types, ids, classes, attribute selectors (`[a]`, `=`, `~=`, `|=`, `^=`, `$=` and `*=`) and `:first-child`, joined by descendant and child combinators, and the specificity adds up over the whole selector. Inheritance is resolved, so every shape carries explicit values and groups carry no style.
- **Opacity and color.** `fill-opacity`, `stroke-opacity` and `currentColor` become plain colors and gradient stop alphas. The raster renderers do not read the opacity keys, so none are written.
- **Reuse.** `use` is expanded in place with the style of the `use` element. A `symbol` and a nested `svg` keep their `viewBox` mapping. Ids stay on the original shape only.
- **Size.** The canvas takes its size from `width`, `height` and `viewBox`. The mapping, including `preserveAspectRatio`, becomes one wrapper transform, so no `viewbox` block is written. The background is transparent.
- **Gradients.** MSX gradients live in the bounding box of the shape and have no transform. The importer converts `userSpaceOnUse` coordinates and `gradientTransform` into that space for each shape. A linear gradient converts exactly under any transform. A radial gradient converts exactly under uniform scale, rotation and translation. Under a stretch or skew it is approximated by a circle of the same area, and a warning says so.
- **Compositing.** `opacity` on a group becomes an isolated layer. `mix-blend-mode` becomes the blend mode of a layer for multiply, screen, overlay, darken, lighten, hard-light, soft-light, difference and exclusion. `isolation: isolate` becomes a layer. `paint-order: stroke` becomes two shapes, the stroke first.
- **Clipping.** `clip-path: url(#id)` on any element becomes a layer with a `clip_path`. `clipPathUnits` of `userSpaceOnUse` and `objectBoundingBox`, the `transform` of the `clipPath`, `clip-rule` and a `clip-path` on the `clipPath` itself (the clips nest, so both apply) are honored. Only shapes with an area, text and a `use` of one of those count, and fill, stroke and opacity inside the clip path are ignored, as in SVG. A `symbol` and a nested `svg` clip to their viewport the same way, unless `overflow` is `visible` or `auto`. The clip is added only when the content can reach past the viewport, so a symbol that fits costs no extra layer.
- **Masks.** `mask: url(#id)` on any element becomes a layer with a `mask`. `maskUnits` and `maskContentUnits` in both units, the `x`, `y`, `width` and `height` region (default 120 percent of the box, kept as a clip of the mask content when the content reaches past it), `mask-type` as an attribute or in style, and a `mask` on the `mask` itself are honored. The mask content keeps its paint, gradients, opacity and nested masks, since it is drawn as normal elements. A box based mask on a shape with no area, or a region with no area, hides the element, as in SVG.
- **Images.** `<image>` with a PNG or JPEG becomes an image element. The bytes come from a `data:` URI, with line breaks and any padding in the base64 accepted, or from a file. A file is read from the SVG's directory and embedded when the importer is given one, as `msx import` and every command that takes an SVG do. Without a directory, as in the browser converter, the path is kept and reported, and the size must then come from the SVG. The format is decided by the bytes, not the mime type. A missing `width` or `height` is taken from the image, the other side keeping its ratio. `preserveAspectRatio` is worked out by the importer: the image is placed and sized inside the box, and a `slice` that spills over the box is clipped to it. Opacity, `transform`, `clip-path` and `mask` apply as for shapes.
- **SVG images.** An `<image>` that holds an SVG, from a `data:` URI or a file, is imported with a nested importer and placed like a bitmap of the size of the inner canvas, with the same `preserveAspectRatio` and the same clip for a `slice`. What the inner document draws outside its own canvas is cut off. A file the inner document names is read from the folder of that file, and a data URI has no folder. The inner document keeps its own ids and styles, and its gradients move into the scene under ids that are free. Its warnings are reported with the image. A document that already contains the one it sits in is skipped, which stops an image that embeds itself, and a chain of different documents stops after four.
- **Paint context.** `context-fill` and `context-stroke` take the paint of the `use` that is being expanded or of the shape that a marker is drawn for, including its opacity. A context paint on an element inside a context refers to the enclosing one. A marker whose content uses a context paint is built once for each distinct context, and any other marker once.
- **Markers.** `marker-start`, `marker-mid` and `marker-end`, and the CSS `marker` shorthand, are expanded into copies of the marker content at the vertices of the shape, placed after the shape inside one layer so the shape and its markers fade, clip and mask together. The properties are inherited, and the `marker` presentation attribute is ignored, as it is only valid in CSS. Markers go on paths, lines, polylines and polygons, and also on rectangles, circles and ellipses through their outline, which is how SVG 2 defines it. `markerWidth`, `markerHeight`, `viewBox`, `preserveAspectRatio`, `refX`, `refY`, `markerUnits` (stroke width by default), `orient` (`auto`, `auto-start-reverse` or an angle with `deg`, `grad`, `rad` or `turn`) and `overflow` are honored, and content past the marker viewport is clipped. At a vertex between two segments a marker faces the bisector of the two directions. The first vertex of a subpath faces along its first segment and the last along its last segment, also for a closed one, which is how resvg draws it. A zero stroke width scales a marker to nothing. Markers on text are ignored, as in SVG.
- **Animation.** SMIL animation of transforms and opacity becomes MSX animation tracks, so an animated SVG plays in `msx animate`, in `msx-viewer` and on the converter page. See [Animation](#animation) below.
- **Transforms.** The `transform` attribute and the CSS `transform` property, with angle and length units, and `transform-origin`.
- **Conditions.** `switch` draws its first child that passes, and `systemLanguage` and `requiredExtensions` apply to any element. The language is English.
- **Text.** Whitespace collapses. A `tspan` with its own position becomes its own text element, which gives multi-line text.
- **Paths.** Path data is parsed leniently and written back normalized. A path with an error draws up to the error, and a path that does not start with a moveto is not drawn, as in SVG.

## Animation

SMIL animation is imported into MSX animation tracks. The written `.msx` source then carries `duration`, `loop_mode` and an `animations::` list, and the compiled binary carries the same in its animation section.

| SMIL | MSX |
|---|---|
| `animateTransform` of type `translate`, `scale` or `rotate`, with or without a rotation centre | `translate_x` and `translate_y`, `scale_x` and `scale_y`, or `rotate` tracks |
| `animate` of `opacity` | An `opacity` track |
| `animate` of `fill-opacity` on a shape that paints only its fill, or of `stroke-opacity` on a shape that paints only its stroke | An `opacity` track on a wrapper layer. Fading the layer is the same picture as fading the one paint, and a shape that paints both is refused. As in SVG the animation replaces the static value, so the shape is drawn at full alpha inside the layer, and the static value applies before the animation begins and after it ends |
| `animate` of `r` on a circle without a stroke | `scale_x` and `scale_y` tracks about the centre. A stroke would scale with the radius, which SVG does not do, so a stroked circle is refused |
| `animate` of `x` and `y` on `rect`, `image`, `use` and nested `svg`, and of `cx` and `cy` on `circle` and `ellipse` | A translate track that holds the difference from the attribute |

Timing is read from `begin` (a clock value, or a list of them, of which the earliest counts), `dur`, `repeatCount`, `repeatDur`, `end` and `fill="freeze"`. Values come from `values`, from `from`, `to` and `by`, or from `to` and `by` alone, which start at the value the element has without the animation. `keyTimes`, `calcMode` (`linear`, `discrete`, `paced` and `spline`) and `keySplines` are honored. An animation element can be a child of its target or name the target with `href`.

An MSX scene has one timeline and its tracks hold linear keyframes with four easings, so the importer bakes every animation onto the scene timeline:

- A `spline` segment is cut into 12 straight pieces, and a `discrete` value becomes a step with a ramp of one millisecond.
- Repeats are unrolled into whole iterations. The jump from the end of one iteration to the start of the next is also a ramp of one millisecond.
- When some animation repeats forever, the scene loops with the least common multiple of the periods as its duration, counted to the millisecond and capped at 60 seconds. A longer common period is reported, and the scene loops at the longest period instead.
- A delay on an animation that repeats forever becomes a phase shift. The animation is treated as already running at time 0, so the loop closes without a seam. A browser shows the delay once, and a looping MSX scene would show it on every loop.
- Without a repeating animation the scene plays once and lasts until the last animation ends. A finite animation in a looping scene plays again on every loop. Before it begins, and after it ends without `fill="freeze"`, the element has the value it has without the animation.

An MSX track moves one element by id, and its change wraps the static transform of the element from the outside. So each imported animation gets a wrapper node of its own around the element:

- A transform animation, and `x`, `y`, `cx` and `cy`, get a group. The element's own `transform` goes outside the wrappers, which is how SMIL adds an animation to a static transform.
- A rotation about a centre is a group that moves the centre to the origin inside the wrapper and back outside it, so no track has to follow a curved path.
- An opacity animation gets a layer, since only a layer composites opacity the same way on every renderer. The animated value replaces the opacity of the element.
- Several animations on one element nest in a fixed order, outermost first: opacity layers, then `animateTransform` in document order, then position (`x`, `y`, `cx`, `cy`), then radius. The attributes change the geometry, which SVG places inside the transform whatever the order in the file, and a moved circle is a translation of the scaled circle. Each `use` copy gets its own wrappers, so copies animate independently.

Wrapper ids are named `anim1`, `anim2` and so on, and skip any id that the SVG already uses.

CSS animations are imported the same way. The `animation` property and its longhands name an `@keyframes` rule, and the `transform`, `opacity`, `fill-opacity` and `stroke-opacity` of its keyframes become tracks:

- The transform functions are `translate`, `translateX`, `translateY`, `scale`, `scaleX`, `scaleY` and `rotate`, with lengths in `px` and angles in `deg`, `grad`, `rad` or `turn`. Each function of a keyframe list gets a wrapper of its own in list order, so `translate(10px) rotate(90deg)` and `rotate(90deg) translate(10px)` differ as they do in CSS. All keyframes of one animation must use the same functions in the same order, where `none` and a keyframe that is missing count as the identity. Anything else would need matrix interpolation.
- `transform-origin` is the pivot of a scale or a rotation, with the default of `0 0`. It is measured in the viewport, or in the box of the element when `transform-box` is `fill-box`.
- Timing comes from the duration, the delay (a negative delay starts part way in), the iteration count (`infinite` included), the direction (`reverse`, `alternate` and `alternate-reverse`, where the easing of each interval is reversed with it), the fill mode (`forwards` holds the last value, `backwards` holds the first during the delay) and the timing function: `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out` and `cubic-bezier()`, also per keyframe. Several animations in one `animation` list all run.
- A finite animation without `forwards` goes back to the static value after its end, so a scene that plays once lasts one millisecond longer than its last animation.
- CSS replaces the `transform` of an element with the animated one. The importer keeps the static transform and adds the animation to it, and says so.

What is reported, with the static frame drawn:

| SVG feature | Reason |
|---|---|
| `animate` of a color, `d`, `points`, `width`, `height`, `rx`, `ry`, `stroke-width`, `stroke-dashoffset` or any other attribute, and `r` on an ellipse | MSX tracks have no channel for it |
| `fill-opacity` where the stroke also paints (and `stroke-opacity` where the fill does), `r` on a stroked circle | The fade or scale would not be the same picture |
| `animateTransform` of type `skewX` or `skewY` | No channel |
| `animateMotion`, `animateColor`, `set` | Not mapped yet |
| `begin` that waits for an event (`click`), for another animation (`id.end`) or never (`indefinite`), and an animation without a finite `dur` | A scene has no input to wait for |
| An animation inside a `mask` or a `marker` | The resolver does not reach those elements |
| `additive="replace"` on an element that has a static `transform` | SMIL would drop the static transform. The importer adds the animation to it and says so |
| CSS `animation` with `steps()` easing, `paused`, a zero duration, skews, matrices, percentages in `translate`, keyframes whose `transform` lists use different functions, and keyframe properties other than `transform`, `opacity`, `fill-opacity` and `stroke-opacity` | No channel, or it would need matrix interpolation |

An `end` that waits for an event is ignored, with a note. At most 4,000 wrappers are written, and an animation that would need more than 20,000 keyframes on the scene timeline is left static.

## What is reported and not imported

| SVG feature | Result |
|---|---|
| `filter` | Drawn without it |
| `mask` that is not `url(#id)`, a mask in a reference cycle, or `color-interpolation: linearRGB` on a mask | Drawn without it, or read as sRGB |
| `clip-path` that is not `url(#id)`, such as `circle()` | Drawn without it |
| `clip-path` on a child inside a `clipPath` | Ignored |
| A `clip-path` that points at a missing element, at something that is not a `clipPath`, or in a cycle | Drawn without it |
| Animation without a channel, `animateMotion`, `set` and the CSS animations listed under [Animation](#animation) | The static frame is drawn |
| `pattern` paint | Fallback color if given, otherwise none |
| A marker that points at a missing element, at something that is not a `marker`, or in a reference cycle | Drawn without it |
| `marker-*` values that are not `url(#id)` | Drawn without markers |
| `foreignObject` | Skipped |
| `image` that is GIF, WebP or compressed SVG (`.svgz`), or points at a web address | Skipped, because MSX holds PNG and JPEG and the importer reads plain SVG |
| Animation inside an SVG `image` | The inner document is drawn as its static frame |
| An SVG `image` that already contains the document it sits in, or sits more than four documents deep | Skipped |
| `image-rendering` other than `auto` and `optimizeQuality` | Drawn with smooth scaling |
| `textPath` | Its text is skipped |
| `spreadMethod` of `reflect` or `repeat` | Imported as `pad` |
| `fr` on a radial gradient, a focal point outside the circle | `fr` ignored, focal point moved onto the circle |
| `context-fill` and `context-stroke` outside a marker or a `use` | Painted as none, because there is no context |
| A gradient used through `context-fill` or `context-stroke` | Fitted to the shape that is painted, not to the box of the context element |
| Blend modes beyond the nine above | Drawn normally |
| CSS selectors with sibling combinators (`+`, `~`), pseudo-classes other than `:first-child`, and pseudo-elements | The rule is skipped |
| Spans with different styles on one text line | The style of the line start is used |
| `text-decoration`, `letter-spacing`, `word-spacing`, `font-style`, `writing-mode`, `vector-effect` | Ignored, with a warning when the value is not the initial one |
| A user-space gradient on a shape with no bounding box area | The average color of its stops |

Files that are not UTF-8 are rejected with an error. Elements are capped at 200,000 shapes, so an exponentially nested `use` chain cannot run away. A `use` that refers back to itself is skipped.

## In the browser

The site has a [converter page](/convert/) that runs the importer in a WebAssembly build. An SVG is pasted, dropped on the editor or opened from disk. The page shows the `.msx` source the importer wrote, the original drawn by the browser next to the converted scene, and every warning. An animated SVG plays in the converted pane with a play bar, driven by the same frame resolver as `msx animate`. The original runs on the browser's own clock, so the two panes are not in step. The source can be copied or downloaded, and the compiled binary can be downloaded too. The conversion is the same code as `msx import`, and the preview is drawn from the written source parsed back, so the preview shows the file as saved.

The status of each SVG feature is on the [SVG parity](svg-parity.md) page.

## Checking an import

`scripts/svg-compare/` renders SVG files with the importer and the CPU renderer and compares each against reference renderers. The script `compare.py` takes a mode and a folder, `summarize.py` groups the result, and `sidebyside.py` writes a sheet of the importer output, the reference and the difference for chosen files. The scripts need `rsvg-convert`, Pillow and NumPy, and use the resvg test suite as an optional second reference.
