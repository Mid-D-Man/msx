// apps/msx-cli/src/main.rs
//! MSX command-line tool.
//!
//! Every command that takes an `.msx` file accepts either DixScript source
//! or a compiled binary — `load_scene`/`load_scene_bytes` sniff the `"MSX\0"`
//! magic bytes and dispatch to `msx-parser` or `msx-binary` accordingly,
//! so callers never have to say which kind of file they're pointing at.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use msx_ast::{Def, Scene};
use msx_render_core::{RenderTarget, Renderer};

#[derive(Parser)]
#[command(name = "msx", version, about = "MSX — MidStroke eXtension CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render an .msx file (source or binary) to SVG.
    Render {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compile DixScript source to a binary .msx file.
    Compile {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Skip MBFA compression on the binary payload.
        #[arg(long)]
        no_compress: bool,
    },
    /// Rasterize an .msx file (source or binary) to a PNG via msx-render-cpu.
    Rasterize {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Rasterize an .msx file to a PNG via msx-render-gpu instead of the
    /// CPU rasterizer — the only path that actually executes `Def::Shader`
    /// WGSL fills for real rather than painting them with `fallback_color`.
    /// Only exists in builds compiled with `--features gpu` (pulls in the
    /// full wgpu dependency tree, off by default — see msx-cli's
    /// Cargo.toml). Falls back to a clear error, not a panic, if no GPU
    /// adapter (real or software) is available on the machine running it.
    #[cfg(feature = "gpu")]
    RasterizeGpu {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Time (seconds) fed to every shader-def's free-running `time`
        /// uniform — see msx-render-gpu's shader.rs for the convention.
        #[arg(long, default_value_t = 0.0)]
        time: f64,
    },
    /// Sample an animated .msx file's timeline (msx-anim) and export it as
    /// a looping animated GIF via msx-render-cpu. Errors if the scene has
    /// no animation tracks — use `rasterize` for a static render.
    Animate {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Frames per second to sample the timeline at.
        #[arg(long, default_value_t = 24)]
        fps: u32,
    },
    /// GPU counterpart to `Animate` — samples BOTH clocks that can move in
    /// this project at once (the msx-anim keyframe timeline AND a
    /// shader-def's `time` uniform) at the same `t` per frame and exports
    /// the result as a GIF via msx-render-gpu. `RasterizeGpu` only ever
    /// drove the shader clock for a single static frame; this is what
    /// actually animates it. Only exists in builds compiled with
    /// `--features gpu` — see `RasterizeGpu`.
    #[cfg(feature = "gpu")]
    AnimateGpu {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Frames per second to sample at.
        #[arg(long, default_value_t = 24)]
        fps: u32,
        /// Sweep length in seconds. Required unless the scene has its own
        /// `animations::` keyframe tracks to infer a length from (see
        /// `cmd_animate_gpu`'s doc comment) — a pure shader-time sweep has
        /// no timeline length of its own.
        #[arg(long)]
        duration: Option<f64>,
        /// Export a single-pass (non-repeating) GIF instead of the
        /// default infinite loop.
        #[arg(long)]
        no_loop: bool,
    },
    /// Print canvas/element/def stats for an .msx file (source or binary).
    Info { input: PathBuf },
    /// Parse + schema-validate DixScript source only; exit code reflects success.
    Validate { input: PathBuf },
    /// source → binary → decode → render; verify the SVG output matches.
    Roundtrip { input: PathBuf },
    /// Rasterize to a temp PNG and open it in the system's default image viewer.
    View { input: PathBuf },
}

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Render { input, output } => cmd_render(input, output.as_deref()),
        Command::Compile { input, output, no_compress } => cmd_compile(input, output, *no_compress),
        Command::Rasterize { input, output } => cmd_rasterize(input, output.as_deref()),
        #[cfg(feature = "gpu")]
        Command::RasterizeGpu { input, output, time } => cmd_rasterize_gpu(input, output.as_deref(), *time),
        Command::Animate { input, output, fps } => cmd_animate(input, output.as_deref(), *fps),
        #[cfg(feature = "gpu")]
        Command::AnimateGpu { input, output, fps, duration, no_loop } =>
            cmd_animate_gpu(input, output.as_deref(), *fps, *duration, *no_loop),
        Command::Info { input } => cmd_info(input),
        Command::Validate { input } => cmd_validate(input),
        Command::Roundtrip { input } => cmd_roundtrip(input),
        Command::View { input } => cmd_view(input),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

// ── Commands ─────────────────────────────────────────────────────────────────

fn cmd_render(input: &Path, output: Option<&Path>) -> Result<(), String> {
    let scene = load_scene(input)?;
    let svg = msx_render_svg::render(&scene);
    let out_path = output.map(PathBuf::from).unwrap_or_else(|| input.with_extension("svg"));
    std::fs::write(&out_path, svg).map_err(|e| format!("failed to write {}: {}", out_path.display(), e))?;
    println!("Wrote {}", out_path.display());
    Ok(())
}

fn cmd_compile(input: &Path, output: &Path, no_compress: bool) -> Result<(), String> {
    let scene = load_scene(input)?;
    validate_shader_refs(&scene, input)?;
    let binary = msx_binary::compile(&scene, !no_compress).map_err(|e| format!("compile failed: {}", e))?;
    std::fs::write(output, &binary).map_err(|e| format!("failed to write {}: {}", output.display(), e))?;
    println!("Wrote {} ({} bytes)", output.display(), binary.len());
    Ok(())
}

/// Confirms every `Def::Shader::source_ref` in the scene resolves to a
/// real file, relative to `input`'s own directory — this is the "resolves
/// and validates the reference at compile time" `ShaderDef`'s doc comment
/// in msx-ast promises. Catches a typo'd/moved shader path here, at
/// compile time, rather than it silently doing nothing at render time —
/// every renderer today falls back to `fallback_color` regardless of
/// whether `source_ref` even resolves, so a bad path otherwise wouldn't
/// surface as an error anywhere at all.
fn validate_shader_refs(scene: &Scene, input: &Path) -> Result<(), String> {
    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
    for def in &scene.defs {
        if let Def::Shader(shader) = def {
            let resolved = base_dir.join(&shader.source_ref);
            if !resolved.is_file() {
                return Err(format!(
                    "shader def '{}': source_ref '{}' does not resolve to a file (looked for {})",
                    shader.id, shader.source_ref, resolved.display()
                ));
            }
        }
    }
    Ok(())
}

fn cmd_rasterize(input: &Path, output: Option<&Path>) -> Result<(), String> {
    let scene = load_scene(input)?;
    let target = render_to_target(&scene);
    let (w, h) = (target.width, target.height);
    let out_path = output.map(PathBuf::from).unwrap_or_else(|| input.with_extension("png"));
    save_png(target, &out_path)?;
    println!("Wrote {} ({}x{})", out_path.display(), w, h);
    Ok(())
}

/// GPU counterpart to `cmd_rasterize` — same shape, different backend.
/// `GpuRenderer::new()` performs the whole instance → adapter → device
/// handshake (see `msx-render-gpu::context::GpuContext`, including its
/// two-stage real-then-software-fallback adapter request) and surfaces a
/// clear `Err` rather than panicking if no adapter is available at all,
/// which this function propagates as an ordinary CLI error rather than a
/// crash — a machine with no GPU and no software fallback driver
/// installed is an expected, diagnosable situation, not a bug.
#[cfg(feature = "gpu")]
fn cmd_rasterize_gpu(input: &Path, output: Option<&Path>, time: f64) -> Result<(), String> {
    let scene = load_scene(input)?;
    let renderer = msx_render_gpu::GpuRenderer::new().map_err(|e| format!("GPU renderer unavailable: {e}"))?;

    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
    let mut target = RenderTarget::new(
        scene.canvas.width.round().max(1.0) as u32,
        scene.canvas.height.round().max(1.0) as u32,
    );
    renderer.render_with_shader_dir(&scene, &mut target, base_dir, time as f32);

    let (w, h) = (target.width, target.height);
    // Deliberately a different default filename than `rasterize`'s
    // (`.gpu.png` vs `.png`) so running both commands against the same
    // input — the obvious way to compare CPU vs GPU output, or a
    // flat-fallback render vs a real WGSL-executed one — doesn't have one
    // silently overwrite the other.
    let out_path = output.map(PathBuf::from).unwrap_or_else(|| input.with_extension("gpu.png"));
    save_png(target, &out_path)?;
    println!("Wrote {} ({}x{}, GPU, t={})", out_path.display(), w, h, time);
    Ok(())
}

fn cmd_animate(input: &Path, output: Option<&Path>, fps: u32) -> Result<(), String> {
    if fps == 0 {
        return Err("--fps must be greater than 0".to_string());
    }
    let scene = load_scene(input)?;
    if !scene.is_animated() {
        return Err(
            "scene has no animation tracks (or an effective duration of 0) — nothing to \
             animate; use `msx rasterize` for a static render"
                .to_string(),
        );
    }

    let duration = scene.effective_duration();
    let frame_dt = 1.0 / fps as f64;

    // A `PingPong` timeline's forward+backward motion only exists across a
    // full `duration * 2` cycle (see `LoopMode::resolve_time`) — sampling
    // just `duration` would bake in only the forward half. `Once`/`Loop`
    // both complete a full pass in a single `duration`-long span.
    let playback_span = if scene.loop_mode == msx_ast::LoopMode::PingPong {
        duration * 2.0
    } else {
        duration
    };

    let mut frames = Vec::new();
    if scene.loop_mode == msx_ast::LoopMode::Once {
        // Include the exact final frame so playback rests on it — a
        // non-repeating GIF has nothing to loop back to.
        let count = (playback_span / frame_dt).ceil().max(1.0) as u32;
        for i in 0..=count {
            let t = (i as f64 * frame_dt).min(playback_span);
            frames.push(render_frame(&scene, t, frame_dt)?);
        }
    } else {
        // Loop/PingPong: the sample at exactly `playback_span` is
        // identical to the sample at `0` — that's what makes it a loop.
        // Including both would bake in a one-frame stutter every repeat.
        let count = (playback_span / frame_dt).round().max(1.0) as u32;
        for i in 0..count {
            let t = i as f64 * frame_dt;
            frames.push(render_frame(&scene, t, frame_dt)?);
        }
    }
    let frame_count = frames.len();

    let out_path = output.map(PathBuf::from).unwrap_or_else(|| input.with_extension("gif"));
    let file = std::fs::File::create(&out_path)
        .map_err(|e| format!("failed to create {}: {}", out_path.display(), e))?;
    let mut encoder = image::codecs::gif::GifEncoder::new(file);
    if scene.loop_mode != msx_ast::LoopMode::Once {
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .map_err(|e| format!("failed to set GIF loop: {}", e))?;
    }
    encoder
        .encode_frames(frames)
        .map_err(|e| format!("GIF encode failed: {}", e))?;

    println!(
        "Wrote {} ({} frames @ {}fps, {:.2}s timeline, loop_mode={:?})",
        out_path.display(),
        frame_count,
        fps,
        duration,
        scene.loop_mode
    );
    Ok(())
}

/// Resolves the scene at time `t` (msx-anim) and rasterizes that static
/// pose into one GIF-ready frame.
fn render_frame(scene: &Scene, t: f64, frame_dt: f64) -> Result<image::Frame, String> {
    let resolved = msx_anim::resolve_at_time(scene, t);
    let target = render_to_target(&resolved);
    let img = rgba_image_from_target(target)?;
    let delay = image::Delay::from_saturating_duration(std::time::Duration::from_secs_f64(frame_dt));
    Ok(image::Frame::from_parts(img, 0, 0, delay))
}

/// GPU counterpart to `cmd_animate` — same shape, but drives BOTH clocks
/// that can move in this project at once, not just one:
///
/// 1. `msx-anim`'s keyframe timeline (`resolve_at_time`, TRS + opacity,
///    applies uniformly across every element type including SDF/Splat/
///    Layer — see `core/msx-anim/src/resolver.rs`'s `apply_delta`), and
/// 2. `msx-render-gpu`'s shader-def `time` uniform (`render_with_shader_dir`,
///    currently only reaches vector-shape shader fills — see `shader.rs`'s
///    known gaps for what it doesn't reach yet: SDF, Splat, anything
///    inside a `Layer`).
///
/// Both are sampled at the SAME `t` per frame, so a scene that ever ends
/// up using both systems together advances them in lockstep rather than
/// needing two separate exports stitched by hand. `cmd_rasterize_gpu`
/// only ever drove clock 2, for one static frame; this is what actually
/// animates it.
///
/// `--duration` is required UNLESS the scene has its own keyframe
/// timeline (`scene.is_animated()`), in which case `scene.effective_duration()`
/// is used automatically — a pure shader-time sweep (no `animations::`
/// block at all, e.g. `shader_orb.msx`) has no timeline length of its own
/// to infer, so it has to be told explicitly how long to sample.
///
/// Deliberately simpler than `cmd_animate` in one respect: no special
/// `PingPong` playback-span doubling. `scene.loop_mode` still isn't
/// consulted for anything here (unlike `cmd_animate`, which reads it to
/// decide GIF repeat) — repeat is controlled directly by `--no-loop`
/// instead, since `PingPong`'s forward+backward half only makes sense for
/// the keyframe clock, and no example in this repo combines `PingPong`
/// keyframes with a shader-def yet. Add it the moment that combination
/// actually exists, rather than speculatively now.
#[cfg(feature = "gpu")]
fn cmd_animate_gpu(
    input: &Path,
    output: Option<&Path>,
    fps: u32,
    duration: Option<f64>,
    no_loop: bool,
) -> Result<(), String> {
    if fps == 0 {
        return Err("--fps must be greater than 0".to_string());
    }
    let scene = load_scene(input)?;
    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));

    let duration = match duration {
        Some(d) if d > 0.0 => d,
        Some(_) => return Err("--duration must be greater than 0".to_string()),
        None => {
            let d = scene.effective_duration();
            if d <= 0.0 {
                return Err(
                    "scene has no animation tracks, so it has no timeline length of its \
                     own to infer — pass --duration <seconds> explicitly to sample the \
                     shader's time uniform over an arbitrary span"
                        .to_string(),
                );
            }
            d
        }
    };

    let renderer = msx_render_gpu::GpuRenderer::new().map_err(|e| format!("GPU renderer unavailable: {e}"))?;
    let frame_dt = 1.0 / fps as f64;
    let count = (duration / frame_dt).round().max(1.0) as u32;

    let mut frames = Vec::with_capacity(count as usize);
    for i in 0..count {
        let t = i as f64 * frame_dt;
        // Keyframe clock: only actually changes anything if the scene has
        // tracks at all — `resolve_at_time` is a no-op clone otherwise,
        // so this is always safe to call regardless of what triggered
        // this function (an explicit --duration on a shader-only scene,
        // or an inferred one on a keyframed scene).
        let resolved = msx_anim::resolve_at_time(&scene, t);

        let mut target = RenderTarget::new(
            resolved.canvas.width.round().max(1.0) as u32,
            resolved.canvas.height.round().max(1.0) as u32,
        );
        // Shader clock: fed straight through, independent of whatever
        // resolve_at_time did or didn't change above.
        renderer.render_with_shader_dir(&resolved, &mut target, base_dir, t as f32);

        let img = rgba_image_from_target(target)?;
        let delay = image::Delay::from_saturating_duration(std::time::Duration::from_secs_f64(frame_dt));
        frames.push(image::Frame::from_parts(img, 0, 0, delay));
    }
    let frame_count = frames.len();

    // Deliberately `.gpu.gif`, distinct from both `cmd_animate`'s `.gif`
    // and `cmd_rasterize_gpu`'s `.gpu.png` — running all three against
    // the same input (the obvious way to compare "CPU keyframes only" vs
    // "GPU shader, single frame" vs "GPU shader, animated") shouldn't
    // have any of them silently overwrite another.
    let out_path = output.map(PathBuf::from).unwrap_or_else(|| input.with_extension("gpu.gif"));
    let file = std::fs::File::create(&out_path)
        .map_err(|e| format!("failed to create {}: {}", out_path.display(), e))?;
    let mut encoder = image::codecs::gif::GifEncoder::new(file);
    if !no_loop {
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .map_err(|e| format!("failed to set GIF loop: {}", e))?;
    }
    encoder
        .encode_frames(frames)
        .map_err(|e| format!("GIF encode failed: {}", e))?;

    println!(
        "Wrote {} ({} frames @ {}fps, {:.2}s sweep, GPU)",
        out_path.display(),
        frame_count,
        fps,
        duration,
    );
    Ok(())
}

