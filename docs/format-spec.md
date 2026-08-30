# MSX — MidStroke eXtension
## Format Specification

*Tracks the binary format's actual `VERSION` byte (currently `2`) and this
repo's current `main` — kept up to date alongside the code, not written
once and left to drift. If something here disagrees with `core/msx-binary`
or `core/msx-ast`, the code is right; that's a doc bug, flag it.*

---

## Overview

MSX is a two-layer vector image format:

1. **Source layer** — A DixScript `.msx` file. The human-readable authoring surface.  
2. **Binary layer** — A compact typed-stream encoding of the evaluated scene graph, optionally MBFA-compressed.

The source layer is optional at runtime. A compiled binary `.msx` file is entirely self-contained and renders to SVG (or PNG, via `msx-render-cpu`/`msx-render-gpu`) without the original source.

---

## Why MSX?

SVG is XML. Nobody writes XML by hand for complex or parametric graphics. MSX source files are **DixScript** — the same `.mdix` format you already use for configs, now driving vectors. The key insight:

| SVG | MSX Source |
|---|---|
| Copy-paste the same `<circle>` 50 times | One QuickFunc `~dot(x,y,r,color)`, call it 50 times |
| No reusable components without JS | QuickFuncs compose freely, evaluated at compile time |
| Gradient/clip IDs must be manually unique | Defs are DixScript objects; IDs are data |
| Verbosity: ~1.2KB per annotated shape | Compact typed binary; MBFA compresses repeating structure |
| Zero compression on SVG | MBFA multi-fold LZ on binary coordinate + opcode streams |

---

## Source Format (DixScript Schema)

An `.msx` file is a valid DixScript file. The MSX compiler evaluates it and produces a `Scene` AST.

### Section contract

| Section | Required | Purpose |
|---|---|---|
| `@CONFIG` | No | Format version, metadata |
| `@ENUMS` | No | Named constants (BlendMode, LineCap, FillRule…) |
| `@QUICKFUNCS` | No | Parametric shape generators and component libraries |
| `@DATA` | **Yes** | Scene definition — canvas, defs, elements |

### `@DATA` top-level keys

```dixscript
@DATA(
  // Canvas — required
  scene: { width = <float>, height = <float>, background = <color> }

  // Optional viewbox
  viewbox: { min_x = <float>, min_y = <float>, width = <float>, height = <float> }

  // Optional gradient / pattern defs (referenced by elements via url(#id))
  defs::
    <def_object>...

  // Element tree — required
  elements::
    <element_object>...
)
```

---

## Element Objects

Every element is a DixScript object literal or a QuickFunc call that returns one.  
All elements share optional `id`, `transform`, and `style` keys.

### `rect`

```dixscript
{
  type   = "rect"
  x      = <float>           // top-left x
  y      = <float>           // top-left y
  width  = <float>
  height = <float>
  rx     = <float>?          // corner radius x (omit for sharp corners)
  ry     = <float>?          // corner radius y (defaults to rx if omitted)
  id        = <string>?
  transform = <transform>?
  style     = <style>
}
```

### `circle`

```dixscript
{
  type = "circle"
  cx   = <float>
  cy   = <float>
  r    = <float>
  id = <string>?  transform = <transform>?  style = <style>
}
```

### `ellipse`

```dixscript
{
  type = "ellipse"
  cx   = <float>   cy = <float>
  rx   = <float>   ry = <float>
  id = <string>?  transform = <transform>?  style = <style>
}
```

### `line`

```dixscript
{
  type = "line"
  x1 = <float>  y1 = <float>
  x2 = <float>  y2 = <float>
  id = <string>?  transform = <transform>?  style = <style>
}
```

### `polyline`

```dixscript
{
  type   = "polyline"
  points = [ [<float>, <float>] ... ]    // array of [x, y] pairs
  id = <string>?  transform = <transform>?  style = <style>
}
```

### `polygon`

Same as `polyline` but closed. `type = "polygon"`.

### `path`

```dixscript
{
  type = "path"
  d    = <string>     // standard SVG path data — M L H V C S Q T A Z (abs + rel)
  id = <string>?  transform = <transform>?  style = <style>
}
```

Supports interpolated strings for parametric paths:

```dixscript
d = $"M {cx - half} {cy} L {cx + half} {cy} Z"
```

### `text`

```dixscript
{
  type    = "text"
  x       = <float>
  y       = <float>     // baseline y
  content = <string>
  id = <string>?  transform = <transform>?  style = <style>
}
```

### `group`

```dixscript
{
  type     = "group"
  elements = [ <element> ... ]     // recursive
  id        = <string>?
  transform = <transform>?
  style     = <style>?             // inheritable styles only; applied to all children
}
```

### `use`

References a def by id. Used to stamp out gradient-filled or reused shapes.

```dixscript
{
  type      = "use"
  href      = "#<id>"
  x         = <float>?
  y         = <float>?
  transform = <transform>?
}
```

### `image`

