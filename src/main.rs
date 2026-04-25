// src/main.rs — MSX CLI
//
// Usage:
//   msx render   <input.msx>  [-o output.svg]  [--no-compress]
//   msx compile  <input.msx>  [-o output.msx]  [--no-compress]
//   msx info     <file.msx>
//   msx validate <source.msx>
//   msx roundtrip <source.msx>
//   msx bench    <file.msx>

use std::{env, fs, path::Path, process, time::Instant};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "render"    => cmd_render(&args),
        "compile"   => cmd_compile(&args),
        "info"      => cmd_info(&args),
        "validate"  => cmd_validate(&args),
        "roundtrip" => cmd_roundtrip(&args),
        "bench"     => cmd_bench(&args),
        _ => { eprintln!("unknown command: {}", args[1]); usage(); process::exit(1); }
    }
}

// ── render ────────────────────────────────────────────────────────────────────

fn cmd_render(args: &[String]) {
    let input = &args[2];
    let output = flag_value(args, "-o")
        .unwrap_or_else(|| default_output(input, "svg"));
    let compress = !has_flag(args, "--no-compress");

    let data = fs::read(input)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", input, e)));

    let t = Instant::now();

    // Detect whether input is binary MSX or source MSX
    let scene = if data.len() >= 4 && &data[0..4] == b"MSX\0" {
        msx::decode(&data)
            .unwrap_or_else(|e| die(&format!("decode binary MSX: {}", e)))
    } else {
        let source = String::from_utf8(data)
            .unwrap_or_else(|_| die("input is not valid UTF-8"));
        msx::parse_scene(&source)
            .unwrap_or_else(|e| die(&format!("parse MSX source: {}", e)))
    };

    let svg = msx::render(&scene);
    let elapsed = t.elapsed();

    fs::write(&output, &svg)
        .unwrap_or_else(|e| die(&format!("write '{}': {}", output, e)));

    println!("render: {} → {} ({} elements, {:.2}ms)",
        input, output, scene.element_count(), elapsed.as_secs_f64() * 1000.0);
    println!("  SVG size: {} bytes", svg.len());
}

// ── compile ───────────────────────────────────────────────────────────────────

fn cmd_compile(args: &[String]) {
    let input    = &args[2];
    let output   = flag_value(args, "-o")
        .unwrap_or_else(|| default_output(input, "msx"));
    let compress = !has_flag(args, "--no-compress");

    let source = fs::read_to_string(input)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", input, e)));

    let t = Instant::now();

    let scene = msx::parse_scene(&source)
        .unwrap_or_else(|e| die(&format!("parse: {}", e)));

    let binary = msx::compile(&scene, compress)
        .unwrap_or_else(|e| die(&format!("compile: {}", e)));

    let elapsed = t.elapsed();

    fs::write(&output, &binary)
        .unwrap_or_else(|e| die(&format!("write '{}': {}", output, e)));

    let svg_approx = msx::render(&scene).len();
    let ratio = binary.len() as f64 / svg_approx as f64 * 100.0;

    println!("compile: {} → {} ({:.2}ms)", input, output, elapsed.as_secs_f64() * 1000.0);
    println!("  elements: {}  defs: {}", scene.element_count(), scene.defs.len());
    println!("  binary:   {} bytes  (compressed: {})", binary.len(), compress);
    println!("  svg ~est: {} bytes  ({:.1}% of SVG size)", svg_approx, ratio);
}

// ── info ──────────────────────────────────────────────────────────────────────

fn cmd_info(args: &[String]) {
    let input = &args[2];
    let data  = fs::read(input)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", input, e)));

    if data.len() >= 4 && &data[0..4] == b"MSX\0" {
        // Binary MSX
        let header = msx::MsxHeader::parse(&data)
            .unwrap_or_else(|e| die(&format!("parse header: {}", e)));

        println!("MSX Binary — {}", input);
        println!("  version:       {}", header.version);
        println!("  dimensions:    {} × {}",    header.width, header.height);
        println!("  compress:      {}", if header.compress == 1 { "mbfa" } else { "none" });
        println!("  elem_count:    {}", header.elem_count);
        println!("  def_count:     {}", header.def_count);
        println!("  str_pool_len:  {} bytes", header.str_pool_len);
        println!("  has_viewbox:   {}", header.has_viewbox());
        println!("  has_defs:      {}", header.has_defs());
        println!("  file_size:     {} bytes", data.len());

        // Decode and show full stats
        match msx::decode(&data) {
            Ok(scene) => {
                let svg = msx::render(&scene);
                println!("  total_elements (recursive): {}", scene.element_count());
                println!("  rendered_svg_size: {} bytes", svg.len());
                println!("  ratio vs svg:      {:.1}%",
                    data.len() as f64 / svg.len() as f64 * 100.0);
            }
            Err(e) => eprintln!("  (decode failed: {})", e),
        }
    } else {
        // Source MSX
        let source = String::from_utf8(data.clone())
            .unwrap_or_else(|_| die("not valid UTF-8"));

        println!("MSX Source — {}", input);
        println!("  source_size: {} bytes", source.len());

        match msx::parse_scene(&source) {
            Ok(scene) => {
                let svg    = msx::render(&scene);
                let binary = msx::compile(&scene, true).unwrap_or_default();
                println!("  canvas:      {} × {}",
                    scene.canvas.width, scene.canvas.height);
                println!("  elements:    {} (top-level: {})",
                    scene.element_count(), scene.elements.len());
                println!("  defs:        {}", scene.defs.len());
                println!("  svg_size:    {} bytes", svg.len());
                println!("  binary_size: {} bytes (mbfa compressed)", binary.len());
                println!("  reduction:   {:.1}% vs source",
                    (1.0 - binary.len() as f64 / source.len() as f64) * 100.0);
                println!("  reduction:   {:.1}% vs svg",
                    (1.0 - binary.len() as f64 / svg.len() as f64) * 100.0);
            }
            Err(e) => eprintln!("  (parse failed: {})", e),
        }
    }
}

