# Source files and DixScript

An MSX source file is a DixScript file with a handful of sections that the MSX parser knows about. DixScript evaluates the whole file before MSX reads it, so function calls, enum values and interpolated strings are already plain values when the scene is built.

## File sections

| Section | Holds |
|---|---|
| `@CONFIG` | File metadata. Every example starts with `@CONFIG( version -> "1.0.0" )` |
| `@QUICKFUNCS` | Functions that return values or whole objects |
| `@ENUMS` | Named integer constants such as blend modes |
| `@IMPORTS` | Functions and enums loaded from another file |
| `@DATA` | The scene |

## The `@DATA` section

Inside `@DATA` the parser reads these keys.

| Key | Form | Meaning |
|---|---|---|
| `scene` | object | Canvas size and background, see [Canvas and viewbox](canvas.md) |
| `viewbox` | object | Optional visible region |
| `defs::` | array | Gradients, shaders and audio |
| `elements::` | array | The drawable tree, in paint order |
| `animations::` | array | Keyframe tracks, see [Keyframe animation](animation.md) |
| `duration` | number | Timeline length in seconds |
| `loop_mode` | string | `once`, `loop` or `ping_pong` |

`defs::`, `elements::` and `animations::` introduce an array. Each entry is an object, written one after another with no separating comma.

```dixscript
@DATA(
  scene = { width = 300, height = 200, background = #101820 }
  elements::
    { type = "circle", cx = 90,  cy = 100, r = 40, style = { fill = #4a9eff } }
    { type = "circle", cx = 210, cy = 100, r = 40, style = { fill = #f5a623 } }
)
```

An object is a list of `key = value` fields. Fields are separated by a comma or by whitespace. Comments start with `//`.

## QuickFuncs

A QuickFunc is a function declared in `@QUICKFUNCS` and called wherever a value or an array entry is expected. It is the main tool for removing repetition. The `parametric` example defines two functions and calls each one several times.

<!-- example: parametric -->

A QuickFunc declaration has the form `~name<return type>(arguments) { return value }`. Arguments are used by name inside the returned object, and the body can compute with them, as in `x = x + 50`. A call that returns an object can stand directly in an `elements::` list.

## Interpolated strings

A string written as `$"..."` evaluates the expressions inside braces and builds the string. It is the way to compute path data from parameters. This path, taken from `std/shapes.msx`, draws a check mark whose size follows `half`.

```dixscript
d = $"M {cx - half * 0.7} {cy} L {cx - half * 0.1} {cy + half * 0.7} L {cx + half * 0.8} {cy - half * 0.6}"
```

## Enums

Fields that take a closed set of values, such as `blend_mode`, `fill_rule` and `stroke_linecap`, accept either a DixScript enum value or a plain string. Both forms mean the same thing.

```dixscript
blend_mode<enum> = BlendMode.Multiply
blend_mode = "multiply"
```

The constants live in `std/enums.msx`. That file notes that importing enums from another file uses a different grammar from importing functions, and recommends copying the `@ENUMS` block into the scene file when a cross-file enum does not resolve. Plain strings work in every case.

> **Warning** A string that is not a recognized value does not raise an error. `stroke_linecap = "rounded"` is read as `butt`, and an unknown `loop_mode` is read as `once`. Check spelling against the value lists on each page.

## Colors

A color can be written as an unquoted hex literal such as `#e94560` or as a quoted string such as `"#e94560"`. The accepted forms are listed in [Style and paint](style-and-paint.md).

> **Warning** A color string that cannot be parsed becomes "no paint" without an error. A fill of `"#e9456"` draws nothing.

## Importing the standard library

The `std/` directory ships four files. Each is a normal DixScript file, so a function can also be copied into a scene.

| File | Provides |
|---|---|
| `std/colors.msx` | Twenty color functions: `white`, `black`, six slate shades, `crimson`, `violet`, `navy`, `azure`, `emerald`, `amber`, `lavender`, `success`, `warning`, `danger`, `info` and `primary` |
| `std/effects.msx` | Layer effect presets: `soft_blur`, `heavy_blur`, `soft_shadow`, `long_shadow`, `pressed_inset`, `neon_glow` and `highlight_rim` |
| `std/shapes.msx` | Components: `badge`, `card`, `icon_check`, `dot` and `divider` |
| `std/enums.msx` | The enum constants described above |

The header of each file documents its import form.

```dixscript
@IMPORTS( Colors from "std/colors.msx" )
// then
fill = Colors.crimson()
```

## Errors and validation

A missing required field or an unknown element `type` stops the parse with a message that names the path, for example `elements[2]: unknown element type 'rectangle'`. `msx validate` runs the parse and the schema check without producing any output, and its exit code reports the result.
