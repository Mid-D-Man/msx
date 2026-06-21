// apps/msx-cli/src/main.rs
//! MSX command-line tool.
//!
//! Every command that takes an `.msx` file accepts either DixScript source
//! or a compiled binary — `load_scene`/`load_scene_bytes` sniff the `"MSX\0"`
//! magic bytes and dispatch to `msx-parser` or `msx-binary` accordingly,
//! so callers never have to say which kind of file they're pointing at.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use msx_ast::Scene;
use msx_render_core::{RenderTarget, Renderer};

#[derive(Parser)]
#[command(name = "msx", version, about = "MSX — MidStroke eXchange CLI")]
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
    let binary = msx_binary::compile(&scene, !no_compress).map_err(|e| format!("compile failed: {}", e))?;
    std::fs::write(output, &binary).map_err(|e| format!("failed to write {}: {}", output.display(), e))?;
    println!("Wrote {} ({} bytes)", output.display(), binary.len());
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
    let (width, height) = (target.width, target.height);
    let img = image::RgbaImage::from_raw(width, height, target.into_bytes())
        .ok_or_else(|| "failed to build image buffer from render target".to_string())?;
    img.save(path).map_err(|e| format!("failed to save {}: {}", path.display(), e))
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