// ── validate ──────────────────────────────────────────────────────────────────

fn cmd_validate(args: &[String]) {
    let input = &args[2];
    let source = fs::read_to_string(input)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", input, e)));

    match msx::parse_scene(&source) {
        Ok(scene) => {
            println!("VALID — {} ({} elements, {} defs)",
                input, scene.element_count(), scene.defs.len());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("INVALID — {}", e);
            process::exit(1);
        }
    }
}

// ── roundtrip ─────────────────────────────────────────────────────────────────

fn cmd_roundtrip(args: &[String]) {
    let input = &args[2];
    let source = fs::read_to_string(input)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", input, e)));

    let scene_a = msx::parse_scene(&source)
        .unwrap_or_else(|e| die(&format!("parse source: {}", e)));

    let svg_a = msx::render(&scene_a);

    // Compile → decode → render again
    let binary = msx::compile(&scene_a, true)
        .unwrap_or_else(|e| die(&format!("compile: {}", e)));

    let scene_b = msx::decode(&binary)
        .unwrap_or_else(|e| die(&format!("decode: {}", e)));

    let svg_b = msx::render(&scene_b);

    // Compare SVG output (whitespace-normalised)
    let normalise = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let na = normalise(&svg_a);
    let nb = normalise(&svg_b);

    if na == nb {
        println!("PASS — roundtrip pixel-perfect ({} → {} → {} bytes)",
            source.len(), binary.len(), svg_b.len());
        println!("  element_count: {}", scene_a.element_count());
    } else {
        eprintln!("FAIL — SVG output differs after binary roundtrip");
        // Show first difference for debugging
        let chars_a: Vec<char> = na.chars().collect();
        let chars_b: Vec<char> = nb.chars().collect();
        for (i, (a, b)) in chars_a.iter().zip(chars_b.iter()).enumerate() {
            if a != b {
                let ctx_a = &na[i.saturating_sub(20)..((i + 20).min(na.len()))];
                let ctx_b = &nb[i.saturating_sub(20)..((i + 20).min(nb.len()))];
                eprintln!("  first diff at char {}: {:?} vs {:?}", i, ctx_a, ctx_b);
                break;
            }
        }
        process::exit(1);
    }
}

// ── bench ─────────────────────────────────────────────────────────────────────

fn cmd_bench(args: &[String]) {
    let input = &args[2];
    let data  = fs::read(input)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", input, e)));

    let runs = 20usize;
    let mut decode_ns = 0u128;
    let mut render_ns = 0u128;

    for _ in 0..runs {
        let t = Instant::now();
        let scene = if &data[0..4] == b"MSX\0" {
            msx::decode(&data).unwrap()
        } else {
            let s = String::from_utf8(data.clone()).unwrap();
            msx::parse_scene(&s).unwrap()
        };
        decode_ns += t.elapsed().as_nanos();

        let t2 = Instant::now();
        let _ = msx::render(&scene);
        render_ns += t2.elapsed().as_nanos();
    }

    let avg_decode = decode_ns as f64 / runs as f64 / 1_000_000.0;
    let avg_render = render_ns as f64 / runs as f64 / 1_000_000.0;

    println!("bench ({} runs): {} ", runs, input);
    println!("  decode/parse: {:.3}ms avg", avg_decode);
    println!("  render:       {:.3}ms avg", avg_render);
    println!("  total:        {:.3}ms avg", avg_decode + avg_render);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn default_output(input: &str, ext: &str) -> String {
    Path::new(input)
        .with_extension(ext)
        .to_string_lossy()
        .to_string()
}

fn die(msg: &str) -> ! {
    eprintln!("error: {}", msg);
    process::exit(1);
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  msx render    <input.msx> [-o output.svg] [--no-compress]");
    eprintln!("  msx compile   <input.msx> [-o output.msx] [--no-compress]");
    eprintln!("  msx info      <file.msx>");
    eprintln!("  msx validate  <source.msx>");
    eprintln!("  msx roundtrip <source.msx>");
    eprintln!("  msx bench     <file.msx>");
}
