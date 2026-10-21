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
| `clip_path` | A list of shapes. Only the area they cover stays visible | none |
| `mask` | A list of elements read as a mask over the layer | none |
| `mask_type` | How `mask` is read, `luminance` or `alpha` | `luminance` |
| `effects` | Effects applied to the buffer, in order | none |
| `id`, `transform` | As for every element | |

## Clip paths

`clip_path` is a list of shapes whose union is the visible area of the layer. Leaving it out clips nothing. An empty list clips everything away, as an empty SVG `clipPath` does.

```
{ type = "layer", clip_path = [
    { type = "circle", cx = 50, cy = 50, r = 40 }
  ],
  elements = [
    { type = "rect", x = 0, y = 0, width = 100, height = 100, style = { fill = #f2545b } }
  ] }
```

The shapes live in the coordinate space of the layer, so the layer `transform` moves them together with the children. Only geometry and `fill_rule` count: fill, stroke and opacity are ignored, and a `line` has no area. Rectangles, circles, ellipses, polylines, polygons, paths and text are used. The clip is applied after the effects and before the layer blends, so a blurred edge is cut by the clip. Two clips that must both apply are written as one layer inside another.

The SVG renderer writes a `clipPath` element. The CPU renderer builds an antialiased coverage mask. The GPU renderer draws the shapes into a mask texture and multiplies the layer with it, and its edges are not antialiased, as for every GPU shape.

## Masks

`mask` is a list of elements that are drawn normally and then read as a mask over the layer. With `mask_type = "luminance"` the layer keeps the brightness of the mask at each point, scaled by the mask alpha, so white and opaque keeps it, black or clear hides it. With `"alpha"` only the mask opacity counts and the color does not. The elements may use any paint, a gradient, an image, group opacity or a layer of their own. Leaving `mask` out masks nothing, and an empty list hides the whole layer.

```
{ type = "layer", mask_type = "luminance", mask = [
    { type = "rect", x = 0, y = 0, width = 100, height = 100, style = { fill = "url(#fade)" } }
  ],
  elements = [
    { type = "rect", x = 0, y = 0, width = 100, height = 100, style = { fill = #4a9eff } }
  ] }
```

The mask is drawn in the coordinate space of the layer, so the layer `transform` moves it with the children. Luminance uses the coefficients 0.2125, 0.7154 and 0.0721 on the stored color values, the same as resvg. It is applied together with `clip_path`, after the effects. A mask that needs a limited region keeps that region as a `clip_path` on a layer around its own elements, and a mask of a mask is a layer with its own `mask` inside the mask list.

The SVG renderer writes a `mask` element with a region far larger than the canvas. The CPU renderer draws the elements into a buffer and scales the layer by it. The GPU renderer draws them into a texture and multiplies the layer with its luminance or alpha.

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
