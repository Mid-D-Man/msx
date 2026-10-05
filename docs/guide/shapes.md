# Basic shapes

Four elements draw the common geometric shapes: `rect`, `circle`, `ellipse` and `line`. Each takes `id`, `transform` and `style` in addition to the fields below.

<!-- example: basic_shapes -->

## Rectangle

```dixscript
{ type = "rect", x = 20, y = 20, width = 160, height = 120, rx = 12,
  style = { fill = #e94560 } }
```

| Field | Meaning | Required |
|---|---|---|
| `x`, `y` | Top left corner | yes |
| `width`, `height` | Size | yes |
| `rx` | Horizontal corner radius | no |
| `ry` | Vertical corner radius. Defaults to `rx` | no |

A corner radius larger than half the width or height is clamped to half. A rect with only `rx` set has circular corners.

## Circle

```dixscript
{ type = "circle", cx = 300, cy = 300, r = 60,
  style = { fill = #0f3460, stroke = #4a9eff, stroke_width = 3 } }
```

`cx` and `cy` give the center and `r` the radius. All three are required.

## Ellipse

```dixscript
{ type = "ellipse", cx = 200, cy = 100, rx = 80, ry = 40,
  style = { fill = #22c55e } }
```

`rx` and `ry` are the horizontal and vertical radii. Both are required.

## Line

```dixscript
{ type = "line", x1 = 20, y1 = 20, x2 = 180, y2 = 120,
  style = { stroke = #ffffff, stroke_width = 2 } }
```

> **Warning** A line has no interior, and the default stroke is `none`. A `line` without a `stroke` in its style draws nothing.

## Anchoring and transforms

All four shapes are positioned by their own coordinates. A [transform](transforms.md) moves the shape afterward, so `rotate` with no pivot turns a shape about the canvas origin. To rotate a shape about its own center, pass the center as the pivot, for example `rotate(45, 300, 300)` for the circle above.
