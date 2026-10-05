# How a scene fits together

Every MSX scene has the same four parts. Knowing what each part holds, and how ids connect them, makes the other pages easier to read.

## The four parts

| Part | Key in `@DATA` | Holds |
|---|---|---|
| Canvas | `scene`, optional `viewbox` | Size, background color and the visible region |
| Defs | `defs::` | Gradients, shaders and audio, each with an `id` |
| Elements | `elements::` | The drawable tree, in paint order |
| Animation | `animations::`, `duration`, `loop_mode` | Keyframe tracks over element ids |

Only the canvas and the element list are needed for a useful file. A scene with no `elements` is valid and draws only its background.

## A minimal scene

```dixscript
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 200, height = 100, background = #0d1117 }
  defs::
    { type = "linear_gradient", id = "fade",
      x1 = 0.0, y1 = 0.0, x2 = 1.0, y2 = 0.0,
      stops = [ { offset = 0.0, color = #4a9eff }, { offset = 1.0, color = #a78bfa } ] }
  elements::
    { type = "rect", id = "bar", x = 20, y = 30, width = 160, height = 40,
      style = { fill = "url(#fade)", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
```

The rect paints itself with the gradient by naming it as `url(#fade)`. The rect has its own id, `bar`, so a keyframe track or a `use` element could refer to it.

## Paint order

Elements paint in document order, so later elements cover earlier ones. Two features change that on purpose: a [layer](layers.md) composites its children in an isolated buffer, and layers can be reordered among their siblings with `z_index`.

## Ids connect the parts

An `id` is how one part of a scene refers to another.

| Reference | Written as | Points at |
|---|---|---|
| Paint | `fill = "url(#id)"` | A gradient or shader def |
| Reuse | `href = "#id"` on a `use` element | Any element with that id |
| Animation | `target_id = "id"` on a track | Any element with that id |
| Media | `extract-media --id id` | An audio def |

Ids are plain strings. Two defs should not share an id, and a reference to an id that does not exist draws nothing.

## Coordinates

The canvas has its origin at the top left. The x axis points right and the y axis points down, as in SVG. Lengths are in user units, which equal pixels at a viewbox scale of one. Angles in transforms and animation are in degrees. Angles in SDF arcs and in splat rotation are in radians.

## Where each part is explained

- The canvas: [Canvas and viewbox](canvas.md)
- Style keys and paint values: [Style and paint](style-and-paint.md)
- Elements: [Element overview](elements-overview.md)
- Defs: [Gradients](gradients.md) and [Shaders](shaders.md)
- Animation: [Keyframe animation](animation.md)
