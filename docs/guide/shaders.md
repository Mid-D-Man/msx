# Shaders

A shader def lets a fill run a WGSL fragment shader. It is the way to draw procedural patterns, noise, raymarched forms and anything else that is easier to compute than to describe.

<!-- example: shader_orb | nosource -->

## The shader def

```dixscript
defs::
  { type = "shader", id = "orb_1",
    source_ref = "shaders/orb_raymarch.wgsl",
    entry_point = "fs_main",
    fallback_color = #7c5cff,
    uniforms = [
      { name = "speed",      type = "float", value = 1.0 },
      { name = "resolution", type = "vec2",  value = [800.0, 600.0] }
    ] }
```

| Field | Meaning | Default |
|---|---|---|
| `id` | Name used by `url(#id)` | required |
| `source_ref` | Path to a `.wgsl` file, relative to the `.msx` file | required |
| `entry_point` | Name of the fragment function | `fs_main` |
| `fallback_color` | Color painted by every renderer that cannot run WGSL | required |
| `uniforms` | Values passed to the shader | none |

A shape uses the shader through its fill, as it would a gradient.

```dixscript
{ type = "rect", x = 40, y = 40, width = 320, height = 200,
  style = { fill = "url(#orb_1)" } }
```

## Uniforms

Each uniform has a `name`, a `type` and a `value`.

| Type | Value |
|---|---|
| `float` | A number |
| `vec2` | An array of exactly two numbers |
| `vec3` | An array of exactly three numbers |
| `vec4` | An array of exactly four numbers |

A uniform without a `type`, with an unknown type, or with the wrong number of components stops the parse.

## Writing the WGSL file

The file holds a fragment stage only. The renderer supplies the vertex stage. The file declares one uniform block and an entry point function that receives at least `@builtin(position)`.

```wgsl
struct Uniforms {
    speed:      f32,
    resolution: vec2<f32>,
    time:       f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / u.resolution;
    return vec4<f32>(uv, 0.5 + 0.5 * sin(u.time * u.speed), 1.0);
}
```

The renderer does not read the WGSL, so the layout of `Uniforms` is a convention that the author follows:

1. One field per entry of the def's `uniforms`, in the declared order, using the matching WGSL type: `f32`, `vec2<f32>`, `vec3<f32>` or `vec4<f32>`.
2. One final field, `time: f32`, which the renderer adds on its own. It is not listed in the def.

The renderer packs the values with the standard WGSL alignment rules, so the struct written in the file lines up without manual padding.

### The time uniform

`time` is a free-running clock in seconds, the same convention as Shadertoy. `msx rasterize-gpu --time 1.5` renders one frame at that time. `msx animate-gpu` renders a sequence of frames across a duration and writes a GIF, advancing both the shader clock and any keyframe animation together.

## Fallback and limits

A renderer that cannot run WGSL paints the shape with `fallback_color`. That covers the SVG renderer, the CPU rasterizer, and the GPU renderer when the `source_ref` file cannot be read. A missing file is reported on the standard error stream and the render continues. `msx compile` is stricter and stops when a `source_ref` does not resolve to a file, so a mistyped path surfaces at build time.

> **Warning** A WGSL file that exists but contains errors is not handled gracefully. The GPU stack reports a compile error through its own error callback. Check a new shader on its own before relying on it in a scene.

> **Limitation** A shader fills shapes only. A shader named in a `stroke` falls back to `fallback_color`. A shape's `opacity` is not multiplied into the shader output, so the shader controls its own alpha. The shader runs on the GPU renderer only, and the CPU and SVG output show the flat fallback color.

Shaders also fill SDF nodes and splats on the GPU, and shapes inside a layer. The `sdf_shader_fill`, `splat_shader_fill` and `layer_shader_fill` examples show each case.
