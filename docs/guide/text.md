# Text

```dixscript
{ type = "text", x = 300, y = 390, content = "MSX Basic Shapes",
  style = { fill = #ffffff, font_size = 14, text_anchor = "middle" } }
```

| Field | Meaning | Required |
|---|---|---|
| `x`, `y` | Anchor position. `y` is the baseline | yes |
| `content` | The string to draw | yes |

Text takes the usual `id` and `transform` fields and a `style`. The keys that shape the text are `font_size`, `font_family`, `font_weight`, `text_anchor` and `dominant_baseline`. `text_anchor` chooses which part of the string sits at `x`: `start`, `middle` or `end`. `font_weight` takes `normal`, `bold` or a number such as 600. The fill color comes from `fill`.

> **Limitation** Only the SVG renderer draws text. The CPU and GPU rasterizers skip `text` elements, because no font shaping or glyph rasterization is wired into them. A scene that relies on text for meaning should be exported as SVG.

## Text and the roundtrip

Text survives the binary format unchanged. A compiled file renders the same SVG text as its source, which is what `msx roundtrip` checks.
