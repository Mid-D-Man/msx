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
| `divide` | `color_dodge` | `color_burn` |
| `hue` | `saturation` | `color` |
| `luminosity` | `plus_lighter` | |

`subtract` and `divide` need a signed intermediate buffer, which CSS blending cannot express. The CPU and GPU renderers implement them. The SVG renderer maps them to `normal`.

`hue`, `saturation`, `color` and `luminosity` work on all three color channels together and follow the W3C Compositing and Blending formulas. `color_dodge` and `color_burn` are per channel and follow the same specification.

`plus_lighter` is the CSS `plus-lighter` operator. It adds premultiplied color and alpha and clamps the sum, so it differs from `add` wherever alpha is below one. The SVG renderer writes both `add` and `plus_lighter` as the CSS mode `plus-lighter`.

## Effects

Each entry of `effects` is an object whose `type` selects the effect. Effects apply to the composited layer in the order listed, before the layer blends with the canvas.

| Type | Fields and defaults |
|---|---|
| `blur` | `radius` (4) |
| `drop_shadow` | `offset_x` (0), `offset_y` (0), `blur_radius` (4), `color` (black), `opacity` (0.5) |
| `inner_shadow` | `offset_x` (0), `offset_y` (0), `blur_radius` (4), `color` (black), `opacity` (0.5) |
| `outer_glow` | `color` (white), `blur_radius` (8), `spread` (0), `opacity` (0.75) |
| `inner_glow` | `color` (white), `blur_radius` (8), `opacity` (0.75) |
| `blur_xy` | `rx` (4), `ry` (4). A standard deviation for each axis, and a zero on one axis blurs only the other. Wire version 3 |
| `color_matrix` | `values` (20 numbers, required), `linear` (false). Wire version 3 |

A `color_matrix` is the 4x5 matrix of `feColorMatrix`, row major. The four rows give the new red, green, blue and alpha, and the five columns weigh the old red, green, blue, alpha and a constant, so the new red is `v[0] r + v[1] g + v[2] b + v[3] a + v[4]`. It works on straight channels in 0 to 1, and the results are held to 0 to 1. With `linear = true` the color goes through linear light for the matrix and back, which is what `feColorMatrix` does by default. The CSS color functions use sRGB, so `linear` stays false for them. Grayscale, saturation, hue rotation, sepia, invert, brightness, contrast and opacity changes are all matrices. The `blur_xy` radii follow the layer transform axis by axis when the transform keeps the axes (scale, flip, quarter turn). For any other rotation both amounts use the mean scale of the transform.

```
{ type = "color_matrix", linear = false, values = [
    0.2126, 0.7152, 0.0722, 0, 0,
    0.2126, 0.7152, 0.0722, 0, 0,
    0.2126, 0.7152, 0.0722, 0, 0,
    0,      0,      0,      1, 0 ] }
```

### Filter graph

A `graph` effect is the form of an SVG `<filter>` that a chain of effects cannot say: named results, two inputs, a flood, a composite, a merge, a blend, a component transfer and regions of their own. It holds a list of nodes, and the result of the last node replaces the layer. Wire version 3. The CPU and SVG renderers draw it, the GPU renderer does not yet.

```
{ type = "graph", nodes = [
    { op = "flood", color = #3ea6ffff },
    { op = "composite", input = "0", input2 = "alpha", operator = "in" } ] }
```

A node has an `op`, its inputs, and optionally a `region = [x, y, width, height]` and `linear = true`. An input is `"source"` (the layer pixels), `"alpha"` (the layer alpha with the color set to black) or the index of an earlier node as text. A node that has no `input` reads the node before it, or the source for the first node. `input2` is the second input of a `composite` or `blend` and defaults to the source.

| `op` | Fields | What it makes |
|---|---|---|
| `flood` | `color` | The color over the whole region. The alpha of the color is the flood opacity |
| `composite` | `input`, `input2`, `operator`, `k` | `over` (default), `in`, `out`, `atop`, `xor`, or `arithmetic` with `k = [k1, k2, k3, k4]` for `k1 a b + k2 a + k3 b + k4` on premultiplied channels |
| `merge` | `inputs` | The inputs painted over each other, the first at the bottom |
| `blend` | `input`, `input2`, `mode` | `input` drawn over `input2` with a blend mode, from the same list as a layer |
| `offset` | `input`, `dx`, `dy` | The input moved by whole pixels. A fraction is cut toward zero |
| `blur` | `input`, `rx`, `ry` | A Gaussian blur with a deviation per axis |
| `color_matrix` | `input`, `values` | The 4x5 matrix of `color_matrix` above |
| `morphology` | `input`, `operator`, `rx`, `ry` | `erode` (default) or `dilate`. Each pixel takes the smallest or largest value in a window of `2 * ceil(r)` pixels per axis that leans one pixel back. A radius of zero leaves the axis as it is |
| `tile` | `input` | The region of `input` repeated over the whole layer. An input without a region is the whole layer |
| `convolve_matrix` | `input`, `columns`, `rows`, `kernel`, `divisor`, `bias`, `target_x`, `target_y`, `edge`, `preserve_alpha` | A kernel of `columns * rows` numbers (at most 1024), row major. `columns` is 3 and `rows` is `columns` unless given, `divisor` is 1, `bias` 0, the target the middle cell, `edge` is `duplicate`, `wrap` or `none` and `preserve_alpha` is false |
| `component_transfer` | `input`, `r`, `g`, `b`, `a` | One function per channel, left out for no change. A function is `{ kind = "table", values = [...] }`, `"discrete"`, `{ kind = "linear", slope = 1, intercept = 0 }` or `{ kind = "gamma", amplitude = 1, exponent = 1, offset = 0 }` |

`linear = true` on a node makes it work on linear light, as `color-interpolation-filters="linearRGB"` does, which is the default of SVG filters. The node converts what it reads to that space, and the result goes back to sRGB at the end. A flood and an offset keep the space of what they hold. A `region` limits where the node paints and everything outside it is transparent. The region is in the units of the layer. When the layer transform keeps the axes it is exact, and under any other transform it becomes the box that holds the transformed rectangle. The graph has at most 256 nodes, and a node can read only nodes before it.

The radius of a blur is its standard deviation, the same number as `stdDeviation` in SVG. Radii, spreads and offsets are in the coordinate system of the layer, so they follow its transform: a layer scaled by 2 blurs twice as far, and a layer rotated by 90 degrees turns the offset of its shadow. The CPU renderer blurs with the SVG definition (three box blurs from a standard deviation of 2, a true Gaussian kernel below that) and treats pixels outside the canvas as transparent.

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
