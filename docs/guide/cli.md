# Command line reference

The `msx` tool is built from `apps/msx-cli`. Every command that takes an input accepts a DixScript source file or a compiled binary. The tool detects the form from the magic bytes at the start of the file.

```bash
cargo run --release -- <command> [arguments]
cargo run --release --features gpu -- <command> [arguments]
```

Two commands, `rasterize-gpu` and `animate-gpu`, exist only in a build made with `--features gpu`.

## Commands

| Command | Does |
|---|---|
| `render` | Writes SVG |
| `compile` | Writes the binary form |
| `rasterize` | Writes a PNG with the CPU renderer |
| `rasterize-gpu` | Writes a PNG with the GPU renderer |
| `animate` | Writes an animated GIF with the CPU renderer |
| `animate-gpu` | Writes an animated GIF with the GPU renderer |
| `info` | Prints statistics for a file |
| `validate` | Parses and checks a source file |
| `roundtrip` | Checks source to binary to decoded output |
| `view` | Opens a rasterized preview |
| `extract-media` | Writes the bytes of an audio def to a file |

## render

```bash
msx render <input> [-o <output>]
```

Writes SVG. Without `-o`, the output goes next to the input with the extension `.svg`.

## compile

```bash
msx compile <input> -o <output> [--no-compress]
```

Writes the binary form. The `-o` argument is required. `--no-compress` skips MBFA compression of the payload. The command first checks that each shader `source_ref` resolves to a file.

## rasterize

```bash
msx rasterize <input> [-o <output>]
```

Writes a PNG from the CPU renderer. The default output is the input name with `.png`. It draws no text and shows shader fills as their `fallback_color`.

## rasterize-gpu

```bash
msx rasterize-gpu <input> [-o <output>] [--time <seconds>]
```

Writes a PNG from the GPU renderer, the one path that runs shader fills. `--time` sets the shader `time` uniform and defaults to 0. When no GPU adapter exists, the command stops with an error message.

## animate

```bash
msx animate <input> [-o <output>] [--fps <n>]
```

Samples the keyframe timeline on the CPU and writes a looping GIF. `--fps` defaults to 24. A scene with no animation tracks is an error, so use `rasterize` for a still image.

## animate-gpu

```bash
msx animate-gpu <input> [-o <output>] [--fps <n>] [--duration <seconds>] [--no-loop]
```

Samples both the keyframe timeline and the shader clock at the same time for every frame. `--duration` is required unless the scene has keyframe tracks to take a length from. `--no-loop` writes a GIF that plays once.

## info

```bash
msx info <input>
```

Prints the canvas size, background, element counts and def count. For a binary file it also prints the version, whether it is compressed, and the size.

## validate

```bash
msx validate <input>
```

Parses a source file and checks the schema, without writing anything. The exit code reports success or failure.

## roundtrip

```bash
msx roundtrip <input>
```

Compiles to binary, decodes, renders both scenes and compares the SVG. The duration, loop mode and tracks are compared with a float tolerance.

## view

```bash
msx view <input>
```

Rasterizes to a temporary PNG and opens it in the system image viewer.

## extract-media

```bash
msx extract-media <input> --id <audio id> [-o <output>]
```

Writes the bytes of an audio def to a file. Without `-o`, the file is named after the id and gets an extension detected from the bytes, or `.bin` when the format is not recognized. The command does not play the audio.
