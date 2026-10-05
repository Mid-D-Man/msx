# Transforms

A `transform` field moves, scales, rotates or skews an element and everything inside it. Rects, circles, ellipses, lines, polylines, polygons, paths, text, groups, `use` elements, images, SDF nodes and layers all accept one. Splats do not.

## Three ways to write a transform

**A string in SVG syntax.** Several operations can follow each other in one string.

```dixscript
transform = "translate(120, 80) rotate(45)"
```

The accepted names are `translate`, `scale`, `rotate`, `skewX`, `skewY` and `matrix`, and arguments are separated by commas or spaces. A name the parser does not know is dropped without an error.

**A single object.** The `type` field names the operation.

```dixscript
transform = { type = "rotate", angle = 45, cx = 100, cy = 100 }
```

**An array of objects.** The array is a chain of operations.

```dixscript
transform = [
  { type = "translate", x = 120, y = 80 },
  { type = "rotate", angle = 45 }
]
```

## Operations

| Type | Fields | Defaults |
|---|---|---|
| `translate` | `x`, `y` | 0, 0 |
| `scale` | `x`, `y` | x is 1, y is the value of x |
| `rotate` | `angle`, `cx`, `cy` | angle 0, pivot at the origin when `cx` and `cy` are omitted |
| `skew_x` | `angle` | 0 |
| `skew_y` | `angle` | 0 |
| `matrix` | `a`, `b`, `c`, `d`, `e`, `f` | the identity matrix |

Angles are in degrees. A positive angle turns clockwise on screen, because the y axis points down. In the string form, the skew names are written `skewX` and `skewY`.

## Order in a chain

A chain follows SVG: the last operation is applied to the element first. The two examples above are the same transform, and both place the element's own origin at (120, 80) after rotating the element about the origin.

Swapping the order changes the result.

| Chain | Effect |
|---|---|
| `translate(120, 80) rotate(45)` | Rotate the element about the origin, then move it to (120, 80) |
| `rotate(45) translate(120, 80)` | Move the element to (120, 80), then rotate that point about the origin, which swings it along an arc |

For a rotation about the element's own center, give `rotate` the pivot (`cx`, `cy`) instead of building a chain.

## Transforms and animation

A keyframe track adds a translate, scale or rotate on top of an element's static transform. The animated part is applied outside the static one, so a keyframed rotation turns the element about the origin of its parent space, not about its own center. [Keyframe animation](animation.md) shows the usual ways to set the pivot.

