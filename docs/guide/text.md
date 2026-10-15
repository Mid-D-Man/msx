# Text

```dixscript
{ type = "text", x = 300, y = 390, content = "MSX Basic Shapes",
  style = { fill = #ffffff, font_size = 14, text_anchor = "middle" } }
```

<!-- example: text_demo -->

| Field | Meaning | Required |
|---|---|---|
| `x`, `y` | Anchor position. `y` is the baseline | yes |
| `content` | The string to draw | yes |

Text takes the usual `id` and `transform` fields and a `style`. The keys that shape the text are `font_size`, `font_family`, `font_weight`, `text_anchor` and `dominant_baseline`. `text_anchor` chooses which part of the string sits at `x`: `start`, `middle` or `end`. `font_weight` takes `normal`, `bold` or a number such as 600. The fill color comes from `fill`.

## Where text renders

The SVG and CPU renderers draw text. The GPU renderer skips `text` elements for now.

The CPU renderer draws text as glyph outlines from one embedded face, DejaVu Sans, in regular and bold. The outlines go through the same path pipeline as any other shape, so fills, strokes and gradients all apply.

- `font_family` is ignored. Every text element uses DejaVu Sans.
- Bold applies when `font_weight` is `bold` or a number of 600 or more.
- Spacing comes from advance widths and the legacy kerning table. There is no OpenType shaping, so ligatures and complex scripts are not supported.
- The embedded subset covers Latin, Latin Extended-A, Greek, Cyrillic, common punctuation, arrows and a few math symbols. A character outside that set draws the font's missing glyph box.
- The browser draws SVG text with its own fonts, so the same string differs slightly in width between SVG and PNG output.

A gradient on text maps onto the text box, which is the advance width of the string by the height from ascender to descender. This matches the box SVG uses.

## Baselines

`y` is the alphabetic baseline unless `dominant_baseline` says otherwise. The CPU renderer reads these values: `middle`, `central`, `hanging`, `text-before-edge`, `text-top`, `text-after-edge`, `text-bottom` and `ideographic`. Any other value keeps the alphabetic baseline.

> **Limitation** The binary format has no slot for `dominant_baseline`, so a compiled file loses it and its text sits on the alphabetic baseline.

## Text and the roundtrip

Text survives the binary format unchanged. A compiled file renders the same SVG text as its source, which is what `msx roundtrip` checks.
