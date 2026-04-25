# MSX — MidStroke eXchange
## Format Specification v0.1.0

---

## Overview

MSX is a two-layer vector image format:

1. **Source layer** — A DixScript `.msx` file. The human-readable authoring surface.  
2. **Binary layer** — A compact typed-stream encoding of the evaluated scene graph, optionally MBFA-compressed.

The source layer is optional at runtime. A compiled binary `.msx` file is entirely self-contained and renders to SVG without the original source.

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
| 4 | 1 | version | `u8` | `1` |
| 5 | 1 | compress | `u8` | `0`=none `1`=mbfa |
| 6 | 1 | flags | `u8` | bit0=has_viewbox bit1=has_metadata bit2=has_defs |
| 7 | 1 | reserved | `u8` | zero |
| 8 | 4 | width | `f32 LE` | canvas width in user units |
| 12 | 4 | height | `f32 LE` | canvas height in user units |
| 16 | 4 | elem_count | `u32 LE` | top-level element count |
| 20 | 4 | str_pool_len | `u32 LE` | string pool byte length |
| 24 | 4 | def_count | `u32 LE` | gradient / pattern def count |
| 28 | 4 | reserved | `[u8; 4]` | zeros |

### Payload Layout (after header; optionally MBFA-compressed as a unit)

```
Background RGBA     4 bytes      [u8 r][u8 g][u8 b][u8 a]
Viewbox             16 bytes     [f32 min_x][f32 min_y][f32 w][f32 h]   (only if flags.bit0)
String pool         variable     [u16 count] ([u16 byte_len][utf8 bytes])*
Def section         variable     def_count × element
Element stream      variable     elem_count × element
```

### Element Type Tags

| Tag | Element |
|---|---|
| `0x00` | Rect |
| `0x01` | Circle |
| `0x02` | Ellipse |
| `0x03` | Line |
| `0x04` | Polyline |
| `0x05` | Polygon |
| `0x06` | Path |
| `0x07` | Text |
| `0x08` | Group |
| `0x09` | Use |
| `0x0A` | LinearGradient (def only) |
| `0x0B` | RadialGradient (def only) |
| `0xFF` | End sentinel |

### Element Wire Format

```
[u8  tag]
[u8  id_flags]        bit0 = has_id   bit1 = has_transform
[u16 id_str_idx]      (only if id_flags.bit0)
[transform block]     (only if id_flags.bit1)
[geometry fields]     (tag-specific — see below)
[style block]
```

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

#### Gradient stop

```
[f32 offset]      0.0..1.0
[u8  r g b a]
```

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

```
msx render    <source.msx>           Evaluate DixScript → SVG
             [-o out.svg]
             [--no-compress]         Skip MBFA when compiling

msx compile   <source.msx>           DixScript → binary .msx
             [-o out.msx]
             [--no-compress]

msx render    <binary.msx>           Binary → SVG (no source needed)
             [-o out.svg]

msx info      <file.msx>             Print header + scene stats
msx validate  <source.msx>           Parse + type-check only; exit code
msx roundtrip <source.msx>           source → binary → SVG; verify no panic
msx bench     <file.msx>             Decode + render timing (10 runs)
```

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
- A compact binary interchange format for tool-to-tool transfer
- A compression target for MBFA on coordinate + opcode streams

**MSX is not:**
- A raster image format (that is MPX)
- A full SVG feature replacement in v0.1 — animations, filters, masks, and clip-paths are post-v1
- A browser-native format — the output of the renderer is standard SVG

**v0.1 feature scope:**

| Feature | v0.1 | Post-v1 |
|---|---|---|
| rect, circle, ellipse, line | ✅ | |
| polygon, polyline | ✅ | |
| path (all SVG commands) | ✅ | |
| text | ✅ | |
| group + transform | ✅ | |
| linear + radial gradient | ✅ | |
| use / def referencing | ✅ | |
| MBFA binary compression | ✅ | |
| SVG export | ✅ | |
| Pattern fills | | ✅ |
| clip-path / mask | | ✅ |
| CSS animations | | ✅ |
| Filter effects | | ✅ |
| Font embedding | | ✅ |
