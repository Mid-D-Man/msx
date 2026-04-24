# MSX (MidStroke eXchange) Format Specification v1.0

## 1. Overview
MSX is a high-performance, two-layer vector image format:
1. Source layer — A DixScript (.msx) file defining the scene.
2. Binary layer — A compact binary encoding of the evaluated scene graph, optionally MBFA-compressed.

---

## 2. Source Format (DixScript)

### 2.1 Required Sections
| Section | Required | Purpose |
|---|---|---|
| @CONFIG | No | Version, metadata |
| @ENUMS | No | Named constants (BlendMode, LineCap, etc.) |
| @QUICKFUNCS | No | Parametric shape generators |
| @DATA | Yes | Scene definition |

### 2.2 @DATA Schema
@DATA(
  scene: { width = <float>, height = <float>, background = <color_string> }
  viewbox: { min_x = <float>, min_y = <float>, width = <float>, height = <float> }
  defs::
    { type = "linear_gradient", id = <string>, x1 = <float>, y1 = <float>, x2 = <float>, y2 = <float>,
      stops = [ { offset = <float>, color = <color_string>, opacity = <float> } … ] }
    { type = "radial_gradient", id = <string>, cx = <float>, cy = <float>, r = <float>, stops = [ … ] }
  elements::
    <element>…
)

### 2.3 Element Objects
- rect: { type = "rect", x = f64, y = f64, width = f64, height = f64, rx = f64?, ry = f64?, id = string?, transform = transform?, style = style }
- circle: { type = "circle", cx = f64, cy = f64, r = f64, id = string?, transform = transform?, style = style }
- ellipse: { type = "ellipse", cx = f64, cy = f64, rx = f64, ry = f64, id = string?, transform = transform?, style = style }
- line: { type = "line", x1 = f64, y1 = f64, x2 = f64, y2 = f64, id = string?, transform = transform?, style = style }
- polyline: { type = "polyline", points = [ [f64, f64] … ], id = string?, transform = transform?, style = style }
- polygon: { type = "polygon", points = [ [f64, f64] … ], id = string?, transform = transform?, style = style }
- path: { type = "path", d = string, id = string?, transform = transform?, style = style }
- text: { type = "text", x = f64, y = f64, content = string, id = string?, transform = transform?, style = style }
- group: { type = "group", elements = [ <element> … ], id = string?, transform = transform?, style = style? }
- use: { type = "use", href = string, x = f64?, y = f64?, transform = transform? }

### 2.4 Style Object
{
  fill              = paint,    // default "black"
  stroke            = paint,    // default "none"
  stroke_width      = f64,      // default 1.0
  opacity           = f64,      // 0.0..1.0, default 1.0
  fill_opacity      = f64?,
  stroke_opacity    = f64?,
  fill_rule         = "nonzero" | "evenodd",
  stroke_linecap    = "butt" | "round" | "square",
  stroke_linejoin   = "miter" | "round" | "bevel",
  stroke_miterlimit = f64?,
  stroke_dasharray  = [f64]?,
  stroke_dashoffset = f64?,
  font_size         = f64?,
  font_family       = string?,
  font_weight       = "normal" | "bold" | int?,
  text_anchor       = "start" | "middle" | "end",
  dominant_baseline = string?,
  visibility        = "visible" | "hidden",
  display           = "inline" | "none",
}

### 2.5 Transform Values
- String: "translate(tx, ty)", "scale(sx, sy)", "rotate(deg, cx, cy)", "matrix(a,b,c,d,e,f)", etc.
- Object: { type = "translate"|"scale"|"rotate"|"matrix", ...fields }

---

## 3. Binary Format

### 3.1 File Header (32 bytes)
| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 4 | magic | 0x4D 0x53 0x58 0x00 |
| 4 | 1 | version | 1 |
| 5 | 1 | compress | 0=none 1=mbfa |
| 6 | 1 | flags | bit0=viewbox bit1=metadata bit2=defs |
| 7 | 1 | reserved | zero |
| 8 | 4 | width | f32 LE |
| 12 | 4 | height | f32 LE |
| 16 | 4 | elem_count | u32 LE |
| 20 | 4 | str_pool_len | u32 LE |
| 24 | 4 | def_count | u32 LE |
| 28 | 4 | reserved | zeros |

### 3.2 Payload (Optionally MBFA-compressed)
- Background: [u8 r][u8 g][u8 b][u8 a]
- Viewbox: [f32 min_x][f32 min_y][f32 w][f32 h] (if flags.bit0)
- String pool: [u16 count] ( [u16 byte_len][utf8…] )*
- Def section: element* (def_count elements)
- Element stream: element* (elem_count elements)

### 3.3 Element Type Tags
0x00: Rect, 0x01: Circle, 0x02: Ellipse, 0x03: Line, 0x04: Polyline, 0x05: Polygon, 0x06: Path, 0x07: Text, 0x08: Group, 0x09: Use, 0x0A: LinearGradient, 0x0B: RadialGradient, 0xFF: End.

### 3.4 Style Block Encoding
[u8 present_flags]
bit0=fill, bit1=stroke, bit2=opacity, bit3=stroke_width, bit4=rule/caps/join, bit5=font, bit6=dash.
- Paint: [u8 type] (0=None, 1=RGBA, 2=GradRef, 3=PatRef)
- Transform: [u8 type] (0=None, 1=Matrix, 2=Translate, 3=Scale, 4=Rotate, 5=SkewX, 6=SkewY, 7=Multiple)

---

## 4. Compression & CLI
- MBFA: Processes the entire payload. Works via coordinate locality and opcode/style reuse.
- CLI: `msx <render|compile|info|validate|roundtrip> <input> [options]`
