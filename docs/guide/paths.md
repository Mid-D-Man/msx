# Paths, polylines and polygons

Three elements describe outlines that are not a single primitive shape. `polyline` and `polygon` connect a list of points with straight segments. `path` accepts the full path data language of SVG.

<!-- example: path_demo -->

## Polyline and polygon

```dixscript
{ type = "polyline", points = [[20, 120], [60, 40], [100, 120], [140, 40]],
  style = { fill = "none", stroke = #f5a623, stroke_width = 3 } }

{ type = "polygon", points = [[200, 40], [260, 40], [230, 110]],
  style = { fill = #a78bfa } }
```

`points` is an array of `[x, y]` pairs. A `polygon` closes the shape: the last point connects back to the first in the fill and in the stroke. A `polyline` stays open, so its stroke does not return to the start. A polyline with a stroke and no fill is the usual way to draw a zigzag.

## Path

```dixscript
{ type = "path", d = "M 250 50 L 450 430 L 50 430 Z",
  style = { fill = #3498db, stroke = #2980b9, stroke_width = 3 } }
```

`d` is a string of path data. It is parsed when the file loads, so a syntax error in `d` stops the parse with the element's path in the message, for example `elements[1].d: ...`.

## Path commands

Each command exists in an absolute form (capital letter) and a relative form (lowercase letter). A relative command measures from the current point.

| Command | Arguments | Draws |
|---|---|---|
| `M` | `x y` | Move to a new start point |
| `L` | `x y` | Line |
| `H` | `x` | Horizontal line |
| `V` | `y` | Vertical line |
| `C` | `x1 y1 x2 y2 x y` | Cubic Bezier curve |
| `S` | `x2 y2 x y` | Smooth cubic, reflecting the previous control point |
| `Q` | `x1 y1 x y` | Quadratic Bezier curve |
| `T` | `x y` | Smooth quadratic, reflecting the previous control point |
| `A` | `rx ry rotation large sweep x y` | Elliptical arc |
| `Z` | none | Close the path |

A command can be followed by several argument sets, which repeat the command. The `large` and `sweep` arguments of an arc are flags, 0 or 1. In `path_demo`, `M 80 350 A 130 130 0 0 1 420 350` draws an arc from (80, 350) to (420, 350) with no axis rotation, `large` set to 0 and `sweep` set to 1, which turns clockwise on screen.

## Building paths from parameters

For shapes that depend on values, an [interpolated string](source-files.md) computes the path data.

```dixscript
d = $"M {x} {y} L {x + w} {y} L {x + w / 2} {y + h} Z"
```
