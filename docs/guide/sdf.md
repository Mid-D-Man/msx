# Signed distance fields

An `sdf` element draws a shape described by a distance function instead of an outline. For every pixel, the function returns the signed distance to the shape's edge: negative inside, positive outside, zero on the edge. Pixels with a negative distance are filled.

The distance form makes operations possible that outlines handle poorly. Two shapes can be merged with a smooth join, one shape can be carved out of another, and a shape can be grown or shrunk by a fixed amount. The renderers evaluate the field per pixel, so edges stay exact at any size.

<!-- example: sdf_shapes | nosource -->

## The sdf element

```dixscript
{ type = "sdf", fill = #4a9eff,
  transform = { type = "translate", x = 75, y = 100 },
  tree = { type = "circle", cx = 0, cy = 0, r = 55 } }
```

| Field | Meaning | Default |
|---|---|---|
| `tree` | The shape tree | required |
| `fill` | Paint for the inside | none |
| `stroke` | Paint for the edge | none |
| `stroke_width` | Edge width. Read only when `stroke` is set | |
| `id`, `transform` | As for every element | |

A node with neither a fill nor a stroke draws nothing. The usual pattern is to build each shape around the origin and position it with `transform`, as the example above does.

## Primitives

| Type | Fields | Defaults |
|---|---|---|
| `circle` | `cx`, `cy`, `r` | `cx` and `cy` 0, `r` required |
| `box` | `x`, `y`, `width`, `height`, `corner_radius` | `x` and `y` 0, size required, radius 0 |
| `line` | `x1`, `y1`, `x2`, `y2`, `thickness` | coordinates 0, thickness 1 |
| `ring` | `cx`, `cy`, `r`, `thickness` | center 0, `r` required, thickness 1 |
| `arc` | `cx`, `cy`, `r`, `angle_start`, `angle_end`, `thickness` | center 0, `r` required, angles 0 and 2π, thickness 1 |

`box` is positioned by its top left corner, like a `rect`. A box with `x = -55`, `y = -45`, `width = 110` and `height = 90` is centered on the origin. `thickness` is the full width of the band, so a `ring` with `r = 48` and `thickness = 16` covers the radii from 40 to 56. Angles of an `arc` are in radians, measured from the positive x axis and increasing clockwise on screen. The arc runs from `angle_start` up to `angle_end`, and its ends are rounded.

> **Warning** Set both angles of an `arc`. The defaults, 0 and 2π, name the same direction, so the arc covers almost nothing and draws only a dot at its start. Use a `ring` for a full circle.

## Operations

An operation combines other trees. Its children are written as nested objects.

| Type | Fields | Result |
|---|---|---|
| `union` | `children` | Everything covered by any child |
| `smooth_union` | `children`, `k` | A union with a rounded join |
| `subtract` | `a`, `b` | `a` with `b` carved out |
| `smooth_subtract` | `a`, `b`, `k` | A subtraction with a rounded edge |
| `intersect` | `a`, `b` | Only the area both cover |
| `smooth_intersect` | `a`, `b`, `k` | An intersection with rounded corners |
| `offset` | `child`, `amount` | The child grown by `amount`, or shrunk when negative |

`union` and `smooth_union` take any number of children in an array. The other binary operations take exactly two trees named `a` and `b`. `offset` takes one tree named `child`. The default `k` of a smooth union is 0.1, and `amount` defaults to 0.

```dixscript
{ type = "sdf", fill = #ff6b6b,
  transform = { type = "translate", x = 225, y = 280 },
  tree = { type = "smooth_union", k = 14,
    children = [ { type = "circle", cx = -22, cy = 0, r = 38 },
                 { type = "circle", cx =  22, cy = 0, r = 38 } ] } }
```

## Choosing k

`k` is the width of the blend zone, in the same units as the distances, which are pixels in the node's own coordinate space. The smooth join moves the edge by at most a quarter of `k`. A `k` below about 4 is therefore hard to see on shapes a few dozen pixels across, and values between 10 and 30 give a clear fillet. A `k` of 0 or less gives the hard operation.

## Where it renders

The CPU and GPU renderers evaluate the tree per pixel. The SVG renderer has no way to express the shape and writes a comment in its place, so an `sdf` element is missing from SVG output. Shapes in this form can also take a shader fill on the GPU, as in the `sdf_shader_fill` example.

For animation, an `sdf` node responds to transform tracks only. Its opacity is not a channel.
