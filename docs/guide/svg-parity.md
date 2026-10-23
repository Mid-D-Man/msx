# SVG parity

This page tracks how much of SVG an imported file keeps. A feature ends up in one of three states. It is **native** when the MSX format and the renderers have it. It is **expanded** when the importer rewrites it into native features, so nothing is lost. It is a **gap** when the importer reports it as a warning and draws the file without it. The importer never drops a feature without saying so, with the exceptions marked "not read" below.

Progress is measured with `scripts/svg-compare/`. At the last full run the importer matched all 54 icons of a four set corpus (Feather, Bootstrap Icons, Simple Icons, Twemoji) and 803 of the 954 files in the resvg shapes, painting, paint server, structure and masking tests. Of the 52 `clipPath` tests, 47 match, of the 40 `mask` tests 38, and of the 56 `image` tests 31. The rest of the `image` tests use SVG, GIF or WebP images. Nearly every file that does not match uses a feature listed as a gap here.

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
| `linearGradient` | Expanded | Exact under any transform |
| `radialGradient` | Expanded, partly | Exact under uniform scale, rotation and translation. A stretch or skew is approximated by a circle of equal area |
| `gradientUnits`, `gradientTransform` | Expanded | Converted into the bounding box of each shape, because the format has neither |
| `spreadMethod` `reflect` and `repeat` | Gap | Imported as `pad` |
| Focal point outside the circle, `fr` | Gap | Clamped onto the circle, `fr` ignored |
| Conic gradient | Native, MSX only | CPU and GPU draw it, the SVG output cannot |
| `pattern` | Gap | Fallback color if given |

## Compositing and effects

| SVG feature | State | Notes |
|---|---|---|
| Group `opacity` | Expanded | An isolated layer |
| `mix-blend-mode` | Expanded | Nine modes, others draw normally |
| `isolation` | Expanded | A layer |
| `clip-path` with `url(#id)` | Expanded | A layer with a clip path. Both units, `clip-rule`, `transform` and nested clip paths work on all three renderers |
| `clip-path` as a basic shape such as `circle()` | Gap | Reported |
| `clip-path` on a child inside a `clipPath` | Gap | Reported, the child is not clipped |
| `mask` with `url(#id)` | Expanded | A layer with a mask. Both units, the region, `mask-type`, nested masks and any paint in the mask work on all three renderers |
| `mask` in a reference cycle, `color-interpolation: linearRGB` | Gap | Reported |
| `filter` | Gap | Layers carry blur and shadow effects natively, but the importer does not map filter primitives to them yet |
| `marker-start`, `marker-mid`, `marker-end`, `marker` in CSS | Expanded | Copies of the marker content at the vertices, on paths, lines, polylines, polygons, rectangles and ellipses. All marker attributes work, and the viewport clips |

## Structure

| SVG feature | State | Notes |
|---|---|---|
| `g`, `defs`, `use`, `symbol`, nested `svg` | Expanded | Placement, `viewBox` and viewport clipping are kept, and `overflow` is honored |
| `viewBox`, `preserveAspectRatio` | Expanded | One wrapper transform |
| `transform`, CSS `transform`, `transform-origin` | Expanded | `transform-box` other than `view-box` is a gap |
| `switch`, `systemLanguage`, `requiredExtensions` | Expanded | The language is English |
| `<style>` rules | Partial | Type, id and class selectors and their compounds. Combinators, attributes and pseudo-classes are skipped |
| `image` with PNG or JPEG | Expanded | From a data URI or a file, with `preserveAspectRatio`, a clip for `slice`, opacity, transform, clip and mask |
| `image` with SVG, GIF or WebP | Gap | Reported. MSX holds only PNG and JPEG, and an SVG image would need the inner file imported into the scene |
| `foreignObject` | Gap | Reported |
| `animate`, `animateTransform`, `animateMotion`, `set`, CSS `animation` | Gap | Reported, the static frame is drawn. MSX has animation tracks, but the importer does not map to them yet and the writer refuses animated scenes |

## Text

| SVG feature | State | Notes |
|---|---|---|
| `text`, `tspan` with positions, anchor, size and weight | Native | A `tspan` with its own position becomes its own text element |
| `font-family` | Not read | Text uses the embedded DejaVu Sans subset |
| `font-style`, `text-decoration`, `letter-spacing`, `word-spacing`, `writing-mode` | Gap | Reported |
| Different styles inside one line | Gap | The style of the line start is used |
| `textPath` | Gap | |
| `dominant-baseline` | Partial | Honored from source, lost when compiled to binary |

## Renderers

| Behavior | SVG | CPU | GPU |
|---|---|---|---|
| Style inherited from a group | Yes | No | No |
| `viewbox` block | Yes | No | No |
| Layer effects | All | All | Blur only |
| Edge antialiasing | Yes | Yes | No |

Imported files do not depend on the rows marked No, because the importer writes explicit style on every shape and a single wrapper transform.

## Order of work

The remaining gaps share a few causes, so they are closed in this order.

1. Done: a clip path on layers. It closes `clip-path` and the `symbol` and nested `svg` viewport clipping. What is left of it is the clip on a clip path child and the basic shapes. Masks followed on the same layer machinery and are done too.
2. Gradient units, a gradient matrix, spread modes and focal points in the format. This makes every gradient exact.
3. Patterns, as a tiled group with its own transform.
4. Done: markers, expanded by the importer into copies at the vertices.
5. Done for PNG and JPEG. What is left is SVG, GIF and WebP images.
6. Filter primitives mapped onto layer effects where an equivalent exists.
7. Text styling, then `textPath`.
8. A binary version bump with slots for `fill_opacity`, `stroke_opacity` and `dominant_baseline`.
