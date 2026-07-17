# MSX — MidStroke eXtension

Vector image format co-designed with **DixScript** and **MBFA**.

## Why MSX?

SVG is XML. Nobody writes XML by hand for complex graphics. MSX source files are
**DixScript** — the same format powering your project configs, but now driving
your vectors. QuickFuncs become parametric shape generators and reusable
component libraries. Zero repetition. Full type safety. Compile once to a compact
binary that MBFA crushes further.

## Workspace

MSX is a multi-crate Rust workspace, not a single library:

| Crate | Role |
|---|---|
| `core/msx-ast` | Element tree, paints, defs, transforms, animation data types |
| `core/msx-anim` | Keyframe timeline resolution (`resolve_at_time`) — TRS + opacity, every element type |
| `core/msx-parser` | DixScript → `Scene` AST |
| `core/msx-binary` | `Scene` ↔ compact binary `.msx`, optional MBFA compression |
| `render/msx-render-core` | Shared `Renderer`/`RenderTarget` traits |
| `render/msx-render-svg` | `Scene` → SVG export |
| `render/msx-render-cpu` | `Scene` → raster PNG, pure CPU (`tiny-skia`) |
| `render/msx-render-gpu` | `Scene` → raster PNG/GIF via `wgpu` — the only backend that actually executes `Def::Shader` WGSL; optional (`gpu` Cargo feature) |
| `primitives/msx-sdf` | Signed-distance-field shape tree (`Sdf` elements) |
| `primitives/msx-splat` | Gaussian-splat elements |
| `apps/msx-cli` | The `msx` command-line tool |
| `apps/msx-viewer` | Native window viewer (CPU renderer only — see Known Gaps) |

## Quick Start

```bash
# Requires ../mbfa as a sibling directory
cargo build --release

# Compile MSX source to binary
cargo run --release -- compile examples/basic_shapes.msx -o out.msx

# Evaluate MSX source (or decode binary) to SVG
cargo run --release -- render examples/basic_shapes.msx -o out.svg
cargo run --release -- render out.msx -o recovered.svg

# Rasterize to PNG (CPU — always available)
cargo run --release -- rasterize examples/basic_shapes.msx -o out.png

# Sample a keyframed scene's timeline to a looping GIF (CPU)
cargo run --release -- animate examples/sdf_splat_orbit.msx -o out.gif

# Roundtrip self-test (source → binary → SVG → compare)
cargo run --release -- roundtrip examples/basic_shapes.msx

# Parse + schema-validate only, no output
cargo run --release -- validate examples/basic_shapes.msx

# Show canvas/element/def stats for a file (source or binary)
cargo run --release -- info out.msx

# --- GPU rendering (real WGSL shader execution) needs the `gpu` feature ---
cargo build --release --features gpu

# Rasterize with real Def::Shader WGSL execution, single frame at t = <seconds>
cargo run --release --features gpu -- rasterize-gpu examples/shader_orb.msx -o out.png --time 1.5

# Sample BOTH the msx-anim keyframe clock and the shader's time uniform
# together into a GIF — see "Animation" below for why there are two clocks
cargo run --release --features gpu -- animate-gpu examples/shader_orb.msx -o out.gif --duration 4 --fps 24

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
          style = { fill = #fff, font_size = 12, text_anchor = "middle",
                    stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }
)

@DATA(
  // Canvas definition
  scene: { width = 600, height = 200, background = #f0f0f0 }

  // Every element is a plain DixScript object or a QuickFunc call
  elements::
    badge(40,  80, "primary", #007bff)
    badge(160, 80, "success", #28a745)
    badge(280, 80, "danger",  #dc3545)
    { type = "circle", cx = 450, cy = 100, r = 60,
      style = s(#533483, #7d3c98, 3) }
)
```

### Element types

`rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path`, `text`,
`group`, `use`, `layer` (isolated-buffer compositing — see "Layers" below),
`sdf` (signed-distance-field shapes, `primitives/msx-sdf`), and `splat`
(Gaussian-splat elements, `primitives/msx-splat`).

### Def types

Referenced by any element's `fill`/`stroke` via `"url(#id)"`:
`linear_gradient`, `radial_gradient`, `conic_gradient`, and `shader` — a
real WGSL fragment shader, executed for real on `msx-render-gpu`. A minimal
shader def:

```dixscript
defs::
  { type = "shader", id = "my_shader", source_ref = "shaders/my_shader.wgsl",
    entry_point = "fs_main", fallback_color = #7c5cff,
    uniforms = [ { name = "speed", type = "float", value = 1.0 },
                 { name = "resolution", type = "vec2", value = [300.0, 200.0] } ] }
elements::
  { type = "rect", x = 0, y = 0, width = 300, height = 200,
    style = { fill = "url(#my_shader)", stroke = "none", stroke_width = 0, opacity = 1.0 } }
```

`source_ref` is a WGSL fragment-shader-only file, resolved relative to the
`.msx` file's own directory. Its uniform struct must declare the `uniforms`
list above in the same order, followed by an auto-appended trailing
`time: f32` the renderer drives every frame — see
`render/msx-render-gpu/src/shader.rs`'s module doc for the exact
byte-layout contract (WGSL's real `vec2`/`vec3`/`vec4` alignment rules, not
a simplified approximation). `fallback_color` is what every renderer
*except* `msx-render-gpu` paints instead — see "Rendering Backends" below.

