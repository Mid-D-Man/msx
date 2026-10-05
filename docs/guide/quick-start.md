# Quick start

This page builds the command line tool, renders a scene three ways, compiles it to binary and checks that the binary decodes to the same picture.

## Build

The workspace needs the `mbfa` crate in a sibling directory (`../mbfa`). The root `Cargo.toml` lists where each dependency comes from.

```bash
cargo build --release
```

The GPU commands need the `gpu` feature, which pulls in wgpu and is off by default.

```bash
cargo build --release --features gpu
```

## A first scene

The repository's `basic_shapes` example draws six rectangles, a circle and a line of text. The source and its rendering follow.

<!-- example: basic_shapes -->

## Render to SVG

`render` evaluates the source and writes SVG. A compiled binary works as input too.

```bash
cargo run --release -- render examples/basic_shapes.msx -o out.svg
```

## Rasterize to PNG

`rasterize` draws the scene on the CPU and always works, with no feature flag.

```bash
cargo run --release -- rasterize examples/basic_shapes.msx -o out.png
```

The CPU rasterizer does not draw `text` elements, so the caption in the example appears in the SVG output only. The [Rendering backends](rendering-backends.md) page lists what each renderer supports.

## Compile to binary

`compile` writes the binary form. The output path is required, and the payload is MBFA compressed unless `--no-compress` is given.

```bash
cargo run --release -- compile examples/basic_shapes.msx -o out.msx
cargo run --release -- render out.msx -o recovered.svg
```

## Check the roundtrip

`roundtrip` compiles the source, decodes the binary, renders both and compares the SVG text. It also compares duration, loop mode and animation tracks with a float tolerance, because static SVG cannot show those.

```bash
cargo run --release -- roundtrip examples/basic_shapes.msx
```

## Validate and inspect

```bash
cargo run --release -- validate examples/basic_shapes.msx
cargo run --release -- info out.msx
```

`validate` parses and checks the schema without producing output. `info` prints the header and scene statistics for a source or binary file.

## Animation and shaders

`animate` samples a keyframed scene on the CPU and writes a looping GIF.

```bash
cargo run --release -- animate examples/orbit_pulse.msx -o out.gif
```

Shader fills run only on the GPU renderer. Both GPU commands need a build with `--features gpu` and a GPU adapter, which may be a software one.

```bash
cargo run --release --features gpu -- rasterize-gpu examples/shader_orb.msx -o out.png --time 1.5
cargo run --release --features gpu -- animate-gpu examples/shader_orb.msx -o out.gif --duration 4 --fps 24
```

The [Command line reference](cli.md) lists every command and flag. To try a scene without installing anything, the [playground](/playground/) runs the SVG renderer in the browser and plays keyframe animation.

## Run the tests

```bash
cargo test -- --nocapture
cargo bench --bench compare
```