```dixscript
{
  type       = "image"
  source_ref = <string>?     // path, resolved relative to the source file — mutually exclusive with `data`
  data       = <string>?     // base64-encoded PNG/JPEG/GIF bytes, embedded inline
  x          = <float>       // anchor point's canvas position — NOT necessarily the
  y          = <float>       // rendered top-left corner; see `anchor` below
  width      = <float>
  height     = <float>
  anchor     = "top_left" | "center" | "top_right" | "bottom_left" | "bottom_right"?   // default "top_left"
  id = <string>?  transform = <transform>?  style = <style>
}
```

Embedded `data` is format-sniffed at parse time (PNG/JPEG/GIF magic bytes) — base64 that decodes to anything else is a parse error, unlike `audio`'s def below.

### `sdf`

A signed-distance-field shape tree — `tree` is a recursive node graph (primitives combined with union/subtract/intersect, optionally smoothed, optionally offset), evaluated per-pixel by every renderer rather than tessellated.

```dixscript
{
  type          = "sdf"
  tree          = <sdf_node>          // see below
  fill          = <paint>
  stroke        = <paint>?
  stroke_width  = <float>?
  id = <string>?  transform = <transform>?
}
```

`<sdf_node>` is one of:

```dixscript
{ type = "circle",   cx = <float>, cy = <float>, r = <float> }
{ type = "box",       cx = <float>, cy = <float>, hx = <float>, hy = <float>, corner_radius = <float>? }
{ type = "line",      x1 = <float>, y1 = <float>, x2 = <float>, y2 = <float>, thickness = <float> }
{ type = "ring",      cx = <float>, cy = <float>, r = <float>, thickness = <float> }
{ type = "arc",       cx = <float>, cy = <float>, r = <float>, thickness = <float>, start_angle = <float>, sweep_angle = <float> }
{ type = "union",            a = <sdf_node>, b = <sdf_node> }
{ type = "smooth_union",     a = <sdf_node>, b = <sdf_node>, k = <float> }
{ type = "subtract",         a = <sdf_node>, b = <sdf_node> }
{ type = "smooth_subtract",  a = <sdf_node>, b = <sdf_node>, k = <float> }
{ type = "intersect",        a = <sdf_node>, b = <sdf_node> }
{ type = "smooth_intersect", a = <sdf_node>, b = <sdf_node>, k = <float> }
{ type = "offset",    node = <sdf_node>, amount = <float> }
```

### `splat`

A single 2D Gaussian splat — an elliptical falloff, not a hard-edged shape. Has no `transform` field (position/rotation are its own explicit fields instead).

```dixscript
{
  type     = "splat"
  x        = <float>            // center
  y        = <float>
  sigma_x  = <float>             // standard deviation along the local x axis
  sigma_y  = <float>             // standard deviation along the local y axis
  rotation = <float>             // radians
  color    = <color>             // peak color at center — the fallback every renderer uses
  opacity  = <float>
  fill     = <paint>?            // overrides `color` where present; see msx-ast's own doc for how the two interact
  id = <string>?
}
```

### `layer`

An isolated compositing group: children render into their own offscreen buffer (cleared to transparent, not the canvas background) before that buffer composites onto its parent at `opacity`, through `blend_mode`, with `effects` applied first. This is where blend modes, post-processing effects, and clip live — an ordinary `group` has none of these.

```dixscript
{
  type       = "layer"
  elements   = [ <element> ... ]
  blend_mode = "normal" | "multiply" | "screen" | "overlay" | "add" | "soft_light"
             | "hard_light" | "difference" | "exclusion" | "darken" | "lighten"
             | "subtract" | "divide"                       // default "normal"
  opacity    = <float>?                                     // default 1.0
  clip       = <bool>?                                      // clip children to this layer's pixel footprint; default false
  z_index    = <float>?                                     // paint order among SIBLING top-level Layers only — see note below
  effects = [
    { type = "blur", radius = <float> }
    { type = "drop_shadow",  offset_x = <float>, offset_y = <float>, blur_radius = <float>, color = <color>, opacity = <float> }
    { type = "inner_shadow", offset_x = <float>, offset_y = <float>, blur_radius = <float>, color = <color>, opacity = <float> }
    { type = "outer_glow",   color = <color>, blur_radius = <float>, spread = <float>, opacity = <float> }
    { type = "inner_glow",   color = <color>, blur_radius = <float>, opacity = <float> }
    ...
  ]
  id = <string>?  transform = <transform>?
}
```