fn cmd_info(input: &Path) -> Result<(), String> {
    let bytes = std::fs::read(input).map_err(|e| format!("failed to read {}: {}", input.display(), e))?;
    let is_binary = bytes.len() >= 4 && &bytes[0..4] == b"MSX\0";
    let scene = load_scene_bytes(&bytes)?;

    println!("File:       {}", input.display());
    println!("Format:     {}", if is_binary { "binary (MBFA)" } else { "DixScript source" });
    println!("Canvas:     {} x {}", scene.canvas.width, scene.canvas.height);
    println!("Background: {}", scene.canvas.background.to_svg_hex());
    println!("Elements:   {} top-level ({} recursive)", scene.elements.len(), scene.element_count());
    println!("Defs:       {}", scene.defs.len());

    if is_binary {
        let header = msx_binary::MsxHeader::parse(&bytes).map_err(|e| format!("header parse failed: {}", e))?;
        println!("Binary version: {}", header.version);
        println!("Compressed:     {}", header.compress == msx_binary::COMPRESS_MBFA);
        println!("Raw size:       {} bytes", bytes.len());
    }

    Ok(())
}

fn cmd_validate(input: &Path) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|e| format!("failed to read {}: {}", input.display(), e))?;
    let scene = msx_parser::parse_scene(&source)?;
    println!("OK — {} top-level element(s), {} def(s)", scene.elements.len(), scene.defs.len());
    Ok(())
}

