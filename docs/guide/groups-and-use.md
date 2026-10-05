# Groups and use

`group` bundles child elements under one transform. `use` draws a second copy of an element that has an `id`.

## Group

```dixscript
{ type = "group", transform = "translate(100, 50)",
  elements = [
    { type = "rect", x = 0, y = 0, width = 80, height = 40, style = { fill = #4a9eff } },
    { type = "text", x = 40, y = 26, content = "OK",
      style = { fill = #ffffff, text_anchor = "middle" } }
  ] }
```

| Field | Meaning |
|---|---|
| `elements` | The children, written as an array in the same form as the top-level list |
| `transform` | Applied to every child. A child's own transform applies first |
| `style` | Optional. In SVG output the children inherit it, see the limitation below |

Groups nest. A child group's transform combines with its parent's. Children paint in order, and `layer` children among them are reordered by `z_index` within the group. [Layers](layers.md) explains that.

Groups are the usual way to build a component from several elements, and a QuickFunc that returns a group is how the `badge` function in the `parametric` example works.

> **Limitation** Style set on a group reaches the children in SVG output only. The CPU rasterizer does not push a group's style down to its children yet, and the GPU renderer reads each shape's own style. To keep the three renderers in agreement, give each child its own style.

## Use

```dixscript
{ type = "rect", id = "tile", x = 0, y = 0, width = 80, height = 80,
  style = { fill = "url(#base)", stroke = "none", stroke_width = 0 } }
{ type = "use", href = "#tile", x = 100, y = 60 }
{ type = "use", href = "#tile", x = 210, y = 60 }
```

| Field | Meaning | Default |
|---|---|---|
| `href` | `#` followed by the id of the element to draw | required |
| `x`, `y` | Offset added to the copy's position | 0, 0 |
| `transform` | Optional transform for the copy | none |

A `use` can point at any element that carries an `id`, including one nested inside a group or a layer. The original element still draws at its own position, so the example above produces three tiles: one at the origin and one at each offset. A reference to an id that does not exist draws nothing.

A `use` takes no `style` of its own. The copy looks like the element it references. When many copies need different colors or sizes, a QuickFunc that takes parameters fits better than `use`.
