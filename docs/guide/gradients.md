# Gradients

Gradients are defs. A gradient is declared once in `defs::` with an `id` and used by any number of fills and strokes through `url(#id)`.

<!-- example: gradients -->

## Referencing a gradient

```dixscript
defs::
  { type = "linear_gradient", id = "sunset",
    x1 = 0.0, y1 = 0.0, x2 = 1.0, y2 = 0.0,
    stops = [ { offset = 0.0, color = #f7971e }, { offset = 1.0, color = #ffd200 } ] }
elements::
  { type = "rect", x = 30, y = 30, width = 250, height = 80,
    style = { fill = "url(#sunset)" } }
```

The reference is a quoted string. The `id` of every def must be present, and a reference to an id that does not exist draws nothing.

## Coordinates

By default gradient coordinates are fractions of the bounding box of the element that uses the gradient, not canvas pixels. A linear gradient from `x1 = 0` to `x2 = 1` runs from the left edge of the element to its right edge, whatever the element's position or size. Reusing one gradient on shapes of different sizes therefore needs no change.

A shape whose bounding box has no width or no height has nothing to map the gradient onto in these units. A perfectly horizontal line is the usual case. A fill or stroke that references a gradient then draws nothing, which is what SVG does. The bounding box covers the geometry without its stroke, and for text it is the text box.

## Units, spread and transform

Linear and radial gradients take three more keys, which work like SVG's `gradientUnits`, `spreadMethod` and `gradientTransform`. A gradient that sets any of them is written as binary version 3, and a gradient that sets none is written as before (see [Binary format](binary-format.md#versions)).

| Field | Meaning | Default |
|---|---|---|
| `units` | `"object_bounding_box"` or `"user_space"` | `"object_bounding_box"` |
| `spread` | What lies beyond the end points: `"pad"`, `"reflect"` or `"repeat"` | `"pad"` |
| `transform` | A transform string such as `"rotate(45) scale(2 1)"`, applied inside the units | none |

With `user_space` the coordinates are in the local coordinates of the element that uses the gradient, so the gradient stays put when the element moves or changes size, and it paints a shape with no box area, such as a horizontal line. The transform acts inside the units: with box units it turns, stretches or moves the gradient inside the box, so a radial gradient under `scale(2 1)` becomes an ellipse. A transform that cannot be undone (a zero scale, or a value that is not a number) counts as no transform, as in resvg.

```dixscript
{ type = "linear_gradient", id = "stripes", x1 = 0, y1 = 0, x2 = 20, y2 = 0,
  units = "user_space", spread = "repeat", transform = "rotate(30)",
  stops = [ { offset = 0, color = #ff0000, opacity = 1 }, { offset = 1, color = #0000ff, opacity = 1 } ] }
```

## Linear gradient

| Field | Meaning | Default |
|---|---|---|
| `id` | Name used by `url(#id)` | required |
| `x1`, `y1` | Start point | 0, 0 |
| `x2`, `y2` | End point | 1, 0 |
| `stops` | The color stops | none |

The default runs left to right. A vertical gradient uses `x2 = 0.0` and `y2 = 1.0`.

## Radial gradient

| Field | Meaning | Default |
|---|---|---|
| `id` | Name used by `url(#id)` | required |
| `cx`, `cy` | Center | 0.5, 0.5 |
| `r` | Radius | 0.5 |
| `fx`, `fy` | Focal point | the center |
| `stops` | The color stops | none |

The defaults describe a circle that touches the edges of the bounding box. The focal point is where the gradient starts, and the circle is where it ends. A focal point outside the circle makes a cone: the gradient covers the area between the two tangent lines from the focal point and the area beyond the circle inside them, and nothing is painted outside it, which is how SVG 2 and the canvas define it. A radius of 0 paints the last stop color.

`fr` is the radius of the focal circle, 0 by default, which makes the focal point a point. With `fr` above 0 the gradient starts on a ring: a point is colored by the largest `t` for which the circle that runs from the focal circle (`t = 0`) to the main circle (`t = 1`) passes through it and has a positive radius. In the usual concentric case that means the first stop color inside `fr`, the gradient between `fr` and `r`, and the last stop color outside `r`. An `fr` larger than `r` runs the gradient inward. A negative `fr`, or one that is not a number, counts as 0. An `fr` above 0 needs binary version 3.

## Conic gradient

| Field | Meaning | Default |
|---|---|---|
| `id` | Name used by `url(#id)` | required |
| `cx`, `cy` | Center | 0.5, 0.5 |
| `angle` | Start angle in degrees | 0 |
| `stops` | The color stops | none |

A conic gradient sweeps the stops around the center. The sweep starts at the top and runs clockwise, like CSS `conic-gradient`. `angle` turns the start clockwise from the top, so `angle = 90` starts at the right edge. The angle is measured in the shape's own space, so a wide or tall bounding box does not stretch the sweep. Positions before the first stop take its color and positions after the last stop take that color.

> **Limitation** The SVG format has no conic gradient, so the SVG renderer writes only a comment, and a fill that references one is not drawn. The CPU and GPU renderers draw it. Conic gradients are stored and round-trip through the binary format.

## Stops

| Field | Meaning | Default |
|---|---|---|
| `offset` | Position along the gradient, 0 to 1 | 0 |
| `color` | Stop color | black |
| `opacity` | Stop opacity, folded into the color's alpha | the color's own alpha |

The `glow` gradient in the example fades from opaque to fully transparent by setting `opacity = 0.0` on its last stop.

## Where it renders

All three renderers interpolate linear and radial gradients, on fills and on strokes. The CPU and GPU renderers also draw conic gradients. The GPU renderer holds at most 32 stops per gradient and resamples a longer list to 32 evenly spaced stops, which softens any hard stop past that limit.

Stop colors keep their own alpha, and the `opacity` of the element multiplies into it. To compare renderers, render the same file with `msx render` and `msx rasterize`.

<!-- example: gradient_demo -->

Gradients also work as the `fill` of a splat. A splat reduces the gradient to its average color first, and so does an `sdf` node. See [Gaussian splats](splats.md).