fn cmd_roundtrip(input: &Path) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|e| format!("failed to read {}: {}", input.display(), e))?;
    let scene_a = msx_parser::parse_scene(&source)?;
    let svg_a = msx_render_svg::render(&scene_a);

    let binary = msx_binary::compile(&scene_a, true).map_err(|e| format!("compile failed: {}", e))?;
    let scene_b = msx_binary::decode(&binary).map_err(|e| format!("decode failed: {}", e))?;
    let svg_b = msx_render_svg::render(&scene_b);

    let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalize(&svg_a) == normalize(&svg_b) {
        println!(
            "PASS — {} element(s), {} bytes binary ({} bytes SVG, {:.1}%)",
            scene_a.element_count(),
            binary.len(),
            svg_a.len(),
            binary.len() as f64 / svg_a.len() as f64 * 100.0,
        );
        Ok(())
    } else {
        Err("FAIL — SVG output differs between source-parsed and binary-decoded scenes".to_string())
    }
}

fn cmd_view(input: &Path) -> Result<(), String> {
    let scene = load_scene(input)?;
    let target = render_to_target(&scene);

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let tmp_path = std::env::temp_dir().join(format!("msx-view-{}.png", millis));

    save_png(target, &tmp_path)?;
    open_in_system_viewer(&tmp_path)?;

    println!(
        "Opened {} in your system's default image viewer.\n\
         (Static render, not a live window — that's what apps/msx-viewer will be.)",
        tmp_path.display()
    );
    Ok(())
}