**Rendering support today:** all three renderers (SVG/CPU/GPU) agree on Layer nesting semantics (a nested Layer's own z_index/opacity resolve at its own level first, then the parent participates as one unit in its sibling list) and on shader fills reaching shapes inside a Layer. `blend_mode` and `Effect::Blur` are implemented on GPU; `DropShadow`/`InnerShadow`/`OuterGlow`/`InnerGlow` are not yet — a Layer using one of those four renders today exactly as if that effect weren't present, on GPU specifically (CPU already implements all five).

`z_index` is purely relative: only compared against other top-level Layers, never against ordinary (non-Layer) elements, which always draw before every Layer regardless of z_index. Ties break by document order (a stable sort, same convention as CSS `z-index`).

---

## Def Objects

Defined in the `defs::` group array. Referenced via `"url(#id)"` in paint values.

### `linear_gradient`

```dixscript
{
  type = "linear_gradient"
  id   = <string>
  x1   = <float>    // 0.0..1.0 in gradient space (or px if gradientUnits = "userSpaceOnUse")
  y1   = <float>
  x2   = <float>
  y2   = <float>
  stops = [
    { offset = <float>, color = <color>, opacity = <float> }
    ...
  ]
}
```

### `radial_gradient`

```dixscript
{
  type = "radial_gradient"
  id   = <string>
  cx   = <float>    // center x, 0.0..1.0
  cy   = <float>    // center y
  r    = <float>    // radius
  fx   = <float>?   // focal point x (defaults to cx)
  fy   = <float>?
  stops = [ ... ]
}
```

### `conic_gradient`

Sweeps stops around a center point, starting at `angle` (radians).

```dixscript
{
  type  = "conic_gradient"
  id    = <string>
  cx    = <float>
  cy    = <float>
  angle = <float>
  stops = [ ... ]
}
```

### `shader`

A `Def::Shader` — real WGSL, executed only by `msx-render-gpu` (the `gpu` Cargo feature). Every other renderer paints `fallback_color` wherever this shader is referenced as a fill.

```dixscript
{
  type          = "shader"
  id            = <string>
  source_ref    = <string>          // path to a .wgsl file, resolved relative to the source file
  entry_point   = <string>          // fragment shader entry point name
  fallback_color = <color>          // what CPU/SVG paint instead
  uniforms = [
    { name = <string>, value = <float> | [<float>,<float>] | [<float>,<float>,<float>] | [<float>,<float>,<float>,<float>] }
    ...
  ]
}
```

### `audio`

A `Def::Audio` — has no canvas position (it's a resource, not a drawable element). Nothing in this project currently plays audio; see the CLI Reference's `extract-media` entry for the one way to get the bytes back out today.

```dixscript
{
  type       = "audio"
  id         = <string>
  source_ref = <string>?      // path to a .wav/.ogg/.mp3, mutually exclusive with `data`
  data       = <string>?      // base64-encoded audio bytes, embedded inline
}
```

Both `source_ref` and `data` go through the same `MediaSource` machinery `Element::Image` uses (see below) — a def or element is `MediaSource::FileRef(path)` or `MediaSource::Embedded(bytes)`, never both. Unlike images, audio parsing does **not** format-sniff the decoded bytes at parse time — there's no render-time failure mode to protect against yet, since nothing renders (or plays) audio.

---

## Style Object

All keys are optional. Unset keys inherit from the parent group or fall back to defaults.

```dixscript
{
  fill              = <paint>        // default "black"
  stroke            = <paint>        // default "none"
  stroke_width      = <float>        // default 1.0
  opacity           = <float>        // 0.0..1.0, default 1.0
  fill_opacity      = <float>?
  stroke_opacity    = <float>?
  fill_rule         = "nonzero" | "evenodd"
  stroke_linecap    = "butt" | "round" | "square"
  stroke_linejoin   = "miter" | "round" | "bevel"
  stroke_miterlimit = <float>?
  stroke_dasharray  = [<float> ...]?
  stroke_dashoffset = <float>?
  font_size         = <float>?
  font_family       = <string>?
  font_weight       = "normal" | "bold" | <int>?
  text_anchor       = "start" | "middle" | "end"
  dominant_baseline = <string>?
  visibility        = "visible" | "hidden"
  display           = "inline" | "none"
}
```

**Paint values:**

| Syntax | Meaning |
|---|---|
| `"none"` | Transparent / no paint |
| `"#rrggbb"` | Opaque hex color |
| `"#rrggbbaa"` | Hex color with alpha |
| `"rgb(r, g, b)"` | Functional RGB |
| `"rgba(r, g, b, a)"` | Functional RGBA |
| `"url(#id)"` | Reference to a gradient or pattern def |
| `"currentColor"` | Inherited color value |

---

## Transform Values

As a string (SVG syntax):

```
"translate(tx, ty)"
"scale(sx)"  |  "scale(sx, sy)"
"rotate(deg)"  |  "rotate(deg, cx, cy)"
"skewX(deg)"  |  "skewY(deg)"
"matrix(a, b, c, d, e, f)"
```

Or as a DixScript object:

```dixscript
{ type = "translate",  x = <float>, y = <float> }
{ type = "scale",      x = <float>, y = <float> }
{ type = "rotate",     angle = <float>, cx = <float>?, cy = <float>? }
{ type = "matrix",     a = <float>, b = <float>, c = <float>, d = <float>, e = <float>, f = <float> }
{ type = "skew_x",     angle = <float> }
{ type = "skew_y",     angle = <float> }
```

Or as an array for chained transforms (applied right-to-left):

```dixscript
transform = [
  { type = "rotate", angle = 45 }
  { type = "translate", x = 100, y = 0 }
]
```

---

## Path Command String

The `d` string in `path` elements follows standard SVG path syntax.

| Command | Absolute | Relative | Arguments |
|---|---|---|---|
| Move to | `M` | `m` | `x y` |
| Line to | `L` | `l` | `x y` |
| Horizontal line | `H` | `h` | `x` |
| Vertical line | `V` | `v` | `y` |
| Cubic bezier | `C` | `c` | `cx1 cy1 cx2 cy2 x y` |
| Smooth cubic | `S` | `s` | `cx2 cy2 x y` |
| Quadratic bezier | `Q` | `q` | `cx cy x y` |
| Smooth quadratic | `T` | `t` | `x y` |
| Arc | `A` | `a` | `rx ry x-rotation large-arc-flag sweep-flag x y` |
| Close path | `Z` | `z` | — |

MSX source supports DixScript interpolated strings for parametric paths:

```dixscript
@QUICKFUNCS(
  ~arrow<object>(x1, y1, x2, y2, color) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    return {
      type  = "path"
      d     = $"M {x1} {y1} L {x2} {y2} L {x2 - dx * 0.2} {y2 - dy * 0.2 - 8}"
      style = { fill = "none", stroke = color, stroke_width = 2.0,
                stroke_linecap = "round", opacity = 1.0 }
    }
  }
)
```

---

## Binary Format

### File Header (32 bytes)

| Offset | Size | Field | Type | Notes |
|---|---|---|---|---|
| 0 | 4 | magic | `[u8; 4]` | `0x4D 0x53 0x58 0x00` — "MSX\0" |
| 4 | 1 | version | `u8` | `2` — bumped when new **tags** would break an old decoder's assumptions about tag-driven dispatch (last bumped for Sdf/Splat/Layer/ConicGradient). New tags/flags that an old decoder can just never encounter or safely ignore (Image/Audio, the animations flag below) do **not** bump this — see "Wire-format versioning" below. |
| 5 | 1 | compress | `u8` | `0`=none `1`=mbfa |
| 6 | 1 | flags | `u8` | bit0=has_viewbox bit1=has_metadata bit2=has_defs bit3=has_animations |
| 7 | 1 | reserved | `u8` | zero |
| 8 | 4 | width | `f32 LE` | canvas width in user units |
| 12 | 4 | height | `f32 LE` | canvas height in user units |
| 16 | 4 | elem_count | `u32 LE` | top-level element count |
| 20 | 4 | str_pool_len | `u32 LE` | string pool byte length |
| 24 | 4 | def_count | `u32 LE` | def count (gradients, shaders, audio) |
| 28 | 4 | reserved | `[u8; 4]` | zeros |

#### Wire-format versioning

New tags/flags are added additively wherever the decoder can either dispatch on the tag itself (a new element/def tag an old decoder simply never emits or reads) or gate a whole optional section behind a flag bit an old decoder never checks. Both of those degrade **gracefully** on version mismatch rather than crashing — an old decoder either never encounters the new tag at all (the file simply doesn't use the feature it postdates), or, for a flag-gated section like animations, silently doesn't read past where it always stopped, same "silently drops what it doesn't know about" behavior as before that section existed, not a new failure mode. Only changes to tags an old decoder actively dispatches on and would now misinterpret bump `version`.

### Payload Layout (after header; optionally MBFA-compressed as a unit)

```
Background RGBA     4 bytes      [u8 r][u8 g][u8 b][u8 a]
Viewbox             16 bytes     [f32 min_x][f32 min_y][f32 w][f32 h]   (only if flags.bit0)
String pool         variable     [u16 count] ([u16 byte_len][utf8 bytes])*
Def section         variable     def_count × def
Element stream      variable     elem_count × element, then [u8 0xFF]  (TAG_END sentinel)
Animation section    variable     only if flags.bit3 — see "Animation Section" below
```

The `TAG_END` sentinel is always written after the element stream, regardless of whether an animation section follows — decoding always reads exactly `elem_count` elements (never scans for the sentinel) and then consumes that one byte before deciding, from `flags.bit3`, whether to read further.

### Element Type Tags

| Tag | Element | Since |
|---|---|---|
| `0x00` | Rect | v0.1 |
| `0x01` | Circle | v0.1 |
| `0x02` | Ellipse | v0.1 |
| `0x03` | Line | v0.1 |
| `0x04` | Polyline | v0.1 |
| `0x05` | Polygon | v0.1 |
| `0x06` | Path | v0.1 |
| `0x07` | Text | v0.1 |
| `0x08` | Group | v0.1 |
| `0x09` | Use | v0.1 |
| `0x0A` | LinearGradient (def only) | v0.1 |
| `0x0B` | RadialGradient (def only) | v0.1 |
| `0x0C` | ConicGradient (def only) | v0.2 |
| `0x0D` | Sdf | v0.2 |
| `0x0E` | Splat | v0.2 |
| `0x0F` | Layer | v0.2 |
| `0x10` | Shader (def only) | v0.3 |
| `0x11` | Image | v0.4 |
| `0x12` | Audio (def only) | v0.4 |
| `0xFF` | End sentinel | v0.1 |

("Since" tracks when each tag was added to `tags.rs`, not the file-header `version` byte — see "Wire-format versioning" above for why those two numbers don't move together.)

### Element Wire Format

```
[u8  tag]
[u8  id_flags]        bit0 = has_id   bit1 = has_transform
[u16 id_str_idx]      (only if id_flags.bit0)
[transform block]     (only if id_flags.bit1)
[geometry fields]     (tag-specific — see below)
[style block]         (most tags — Sdf and Splat use `fill`/`stroke` paint fields directly instead, Layer has none)
```

`Splat` (`0x0E`) has no `transform` field on the AST at all, so it skips the shared `id_flags` header entirely in favor of a simpler one: `[u8 has_id] [u16 id_str_idx if has_id]`.

#### Geometry fields by tag

| Tag | Fields (all f32 LE unless noted) |
|---|---|
| `0x00` Rect | `x y width height rx ry` |
| `0x01` Circle | `cx cy r` |
| `0x02` Ellipse | `cx cy rx ry` |
| `0x03` Line | `x1 y1 x2 y2` |
| `0x04` Polyline | `[u32 count] [f32 x y]*` |
| `0x05` Polygon | `[u32 count] [f32 x y]*` |
| `0x06` Path | `[u32 cmd_count] [path_command]*` |
| `0x07` Text | `[u16 str_idx] [f32 x] [f32 y]` |
| `0x08` Group | `[u32 child_count] [element]*` |
| `0x09` Use | `[u16 href_str_idx] [f32 x] [f32 y]` |
| `0x0A` LinearGradient | `[u16 id_str_idx] [f32 x1 y1 x2 y2] [u16 stop_count] [stop]*` |
| `0x0B` RadialGradient | `[u16 id_str_idx] [f32 cx cy r fx fy] [u16 stop_count] [stop]*` |
| `0x0C` ConicGradient | `[u16 id_str_idx] [f32 cx cy angle] [u16 stop_count] [stop]*` |
| `0x0D` Sdf | `[sdf_tree] [paint fill] [u8 has_stroke] ([paint stroke] [f32 stroke_width] if has_stroke)` — note: no style block, `fill`/`stroke` are direct paint fields |
| `0x0E` Splat | `[f32 x y sigma_x sigma_y rotation] [u8 r g b a] [f32 opacity] [u8 has_fill] ([paint fill] if has_fill)` |
| `0x0F` Layer | `[u8 blend_mode] [f32 opacity] [u8 clip] [u16 effect_count] [effect]* [u32 child_count] [element]* [f32 z_index]` — no style block |
| `0x10` Shader | `[u16 id_str_idx source_ref_str_idx entry_point_str_idx] [u8 r g b a fallback_color] [u16 uniform_count] ([u16 name_str_idx] [uniform_value])*` |
| `0x11` Image | `[media_source] [f32 x y width height] [u8 anchor]`, then the shared style block |
| `0x12` Audio | `[u16 id_str_idx] [media_source]` — no style block, def only |

#### Gradient stop

```
[f32 offset]      0.0..1.0
[u8  r g b a]
```

#### Shader uniform value

```
[u8 type]
  0x00  Float   [f32]
  0x01  Vec2    [f32 x y]
  0x02  Vec3    [f32 x y z]
  0x03  Vec4    [f32 x y z w]
```

#### MediaSource encoding

Shared by `Image` and `Audio` — one or the other, never both, decoded at read time only from the discriminant byte:

```
[u8 kind]
  0x00  FileRef    [u16 path_str_idx]                    // through the shared string pool
  0x01  Embedded   [u32 byte_len] [u8 bytes]*             // NOT the string pool — see note below
```

Embedded bytes use their own `u32`-length-prefixed block rather than the string pool: the pool is `u16`-length-prefixed per entry (65,535-byte ceiling per string), which is fine for paths/ids but would silently truncate a real embedded image or audio file past that size. base64 **decoding** happens only in `msx-parser` (source → bytes, at parse time); base64 **re-encoding** happens only in `msx-render-svg` (bytes → `data:` URI, at SVG-emit time) — the binary format itself and every other renderer only ever handle raw bytes.

#### SDF tree encoding (`sdf_tree`)

A recursive node graph — combinators recurse into their own child node(s) inline, no separate length prefix needed since each node's own tag determines how many more nodes follow.

```
[u8 tag]
  0x00  Circle           [f32 cx cy r]
  0x01  Box              [f32 cx cy hx hy] [u8 has_corner_radius] ([f32 corner_radius] if present)
  0x02  Line             [f32 x1 y1 x2 y2 thickness]
  0x03  Ring             [f32 cx cy r thickness]
  0x04  Arc              [f32 cx cy r thickness start_angle sweep_angle]
  0x05  Union            [sdf_tree a] [sdf_tree b]
  0x06  SmoothUnion      [sdf_tree a] [sdf_tree b] [f32 k]
  0x07  Subtract         [sdf_tree a] [sdf_tree b]
  0x08  SmoothSubtract   [sdf_tree a] [sdf_tree b] [f32 k]
  0x09  Intersect        [sdf_tree a] [sdf_tree b]
  0x0A  SmoothIntersect  [sdf_tree a] [sdf_tree b] [f32 k]
  0x0B  Offset           [sdf_tree node] [f32 amount]
```

#### Effect encoding

```
[u8 tag]
  0x00  Blur          [f32 radius]
  0x01  DropShadow    [f32 offset_x offset_y blur_radius] [u8 r g b a color] [f32 opacity]
  0x02  InnerShadow   [f32 offset_x offset_y blur_radius] [u8 r g b a color] [f32 opacity]
  0x03  OuterGlow     [u8 r g b a color] [f32 blur_radius spread opacity]
  0x04  InnerGlow     [u8 r g b a color] [f32 blur_radius opacity]
```

#### BlendMode byte values

`Normal` is the `#[default]` (byte `0`). This exact numbering is what a `Layer`'s `blend_mode` field serializes to, and is also what GPU's blend shader receives directly as its `mode` uniform — no remapping table between the wire format and the shader.

| Byte | Mode | Byte | Mode |
|---|---|---|---|
| 0 | Normal | 7 | Difference |
| 1 | Multiply | 8 | Exclusion |
| 2 | Screen | 9 | Darken |
| 3 | Overlay | 10 | Lighten |
| 4 | Add | 11 | Subtract |
| 5 | SoftLight | 12 | Divide |
| 6 | HardLight | | |

### Animation Section

Only present when header `flags.bit3` (has_animations) is set — gated on whether the scene has any non-default animation-related state (`duration`, `loop_mode`, or a non-empty `animations` list), not on whether the scene is *currently* animated in the rendering sense (a `duration`/`loop_mode` explicitly authored alongside zero real keyframe tracks still round-trips through this section — that's a fidelity guarantee, not a rendering one).

```
[f32 duration]
[u8  loop_mode]            0=Once  1=Loop  2=PingPong  (#[default] = Once)
[u16 track_count]
  track:
    [u16 target_id_str_idx]     // through the shared string pool
    [u8  property]              // AnimatedProperty discriminant
    [u16 keyframe_count]
      keyframe:
        [f32 time]
        [f32 value]
        [u8  easing]             // Easing discriminant
```

`target_id` goes through the shared string pool rather than being inlined — several properties on the same element each get their own track, so the same id repeats often enough that pool dedup is worth it, same reasoning as `MediaSource::FileRef` paths.

**Rendering support:** `msx-anim::resolve_at_time` reads this data to drive `msx-viewer`'s live keyframe playback and `msx animate`'s GIF export. It does not drive the GPU shader `time` uniform — that's a separate clock (see `msx animate-gpu` in the CLI Reference below), and live GPU playback in the viewer hasn't been started.

### Style Block

```
[u8 present_flags]
  bit 0 = fill present
  bit 1 = stroke present
  bit 2 = opacity present
  bit 3 = stroke_width present
  bit 4 = fill_rule + linecap + linejoin present
  bit 5 = font fields present
  bit 6 = dash present
  bit 7 = visibility / display present

[paint]                     if bit 0   (fill)
[paint]                     if bit 1   (stroke)
[f32]                       if bit 2   (opacity)
[f32]                       if bit 3   (stroke_width)
[u8 fill_rule]              if bit 4   (0=nonzero  1=evenodd)
[u8 linecap]                           (0=butt 1=round 2=square)
[u8 linejoin]                          (0=miter 1=round 2=bevel)
[f32 miterlimit]
[u16 font_size_x100]        if bit 5   (stored as integer × 100)
[u16 font_family_str_idx]
[u8  font_weight]                      (0=normal 1=bold 2..=numeric/100)
[u8  text_anchor]                      (0=start 1=middle 2=end)
[u16 dash_count]            if bit 6
[f32 dash]*
[f32 dashoffset]
[u8  vis_display_flags]     if bit 7   bit0=hidden  bit1=display_none
```

Note: `Sdf` and `Layer` don't have a style block at all — `Sdf` has direct `fill`/`stroke` paint fields instead (see its geometry-fields row above), and `Layer` has no fill/stroke concept at all (it's a compositing group, not a drawable shape).

### Paint Encoding

```
[u8 type]
  0x00  None                (transparent)
  0x01  Color               [u8 r][u8 g][u8 b][u8 a]
  0x02  Gradient reference  [u16 str_idx]   (points into string pool)
  0x03  Pattern reference   [u16 str_idx]
```

### Transform Block

```
[u8 type]
  0x00  None              (no further bytes)
  0x01  Matrix            [f32 a b c d e f]
  0x02  Translate         [f32 tx ty]
  0x03  Scale             [f32 sx sy]
  0x04  Rotate            [f32 angle] [u8 has_center] ([f32 cx cy] if has_center)
  0x05  SkewX             [f32 angle]
  0x06  SkewY             [f32 angle]
  0x07  Multiple          [u8 count] [transform_block]*
```

### Path Command Encoding

```
[u8 cmd_tag]
  Absolute commands:
  0x00  MoveTo         [f32 x y]
  0x01  LineTo         [f32 x y]
  0x02  HLineTo        [f32 x]
  0x03  VLineTo        [f32 y]
  0x04  CubicBezier    [f32 cx1 cy1 cx2 cy2 x y]
  0x05  SmoothCubic    [f32 cx2 cy2 x y]
  0x06  QuadBezier     [f32 cx cy x y]
  0x07  SmoothQuad     [f32 x y]
  0x08  Arc            [f32 rx ry x_rotation] [u8 flags: bit0=large_arc bit1=sweep] [f32 x y]

  Relative commands (same wire layout, tag = absolute + 0x10):
  0x10  rel MoveTo
  0x11  rel LineTo
  0x12  rel HLineTo
  0x13  rel VLineTo
  0x14  rel CubicBezier
  0x15  rel SmoothCubic
  0x16  rel QuadBezier
  0x17  rel SmoothQuad
  0x18  rel Arc

  0xFF  ClosePath       (no further bytes)
```

---

## Compression Pipeline

When `header.compress == 1`, the entire payload (everything after the 32-byte header) is passed through `mbfa::compress(&payload, 8)` before writing and `mbfa::decompress` before reading.

**Why MBFA exploits MSX binary well:**

| Binary structure | MBFA mechanism |
|---|---|
| Repeated element tags (`0x01 cx cy r` for many circles) | Fold-1 LZ finds back-references across the opcode + geometry prefix |
| Coordinate locality (nearby shapes share high bytes of f32) | LZ window covers full coordinate stream; partial matches still compress |
| Palette reuse (same RGBA repeated across elements) | 4-byte color block → frequent LZ back-ref |
| Path command repetition (`M L L Z` patterns) | Opcode byte stream compresses like source code |
| Style blocks for uniform elements (many shapes same fill) | LZ matches the entire style block verbatim |

Unlike MPX (which splits by channel), MSX passes the stream as-is because opcode-interleaved data actually benefits from LZ finding cross-type patterns (e.g., a `circle` tag byte followed by its color is a recurring 5-byte sequence in icon sets).

---

## CLI Reference

`msx-cli`'s commands all accept either DixScript source or a compiled binary for any `<file>` argument — `load_scene`/`load_scene_bytes` sniff the `"MSX\0"` magic bytes and dispatch to `msx-parser` or `msx-binary` accordingly.

```
msx render        <file.msx>              Evaluate/decode → SVG
                  [-o out.svg]

msx compile       <source.msx>            DixScript → binary .msx
                  [-o out.msx]
                  [--no-compress]         Skip MBFA on the binary payload

msx rasterize     <file.msx>              → PNG via msx-render-cpu
                  [-o out.png]

msx rasterize-gpu <file.msx>              → PNG via msx-render-gpu instead —
                  [-o out.png]            the only path that actually executes
                  [--time <seconds>]      `Def::Shader` WGSL rather than painting
                                          `fallback_color`. Only in `--features gpu`
                                          builds; a clear error (not a panic) if no
                                          GPU adapter is available.

msx animate       <source.msx>            Samples msx-anim's keyframe timeline,
                  [-o out.gif]            exports a looping GIF via msx-render-cpu.
                  [--fps <n>]             Errors if the scene has no animation
                                          tracks — use `rasterize` for a static one.

msx animate-gpu   <source.msx>            GPU counterpart — samples BOTH clocks
                  [-o out.gif]            that can move at once (msx-anim's
                  [--fps <n>]             keyframe timeline AND a shader-def's
                                          `time` uniform) at the same t per frame.
                                          The right tool whenever a scene uses (or
                                          might use) a shader def and keyframes
                                          together.

msx info          <file.msx>              Print header + scene stats
msx validate      <source.msx>            Parse + type-check only; exit code
msx roundtrip     <source.msx>            source → binary → SVG text comparison,
                                          plus a float-tolerant check of
                                          duration/loop_mode/animations (the SVG
                                          comparison alone can't see those —
                                          static SVG output doesn't depend on them)
msx view          <file.msx>              Rasterize to a temp PNG and open it in
                                          the system's default image viewer — a
                                          quick one-shot preview, distinct from
                                          the separate `msx-viewer` app below
msx extract-media <file.msx> --id <id>    Pull a Def::Audio's raw bytes back out
                  [-o out.wav]            to a real file — `.wav`/`.ogg`/`.mp3`
                                          if the bytes sniff as one, `.bin`
                                          otherwise (sniffing is informational
                                          here, not a gate). The only way to
                                          actually verify embedded/referenced
                                          audio with a real audio-aware tool.
```

`msx-viewer` (a **separate** binary, `apps/msx-viewer` — not the `msx view` subcommand above) opens an `.msx` file in a persistent native window. It plays a scene's keyframe timeline live if it has one (see `apps/msx-viewer/src/playback.rs`) — the shader `time` uniform is a separate, GPU-only clock the viewer doesn't drive yet.

---

## Example: Parametric Badge Component

```dixscript
// badge.msx

@CONFIG( version -> "1.0.0" )

@ENUMS(
  Variant { Primary = 0, Success = 1, Warning = 2, Danger = 3 }
)

@QUICKFUNCS(
  ~badge<object>(x, y, label, color) {
    return {
      type     = "group"
      elements = [
        { type = "rect", x = x, y = y, width = 90, height = 30, rx = 15,
          style = { fill = color, stroke = "none", stroke_width = 0, opacity = 1.0 } }
        { type = "text", x = x + 45, y = y + 20, content = label,
          style = { fill = "#ffffff", font_size = 12, text_anchor = "middle",
                    font_weight = "bold", stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }

  ~card<object>(x, y, w, h, accent) {
    return {
      type     = "group"
      elements = [
        { type = "rect", x = x, y = y, width = w, height = h, rx = 8,
          style = { fill = "#ffffff", stroke = "#e0e0e0", stroke_width = 1, opacity = 1.0 } }
        { type = "rect", x = x, y = y, width = w, height = 4, rx = 2,
          style = { fill = accent, stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }
)

@DATA(
  scene: { width = 700, height = 300, background = "#f4f5f7" }

  elements::
    badge(40,  30, "primary", "#007bff")
    badge(150, 30, "success", "#28a745")
    badge(260, 30, "warning", "#ffc107")
    badge(370, 30, "danger",  "#dc3545")
    card(40,  100, 180, 160, "#007bff")
    card(250, 100, 180, 160, "#28a745")
    card(460, 100, 180, 160, "#dc3545")
)
```

---

## Scope and Non-goals

**MSX is:**
- A source format for authoring parametric/reusable vector graphics in DixScript
- A compact binary interchange format for tool-to-tool transfer, including keyframe animation data
- A compression target for MBFA on coordinate + opcode streams

**MSX is not:**
- A raster image format (that is MPX)
- A browser-native format — SVG is an export target, not the runtime format
- A format with pattern fills, clip-path/mask, or CSS-style timing functions beyond the fixed `Easing` set below — none of these exist yet in any renderer

**Current feature scope:**

| Feature | Status |
|---|---|
| rect, circle, ellipse, line, polygon, polyline | ✅ all three renderers (SVG/CPU/GPU) |
| path (all SVG commands) | ✅ all three renderers |
| text | ✅ all three renderers |
| group + transform | ✅ all three renderers |
| linear, radial, conic gradient | ✅ all three renderers |
| use / def referencing | ✅ all three renderers |
| SDF shape trees (circle/box/line/ring/arc + boolean combinators) | ✅ all three renderers |
| Gaussian splats | ✅ all three renderers |
| Image (file-ref or embedded, PNG/JPEG/GIF) | ✅ all three renderers |
| Audio (file-ref or embedded) as a def | ✅ round-trips losslessly; nothing anywhere plays it — `extract-media` is the only way to pull it back out and verify with a real tool |
| Layer (isolated compositing group) | ✅ all three renderers agree on nesting/paint-order semantics |
| Layer blend modes (Multiply/Screen/etc.) | ✅ CPU, SVG, GPU |
| Layer effects — Blur | ✅ CPU, SVG, GPU |
| Layer effects — DropShadow/InnerShadow/OuterGlow/InnerGlow | ✅ CPU, SVG · ❌ GPU (flagged, not silent — see `msx-render-gpu`'s `effects.rs`) |
| Shader defs (real WGSL fills) | ✅ GPU only (`--features gpu`) — CPU/SVG paint `fallback_color` instead, by design |
| Keyframe animation (`animations::`, `duration`, `loop_mode`) | ✅ parses, compiles to binary, round-trips, bakes to GIF (`msx animate`/`msx animate-gpu`), plays live in `msx-viewer` |
| Live GPU playback (shader `time` uniform driven continuously in the viewer) | ❌ not started — see `apps/msx-viewer/src/window.rs`'s module doc |
| MBFA binary compression | ✅ |
| SVG export | ✅ |
| True per-element paint order within one Run (vector/SDF/splat interleaving) | ❌ still type-batched, not real document order — a real, identified, deliberately deferred gap |
| Pattern fills | ❌ |
| clip-path / mask (beyond a Layer's own `clip: bool`, which only clips to the layer's own pixel footprint) | ❌ |
| Font embedding | ❌ |
