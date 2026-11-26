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

Text takes the usual `id` and `transform` fields and a `style`. The keys that shape the text are `font_size`, `font_family`, `font_weight`, `font_style`, `letter_spacing`, `text_decoration`, `writing_mode`, `text_anchor` and `dominant_baseline`. `text_anchor` chooses which part of the string sits at `x`: `start`, `middle` or `end`. `font_weight` takes `normal`, `bold` or a number such as 600. The fill color comes from `fill`.

## Spacing, decoration and slant

```dixscript
{ type = "text", x = 30, y = 60, content = "Spaced and underlined",
  style = { fill = #ffffff, font_size = 22, letter_spacing = 3,
            text_decoration = "underline", font_style = "italic" } }
```

<!-- example: text_styles -->

| Key | Values | Default |
|---|---|---|
| `letter_spacing` | A number, the extra space after every glyph in user units. It can be negative | `0` |
| `text_decoration` | A list of `underline`, `overline` and `line-through`, or `none` | `none` |
| `font_style` | `normal`, `italic` or `oblique` | `normal` |
| `writing_mode` | `horizontal-tb` or `vertical-rl` | `horizontal-tb` |

- Letter spacing follows every glyph, the last one included, so it widens the text box and moves `middle` and `end` anchored text.
- Each decoration line is a box as wide as the text box. The CPU and GPU renderers take its position and thickness from the font's own metrics. The overline sits at the ascender. A decoration takes the fill and stroke of the text.
- `italic` and `oblique` look the same. The CPU and GPU renderers slant the upright face by a synthesized 12 degrees, because the embedded face has no italic. The text box and baseline do not change. An angle after `oblique` in the importer is read as `oblique`.
- The SVG renderer writes all three as attributes of the `text` element, and the browser draws them with its own font.
- `vertical-rl` sets the text running down the page, as `tb`, `tb-rl` and `vertical-lr` do in the importer. A single line reads the same in all of them. The glyphs turn a quarter turn clockwise, the text is centred on `x`, `y` is where it starts, and `text_anchor` works along `y`. Decoration lines run along the text beside the glyphs.
- In vertical text `dx` moves the column and `dy` moves along the text. Only the glyphs of the embedded face are drawn, so upright CJK glyphs are not part of this.
- A scene that sets one of these keys is written as binary version 3 (see [Binary format](binary-format.md#versions)).

## Text on a path

```dixscript
{ type = "text", x = 0, y = 0, content = "Some long text on a curve",
  path = "M 20 100 C 35 135 85 135 100 100 C 115 65 165 65 180 100",
  start_offset = 10,
  style = { fill = #ffffff, font_size = 24 } }
```

<!-- example: text_on_path -->

| Key | Meaning | Default |
|---|---|---|
| `path` | Path data. The first subpath is followed | none, the text is flat |
| `start_offset` | Distance along the path where the text starts, in user units | `0` |

- Each glyph stands on the path with the middle of its baseline, turned to the direction of the path there. A glyph whose middle falls before the start or past the end is not drawn.
- `x` adds to `start_offset` and `y` moves the baseline across the path, positive away from the side the glyphs stand on. `text_anchor` moves the text around the start, so `middle` centres it on `start_offset + x`.
- Letter spacing, size, weight and slant apply. Decoration lines, `writing_mode` and `dominant_baseline` do not.
- A path with no length draws nothing.
- The SVG renderer writes a `textPath` that points at a copy of the path in the `defs`, and the browser lays the glyphs out with its own font.
- A text with a path is written as binary version 3, in its own element tag.

## Where text renders

All three renderers draw text.

The CPU and GPU renderers draw text as glyph outlines from one embedded face, DejaVu Sans, in regular and bold. The outlines go through the same path pipeline as any other shape, so fills, strokes and gradients all apply. The GPU renderer does not antialias, so its text edges are harder than the CPU renderer's.

- `font_family` is ignored. Every text element uses DejaVu Sans.
- Bold applies when `font_weight` is `bold` or a number of 600 or more.
- Spacing comes from advance widths and the legacy kerning table. There is no OpenType shaping, so ligatures and complex scripts are not supported.
- The embedded subset covers Latin, Latin Extended-A, Greek, Cyrillic, common punctuation, arrows and a few math symbols. A character outside that set draws the font's missing glyph box.
- The browser draws SVG text with its own fonts, so the same string differs slightly in width between SVG and PNG output.

A gradient on text maps onto the text box, which is the advance width of the string by the height from ascender to descender. This matches the box SVG uses.

## Baselines

`y` is the alphabetic baseline unless `dominant_baseline` says otherwise. The CPU and GPU renderers read these values: `middle`, `central`, `hanging`, `text-before-edge`, `text-top`, `text-after-edge`, `text-bottom` and `ideographic`. Any other value keeps the alphabetic baseline.

From binary version 3 the compiled file keeps `dominant_baseline`, and a scene that sets it is written as version 3 (see [Binary format](binary-format.md#versions)).

Text has no `baseline-shift` key. The SVG importer reads `baseline-shift` on spans and puts the result in the position of each text, so a raised piece is a text with a smaller `y` (or a larger `x` in vertical text). On a path the shift is a negative `y`, which moves the baseline toward the side the glyphs stand on. See [SVG import](svg-import.md) for the values it reads.

## Text and the roundtrip

Text survives the binary format unchanged. A compiled file renders the same SVG text as its source, which is what `msx roundtrip` checks.
