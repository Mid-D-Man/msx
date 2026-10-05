# Binary format

A compiled MSX file holds the evaluated scene: every function call is resolved, every color is four bytes and every coordinate is a number. It is the form to ship, store or hand to another tool. The decoder rebuilds the same `Scene` that the parser produces, so every command accepts either form.

```bash
msx compile scene.msx -o scene.bin.msx
msx info scene.bin.msx
```

The commands detect the file type from its first four bytes, so a binary file can be named anything.

## Layout at a glance

A file is a 32 byte header followed by a payload.

| Part | Contents |
|---|---|
| Header | Magic `MSX\0`, version, compression flag, flag bits, canvas size, counts |
| Background | Four bytes of RGBA |
| Viewbox | Four floats, present only when the viewbox flag is set |
| String pool | Every distinct string stored once, referenced by index |
| Defs | Gradients, shaders and audio |
| Elements | A stream of tagged elements, closed by an end marker |
| Animation | Duration, loop mode and tracks, present only when the animation flag is set |

Numbers are little-endian. Coordinates and sizes are 32 bit floats. The element stream is driven by a one byte tag per element: `0x00` is a rect, `0x01` a circle, and so on up to `0x11` for an image. Paths are stored as command bytes with their numbers, not as the `d` string, so a path decodes without parsing text. The full tag table, the style block, the paint encoding and the transform block are in the [full specification](format-spec.md).

## Header flags

| Bit | Meaning |
|---|---|
| 0 | A viewbox follows the background |
| 1 | Metadata is present |
| 2 | The file has defs |
| 3 | An animation section follows the elements |

The animation flag is set when the scene has a `duration`, a `loop_mode` or any track. A scene with a duration and no tracks still keeps both values through a roundtrip.

## Compression

The compression byte is `1` by default, and the payload after the header is compressed as one unit with MBFA. Compression is optional, and `msx compile --no-compress` writes the payload as is. MSX passes the whole stream to MBFA without splitting it by channel, because tag bytes, coordinates, colors and path commands recur in patterns that a general match finder picks up across element types.

## Versioning

The version byte is `2`. It changes only when a change would make an older decoder misread tags it already dispatches on. Tags and sections added since, namely shaders, images, audio and the animation section, are additive: an older decoder either never meets them or does not read past where it always stopped. Files compiled by the current tools carry version 2.

## What a compiled file keeps

The style block stores fill, stroke, opacity, stroke width, fill rule, line cap, line join, miter limit, the dash array and offset, the font keys and the visibility flags. A key the source did not set stays unset after a roundtrip, so a decoded scene renders the same SVG as the source.

Three style keys have no slot in the block: `fill_opacity`, `stroke_opacity` and `dominant_baseline`. They are dropped when a scene is compiled, and `msx roundtrip` reports a mismatch for a scene that sets them. Writing the alpha into the color (`#ff880080`) is the portable way to make a fill or stroke translucent.

## Embedded media

Images and audio defs store their bytes as raw bytes. The base64 text in a source file is decoded once, at parse time, and is not stored. A `source_ref` is stored as a path string through the string pool. `msx compile` checks that every shader `source_ref` resolves to a file relative to the input, so a mistyped shader path stops the build.

## Checking a build

`msx roundtrip` compiles a source file, decodes the binary, renders both scenes to SVG and compares the text. It also compares the duration, loop mode and tracks, which static SVG cannot show. Floats in tracks are compared with a small tolerance, because they pass through 32 bit storage.
