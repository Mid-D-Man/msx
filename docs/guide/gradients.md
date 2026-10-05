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

Gradient coordinates are fractions of the bounding box of the element that uses the gradient, not canvas pixels. A linear gradient from `x1 = 0` to `x2 = 1` runs from the left edge of the element to its right edge, whatever the element's position or size. Reusing one gradient on shapes of different sizes therefore needs no change.

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

The defaults describe a circle that touches the edges of the bounding box.

## Conic gradient

| Field | Meaning | Default |
|---|---|---|
| `id` | Name used by `url(#id)` | required |
| `cx`, `cy` | Center | 0.5, 0.5 |
| `angle` | Start angle in degrees | 0 |
| `stops` | The color stops | none |

A conic gradient sweeps the stops around the center.

> **Limitation** The SVG format has no conic gradient, so the SVG renderer writes only a comment, and a fill that references one is not drawn. The CPU and GPU rasterizers paint it as a flat color. Conic gradients are stored and round-trip through the binary format.

## Stops

| Field | Meaning | Default |
|---|---|---|
| `offset` | Position along the gradient, 0 to 1 | 0 |
| `color` | Stop color | black |
| `opacity` | Stop opacity, folded into the color's alpha | the color's own alpha |

The `glow` gradient in the example fades from opaque to fully transparent by setting `opacity = 0.0` on its last stop.

## Where it renders

The SVG renderer writes real linear and radial gradients. The CPU and GPU rasterizers paint a gradient reference as one flat color, the average of its stops, and they do not interpolate across the shape. To compare, render the same file with `msx render` and `msx rasterize`.

Gradients also work as the `fill` of a splat. A splat reduces the gradient to its average color first. See [Gaussian splats](splats.md).
