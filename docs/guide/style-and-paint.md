# Style and paint

Every drawable element takes a `style` object. Style controls how an element is filled and stroked and how its text is set. A scene that omits `style`, or omits any key inside it, gets the defaults below.

## Style keys

| Key | Value | Default |
|---|---|---|
| `fill` | paint | black |
| `stroke` | paint | none |
| `stroke_width` | number | 1 |
| `opacity` | number from 0 to 1 | 1 |
| `fill_opacity` | number from 0 to 1 | unset |
| `stroke_opacity` | number from 0 to 1 | unset |
| `fill_rule` | `nonzero` or `evenodd` | `nonzero` |
| `stroke_linecap` | `butt`, `round` or `square` | `butt` |
| `stroke_linejoin` | `miter`, `round` or `bevel` | `miter` |
| `stroke_miterlimit` | number | unset |
| `stroke_dasharray` | array of numbers | unset, a solid line |
| `stroke_dashoffset` | number | unset |
| `font_size` | number | unset |
| `font_family` | string | unset |
| `font_weight` | `normal`, `bold` or a number such as 600 | unset |
| `text_anchor` | `start`, `middle` or `end` | `start` |
| `dominant_baseline` | string passed to SVG | unset |
| `visibility` | `"hidden"` hides the element | visible |
| `display` | `"none"` removes the element | shown |

The defaults match SVG: black fill, no stroke, a stroke width of 1 when a stroke is set, and full opacity.

```dixscript
style = { fill = #4a9eff, stroke = #ffffff, stroke_width = 2.0, opacity = 0.9 }
```

## Paint values

`fill` and `stroke` take a paint. A paint can be written in any of these forms.

| Form | Example | Meaning |
|---|---|---|
| none | `"none"` | No paint |
| Hex | `#f80`, `#ff8800`, `#ff880080` | Three, six or eight digits. Eight digits end in an alpha byte |
| Function | `rgb(255, 136, 0)`, `rgba(0, 0, 0, 0.5)` | An alpha of 0.5 gives a byte value of 128 |
| Name | `black`, `white`, `red`, `green`, `blue` | Five color names are built in, plus `none`. `green` is `rgb(0, 128, 0)` |
| Reference | `"url(#sunset)"` | A gradient or shader def, by id |
| Inherit | `"currentColor"` | The current color of the context |

Hex values can be written without quotes as DixScript literals (`#4a9eff`), or as quoted strings. A reference is always a quoted string.

> **Warning** A paint string that matches none of these forms becomes `none`. There is no error, so a misspelled color shows up as a missing fill.

## Opacity

`opacity` applies to the whole element, fill and stroke together. `fill_opacity` and `stroke_opacity` scale one channel each in SVG output. An alpha written into the color itself (`#ff880080`) works in every renderer and in the compiled binary form, and it is the portable way to make one channel translucent. The binary form does not store `fill_opacity`, `stroke_opacity` or `dominant_baseline`.

## Where each renderer stands

The SVG renderer writes every style key into the output, so it is the reference for what a style means. The CPU rasterizer applies stroke width, dashes, caps, joins, the miter limit, the fill rule and opacity. It does not read `fill_opacity` or `stroke_opacity`, and it draws no text, so the font keys have no effect there. The GPU renderer honors `fill_rule` and `opacity` for ordinary shapes. A shape filled by a shader keeps the shader's own alpha, because the shape's `opacity` is not multiplied in. The [Rendering backends](rendering-backends.md) page collects these differences in one table.
