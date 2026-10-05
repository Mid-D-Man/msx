# Images and audio

MSX carries raster images as elements and audio as a def. Both accept their bytes in one of two ways: embedded in the file as base64, or as a path to a file on disk.

## Image

```dixscript
{ type = "image", id = "icon", x = 150, y = 170, width = 120, height = 120,
  anchor = "center", data = "iVBORw0KGgo..." }
```

| Field | Meaning | Required |
|---|---|---|
| `x`, `y` | Position, interpreted through `anchor` | yes |
| `width`, `height` | Drawn size | yes |
| `data` | Base64 of a PNG or JPEG file | one of `data` or `source_ref` |
| `source_ref` | Path to an image file | one of `data` or `source_ref` |
| `anchor` | Which point of the image sits at `x`, `y` | `top_left` |
| `style` | `opacity` applies to the image | none |

Setting both `data` and `source_ref`, or neither, stops the parse. The image is scaled to `width` by `height`.

A `source_ref` path is relative to the `.msx` file. The renderers read the file when they draw, so a missing or invalid file shows up at render time and does not stop the parse. An embedded `data` value is checked at parse time. Its first bytes must look like a PNG or a JPEG, and a corrupted blob is reported with the element's path.

### Anchor

`anchor` chooses which point of the image the position refers to. The nine values are `top_left`, `top`, `top_right`, `left`, `center`, `right`, `bottom_left`, `bottom` and `bottom_right`. With the default `top_left`, `x` and `y` are the top left corner. With `center`, they are the center of the image. An unrecognized value is read as `top_left`.

## Embedded media in the binary format

An embedded image or audio blob is stored in the compiled file as raw bytes. The base64 form exists only in the source file and in SVG output, where an image becomes a `data:` URI.

## Audio

```dixscript
defs::
  { type = "audio", id = "blip", data = "UklGRi..." }
```

An audio def has an `id` and the same `data` or `source_ref` choice as an image. It is stored and round-trips through the binary format without loss. No renderer plays it.

`msx extract-media` writes the bytes of an audio def to a file, which is the way to confirm that an embedded clip is intact.

```bash
msx extract-media scene.msx --id blip -o blip.wav
```

## Limits of the source format

A DixScript string cannot contain a line break, so an embedded blob is one long line in the source file. For anything beyond a small icon, `source_ref` keeps the source readable.
