# Canvas and viewbox

The `scene` block sets the size of the drawing and the color behind it. The optional `viewbox` block selects which region of the drawing space is visible.

## The scene block

```dixscript
scene = { width = 600, height = 400, background = #1a1a2e }
```

| Field | Type | Required | Default |
|---|---|---|---|
| `width` | number | yes | |
| `height` | number | yes | |
| `background` | color | no | white |

Width and height are in pixels for raster output and in user units for SVG. A background of `"none"` is a fully transparent color. The `mid-qr-k` example uses it so that the QR code carries no backdrop.

## The viewbox block

```dixscript
scene   = { width = 400, height = 300, background = #ffffff }
viewbox = { min_x = 100, min_y = 50, width = 200, height = 150 }
```

| Field | Type | Default |
|---|---|---|
| `min_x` | number | 0 |
| `min_y` | number | 0 |
| `width` | number | the canvas width |
| `height` | number | the canvas height |

The example above shows the region from (100, 50) to (300, 200) stretched over the 400 by 300 canvas, a 2x zoom on that region.

> **Limitation** Only the SVG renderer applies the viewbox, by writing it as the `viewBox` attribute. The CPU and GPU rasterizers size their output from `width` and `height` and do not read the viewbox, so a scene whose viewbox differs from its canvas looks different in raster output.

## Coordinates

The origin is the top left corner. The x axis points right and the y axis points down. Elements are positioned in this space, and [transforms](transforms.md) move them within it.