// ── Shared helpers ───────────────────────────────────────────────────────────

fn load_scene(path: &Path) -> Result<Scene, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    load_scene_bytes(&bytes)
}

fn load_scene_bytes(bytes: &[u8]) -> Result<Scene, String> {
    if bytes.len() >= 4 && &bytes[0..4] == b"MSX\0" {
        msx_binary::decode(bytes).map_err(|e| format!("binary decode failed: {}", e))
    } else {
        let source = std::str::from_utf8(bytes).map_err(|_| "input is not valid UTF-8 DixScript source".to_string())?;
        msx_parser::parse_scene(source)
    }
}

fn render_to_target(scene: &Scene) -> RenderTarget {
    let renderer = msx_render_cpu::CpuRenderer::new();
    let mut target = RenderTarget::new(
        scene.canvas.width.round().max(1.0) as u32,
        scene.canvas.height.round().max(1.0) as u32,
    );
    renderer.render(scene, &mut target);
    target
}

fn save_png(target: RenderTarget, path: &Path) -> Result<(), String> {
    let img = rgba_image_from_target(target)?;
    img.save(path).map_err(|e| format!("failed to save {}: {}", path.display(), e))
}

fn rgba_image_from_target(target: RenderTarget) -> Result<image::RgbaImage, String> {
    let (width, height) = (target.width, target.height);
    image::RgbaImage::from_raw(width, height, target.into_bytes())
        .ok_or_else(|| "failed to build image buffer from render target".to_string())
}

fn open_in_system_viewer(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).status();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd").args(["/C", "start", "", &path.display().to_string()]).status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(path).status();

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("viewer process exited with status {}", status)),
        Err(e) => Err(format!("failed to launch system viewer: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Canvas, Color};

    #[test]
    fn load_scene_bytes_detects_dixscript_source() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #ffffff }
  elements::
)
"#;
        let scene = load_scene_bytes(src.as_bytes()).expect("source should parse");
        assert!((scene.canvas.width - 10.0).abs() < 1e-9);
    }

    #[test]
    fn load_scene_bytes_detects_binary() {
        let scene = Scene::new(Canvas::new(50.0, 50.0, Color::WHITE));
        let binary = msx_binary::compile(&scene, true).expect("compile should succeed");
        let decoded = load_scene_bytes(&binary).expect("binary should decode");
        assert!((decoded.canvas.width - 50.0).abs() < 1e-9);
    }

    #[test]
    fn load_scene_bytes_rejects_invalid_input() {
        let garbage = [0xFFu8, 0xFE, 0x00, 0x01, 0x02];
        assert!(load_scene_bytes(&garbage).is_err());
    }
            }
