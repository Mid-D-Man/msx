# Layers, blend modes and effects

A `layer` is a container that draws its children into a separate buffer and then composites that buffer onto what is already on the canvas. Compositing is where a layer gets its own opacity, a blend mode, effects such as blur or a shadow, and a place in the paint order.

```dixscript
{ type = "layer", id = "glow", blend_mode = "screen", opacity = 0.8, z_index = 1,
  effects = [ { type = "blur", radius = 6 } ],
  elements = [
    { type = "circle", cx = 150, cy = 100, r = 50, style = { fill = #4a9eff } }
  ] }
```

## Fields

| Field | Meaning | Default |
|---|---|---|
| `elements` | The children, drawn into the layer's buffer | none |
| `blend_mode` | How the buffer combines with the canvas below | `normal` |
| `opacity` | Opacity of the whole buffer, 0 to 1 | 1 |
| `z_index` | Paint order among sibling layers | 0 |
| `clip` | Clip the children to the layer's own pixel footprint | false |
| `effects` | Effects applied to the buffer, in order | none |
| `id`, `transform` | As for every element | |

## Blend modes

`blend_mode` accepts a DixScript enum value or one of these strings. A string that is not in the list is read as `normal`.

| Value | Value | Value |
|---|---|---|
| `normal` | `multiply` | `screen` |
| `overlay` | `add` | `soft_light` |
| `hard_light` | `difference` | `exclusion` |
| `darken` | `lighten` | `subtract` |
| `divide` | | |

`subtract` and `divide` need a signed intermediate buffer, which CSS blending cannot express. The CPU and GPU renderers implement them. The SVG renderer maps them to `normal`. The SVG renderer writes `add` as the CSS mode `plus-lighter`.

## Effects

Each entry of `effects` is an object whose `type` selects the effect. Effects apply to the composited layer in the order listed, before the layer blends with the canvas.

| Type | Fields and defaults |
|---|---|
| `blur` | `radius` (4) |
| `drop_shadow` | `offset_x` (0), `offset_y` (0), `blur_radius` (4), `color` (black), `opacity` (0.5) |
| `inner_shadow` | `offset_x` (0), `offset_y` (0), `blur_radius` (4), `color` (black), `opacity` (0.5) |
| `outer_glow` | `color` (white), `blur_radius` (8), `spread` (0), `opacity` (0.75) |
| `inner_glow` | `color` (white), `blur_radius` (8), `opacity` (0.75) |

An unknown effect type stops the parse. The `std/effects.msx` file ships ready-made presets: `soft_blur`, `heavy_blur`, `soft_shadow`, `long_shadow`, `pressed_inset`, `neon_glow` and `highlight_rim`.

> **Limitation** The GPU renderer applies `blur` only. The other four effects run in the SVG renderer, as a generated filter chain, and in the CPU rasterizer. A GPU render of a layer with a drop shadow shows the layer without the shadow.

## Paint order and z_index

Elements paint in document order. Layers add one rule: among the layers that share a parent list, the order is sorted by `z_index`, lowest first. Elements that are not layers keep their slots. The layers are reassigned to the slots that layers occupied, so a rect written between two layers stays between the two layer positions.

Layers with equal `z_index` keep their document order. A nested layer sorts only against its own siblings, at its own nesting level, before the level that contains it is sorted.

Because `z_index` is a number that an animation track can change, two layers can trade places during playback. The `layer_z_index_swap` example sweeps one layer from 0 to 1 and the other from 1 to 0, and the box on top changes at the midpoint with no blending.

<!-- example: layer_z_index_swap | animated | nosource -->

```dixscript
animations::
  { target_id = "box_a", property = "z_index",
    keyframes = [ { time = 0.0, value = 0.0 }, { time = 1.6, value = 1.0 } ] }
  { target_id = "box_b", property = "z_index",
    keyframes = [ { time = 0.0, value = 1.0 }, { time = 1.6, value = 0.0 } ] }
```

## Layers and shaders

A shape inside a layer can take a shader fill, and the GPU renderer runs the shader in the layer's buffer. The `layer_shader_fill` example does this. See [Shaders](shaders.md).
