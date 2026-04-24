# MSX — MidStroke eXchange

Vector image format co-designed with **DixScript** and **MBFA**.

## Why MSX?

SVG is XML. Nobody writes XML by hand for complex graphics. MSX source files are
**DixScript** — the same format powering your project configs, but now driving
your vectors. QuickFuncs become parametric shape generators and reusable
component libraries. Zero repetition. Full type safety. Compile once to a compact
binary that MBFA crushes further.

## Stack

| Layer | Technology | Role |
|---|---|---|
| Source format | DixScript `.msx` | Human-readable; QuickFuncs = components |
| Scene graph | Rust AST | Element tree after DixScript evaluation |
| Binary encoding | Typed streams | Coordinate, opcode, color, string pools |
| Compression | MBFA multi-fold LZ | Applied to binary payload |
| Export | SVG renderer | Pixel-perfect SVG for browser/tool compat |

## Quick Start

```bash
# Requires ../mbfa as a sibling directory
cargo build --release

# Render MSX source to SVG
cargo run --release -- render examples/basic_shapes.msx -o out.svg

# Compile MSX source to binary
cargo run --release -- compile examples/basic_shapes.msx -o out.msx

# Decompile binary back to SVG
cargo run --release -- render out.msx -o recovered.svg

# Roundtrip self-test (source → binary → SVG → compare)
cargo run --release -- roundtrip examples/basic_shapes.msx

# Show binary file info
cargo run --release -- info out.msx

# Run all tests
cargo test -- --nocapture

# Run benchmarks
cargo bench --bench compare
```

## MSX Source Format

An `.msx` file is a valid DixScript file with a vector-graphics schema.

```dixscript
@CONFIG(
  version -> "1.0.0"
)

@ENUMS(
  FillRule { NonZero = 0, EvenOdd = 1 }
)

@QUICKFUNCS(
  // Reusable style shorthand
  ~s<object>(fill, stroke, sw) {
    return { fill = fill, stroke = stroke, stroke_width = sw, opacity = 1.0 }
  }

  // Parametric badge component — DixScript QuickFuncs compose freely
  ~badge<object>(x, y, label, color) {
    return {
      type     = "group"
      elements = [
        { type = "rect", x = x, y = y, width = 90, height = 30, rx = 15,
          style = { fill = color, stroke = "none", stroke_width = 0, opacity = 1.0 } }
        { type = "text", x = x + 45, y = y + 20, content = label,
          style = { fill = "#fff", font_size = 12, text_anchor = "middle",
                    stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }
)

@DATA(
  // Canvas definition
  scene: { width = 600, height = 200, background = "#f0f0f0" }

  // Every element is a plain DixScript object or a QuickFunc call
  elements::
    badge(40,  80, "primary", "#007bff")
    badge(160, 80, "success", "#28a745")
    badge(280, 80, "danger",  "#dc3545")
    { type = "circle", cx = 450, cy = 100, r = 60,
      style = s("#533483", "#7d3c98", 3) }
)
```

## Binary Format (MSX)
Header (32 bytes):
[0..4]   magic:      0x4D 0x53 0x58 0x00  ("MSX\0")
[4]      version:    u8 = 1
[5]      compress:   u8  (0=none  1=mbfa)
[6]      flags:      u8  (bit0=has_viewbox  bit1=has_metadata  bit2=has_defs)
[7]      reserved:   u8
[8..12]  width:      f32 LE
[12..16] height:     f32 LE
[16..20] elem_count: u32 LE
[20..24] str_pool_len: u32 LE
[24..28] def_count:  u32 LE
[28..32] reserved:   [u8; 4]

Payload (optionally MBFA-compressed):
Background RGBA    4 bytes
Viewbox            16 bytes  (if flags bit 0)
String pool        [u16 count][u16 len + bytes]*
Def section        [element]* for gradient/pattern defs
Element stream     [element]*  (recursive, terminated by 0xFF)

## MBFA Co-design

Vector binary data has structure MBFA exploits:
- **Coordinate streams** — adjacent elements often share x/y proximity → LZ back-references span across shapes
- **Opcode streams** — repeated path command sequences (M L L Z over and over) → fold-1 matches, fold-2 pair-encodes repeated opcodes
- **Color data** — palettes tend to repeat; RGB bytes of similar colours are nearby values → delta-encoding before MBFA degrades entropy fast

## License

MIT