Every `.msx` file under `examples/` that uses a shader is a working
reference: `shader_placeholder.msx` (a simple animated plasma effect),
`shader_checkerboard.msx` (a deliberately trivial diagnostic pattern —
useful when debugging the pipeline itself, not decoration), and
`shader_orb.msx` (a raymarched, rim-lit sphere — the heaviest WGSL of the
three, loop/branch-heavy rather than flat per-pixel math).

## Rendering Backends

Three renderers, not one — they don't all support the same things:

| Backend | Crate | `Def::Shader` fills | Notes |
|---|---|---|---|
| CPU raster | `msx-render-cpu` | Flat `fallback_color` only | Always available, no feature flag |
| SVG export | `msx-render-svg` | Flat `fallback_color` only | `msx render` |
| GPU raster | `msx-render-gpu` | **Real WGSL execution** | Behind `--features gpu`; needs a GPU adapter (real or software/lavapipe) |

Within `msx-render-gpu` specifically, real shader execution doesn't reach
everywhere yet — the WGSL only runs where the geometry is either a
triangulated shape or has an explicit compositing path built for it:

| Element type | Flat color / gradient fill | Real shader fill |
|---|---|---|
| Vector shapes (rect, circle, path, …) | ✅ | ✅ fill only, not inside a `layer` |
| `sdf` | ✅ | ✅ fill only, not inside a `layer` — composited against the node's real antialiased silhouette (mask × shader-color), not just its bounding box |
| `splat` | ✅ (flat `color` field, no `Paint`/def reference at all) | ❌ — splats have no fill-reference concept yet, structurally |
| Anything inside a `layer` | ✅ | ❌ — `layer.rs` doesn't route to the shader pipeline yet, for any element type |

A shader fill that fails to resolve (missing `source_ref`, adapter
unavailable, etc.) always falls back to flat `fallback_color` rather than
vanishing or panicking — this holds for every element type above.
Malformed-but-*readable* WGSL is the one failure mode that isn't caught
gracefully yet (see `shader.rs`'s module doc).

## Animation

There are **two independent clocks**, and understanding which one moves
what is the whole story:

1. **Keyframe timeline** (`msx-anim`, `resolve_at_time`) — `animations::`
   blocks with `duration`/`loop_mode`, driving translate/scale/rotate/
   opacity. Works uniformly across every element type, `sdf` and `splat`
   included. `msx animate` samples this to a GIF via `msx-render-cpu`.
2. **Shader `time` uniform** — the free-running clock auto-appended to
   every shader def's uniform struct (see "Def types" above). Only moves
   on `msx-render-gpu`; `msx rasterize-gpu --time <seconds>` samples one
   frame of it.

`msx animate-gpu` is the only command that drives both at the same `t` per
frame, into one GIF — the right tool whenever a scene uses (or might use)
a shader def and keyframes together. `msx-viewer` doesn't drive either
clock continuously yet (see Known Gaps) — everything animated today is a
baked GIF, not live playback.

## Layers

`{ type = "layer", children = [...] }` composites its children into an
isolated offscreen buffer first (own opacity, own future blend mode),
rather than drawing straight into the shared scene buffer the way `group`
does. The tradeoff: content inside a `layer` doesn't get real shader
execution (see the table above) — that's `layer.rs`'s own render path, not
the one the top-level shader routing goes through.

## Binary Format (MSX)

Header (32 bytes):
```
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
```

Payload (optionally MBFA-compressed):
```
Background RGBA    4 bytes
Viewbox            16 bytes  (if flags bit 0)
String pool        [u16 count][u16 len + bytes]*
Def section        [element]* — gradient, pattern, AND shader defs
Element stream      [element]*  (recursive, terminated by 0xFF) — every
                    element type above, including sdf/splat/layer
```

Full tag values and per-type wire layouts are in `docs/format-spec.md` —
**that document is significantly further behind current reality than this
README was before this pass** (its element/tag tables and "v0.1 feature
scope" section predate `sdf`, `splat`, `shader` defs, `layer`, and
`msx-anim` entirely, and its CLI reference lists commands that don't
exist). Flagging rather than silently rewriting it — it's a bigger, more
structural pass than this README update was.

**Known gap:** the binary codec does not currently serialize
`animations`/`duration`/`loop_mode` at all — compiling an animated scene to
binary silently drops its keyframe tracks. Confirmed still open as of this
pass (`core/msx-binary/src/compiler.rs` has no reference to any of the
three).

## MBFA Co-design

Vector binary data has structure MBFA exploits:
- **Coordinate streams** — adjacent elements often share x/y proximity → LZ back-references span across shapes
- **Opcode streams** — repeated path command sequences (M L L Z over and over) → fold-1 matches, fold-2 pair-encodes repeated opcodes
- **Color data** — palettes tend to repeat; RGB bytes of similar colours are nearby values → delta-encoding before MBFA degrades entropy fast

## License

MIT
