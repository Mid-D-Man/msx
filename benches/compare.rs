// benches/compare.rs
// DixScript evaluation happens ONCE at startup via lazy_static.
// Criterion only measures: compile, decode, render, SVG-build.

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};
use msx::{compile, decode, parse_scene, render, Scene};
use std::sync::OnceLock;
use std::time::Duration;

// ── Shared pre-built fixtures ─────────────────────────────────────────────────
// DixScript is called ONCE here. All benchmarks reuse these.

static SCENE_10: OnceLock<Scene>    = OnceLock::new();
static SCENE_BADGES: OnceLock<Scene> = OnceLock::new();
static SCENE_50: OnceLock<Scene>    = OnceLock::new();
static SCENE_200: OnceLock<Scene>   = OnceLock::new();

fn scene_10() -> &'static Scene {
    SCENE_10.get_or_init(|| parse_scene(MSX_CIRCLES_10).expect("scene_10"))
}
fn scene_badges() -> &'static Scene {
    SCENE_BADGES.get_or_init(|| parse_scene(MSX_BADGES).expect("scene_badges"))
}
fn scene_n(n: usize) -> Scene {
    parse_scene(&gen_msx_circles(n)).expect("scene_n")
}

// ── MSX source constants ──────────────────────────────────────────────────────

const MSX_CIRCLES_10: &str = r#"
@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~dot<object>(cx, cy, r, color) {
    return { type = "circle", cx = cx, cy = cy, r = r,
             style = { fill = color, stroke = "none", stroke_width = 0, opacity = 0.9 } }
  }
)
@DATA(
  scene = { width = 600, height = 400, background = #1a1a2e }
  elements::
    dot(60,  200, 40, #e94560)  dot(140, 200, 35, #533483)
    dot(220, 200, 45, #0f3460)  dot(300, 200, 30, #4a9eff)
    dot(380, 200, 50, #22c55e)  dot(460, 200, 25, #f5a623)
    dot(520, 200, 40, #a78bfa)  dot(100, 100, 30, #ef4444)
    dot(300, 100, 45, #3b82f6)  dot(500, 100, 35, #10b981)
)
"#;

const MSX_BADGES: &str = r#"
@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~badge<object>(x, y, label, color) {
    return {
      type = "group"
      elements = [
        { type = "rect", x = x, y = y, width = 90, height = 30, rx = 15,
          style = { fill = color, stroke = "none", stroke_width = 0, opacity = 1.0 } }
        { type = "text", x = x + 45, y = y + 20, content = label,
          style = { fill = #ffffff, font_size = 12, text_anchor = "middle",
                    font_weight = "bold", stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }
)
@DATA(
  scene = { width = 700, height = 200, background = #f4f5f7 }
  elements::
    badge(20,  80, "primary", #007bff)
    badge(130, 80, "success", #28a745)
    badge(240, 80, "warning", #ffc107)
    badge(350, 80, "danger",  #dc3545)
    badge(460, 80, "info",    #17a2b8)
    badge(570, 80, "dark",    #343a40)
)
"#;

fn gen_msx_circles(n: usize) -> String {
    let colors = ["#e94560", "#533483", "#0f3460", "#4a9eff", "#22c55e"];
    let mut s = String::from(
r#"@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~dot<object>(cx, cy, r, color) {
    return { type = "circle", cx = cx, cy = cy, r = r,
             style = { fill = color, stroke = "none", stroke_width = 0, opacity = 0.9 } }
  }
)
@DATA(
  scene = { width = 1000, height = 1000, background = #ffffff }
  elements::
"#);
    for i in 0..n {
        let x = (i % 20) * 50 + 25;
        let y = (i / 20) * 50 + 25;
        let r = 15 + (i % 5) * 3;
        let c = colors[i % colors.len()];
        s.push_str(&format!("  dot({}, {}, {}, {})\n", x, y, r, c));
    }
    s.push(')');
    s
}

// ── SVG string builders (no MSX at all — pure baseline) ──────────────────────

fn svg_circles(n: usize) -> String {
    let colors = ["#e94560", "#533483", "#0f3460", "#4a9eff", "#22c55e"];
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000" viewBox="0 0 1000 1000">
<rect width="1000" height="1000" fill="#ffffff"/>
"#
    );
    for i in 0..n {
        let x = (i % 20) * 50 + 25;
        let y = (i / 20) * 50 + 25;
        let r = 15 + (i % 5) * 3;
        let c = colors[i % colors.len()];
        s.push_str(&format!(
            r#"<circle cx="{}" cy="{}" r="{}" fill="{}" stroke="none" stroke-width="0" opacity="0.9"/>
"#, x, y, r, c
        ));
    }
    s.push_str("</svg>");
    s
}

fn svg_badges() -> String {
    let badges = [
        (20i32,  "primary", "#007bff"), (130, "success", "#28a745"),
        (240, "warning", "#ffc107"),    (350, "danger",  "#dc3545"),
        (460, "info",    "#17a2b8"),    (570, "dark",    "#343a40"),
    ];
    let mut s = String::from(
r#"<svg xmlns="http://www.w3.org/2000/svg" width="700" height="200" viewBox="0 0 700 200">
<rect width="700" height="200" fill="#f4f5f7"/>
"#
    );
    for (x, label, color) in &badges {
        s.push_str(&format!(
            r#"<g><rect x="{}" y="80" width="90" height="30" rx="15" fill="{}" stroke="none" stroke-width="0" opacity="1"/>
<text x="{}" y="100" fill="#ffffff" font-size="12" text-anchor="middle" font-weight="bold">{}</text></g>
"#, x, color, x + 45, label
        ));
    }
    s.push_str("</svg>");
    s
}

fn svg_circles_10() -> String { svg_circles(10) }

// ── Pre-compiled binaries ─────────────────────────────────────────────────────

static BIN_10_MBFA: OnceLock<Vec<u8>>  = OnceLock::new();
static BIN_10_RAW:  OnceLock<Vec<u8>>  = OnceLock::new();
static BIN_BADGES:  OnceLock<Vec<u8>>  = OnceLock::new();

fn bin_10_mbfa() -> &'static [u8] {
    BIN_10_MBFA.get_or_init(|| compile(scene_10(), true).unwrap())
}
fn bin_10_raw() -> &'static [u8] {
    BIN_10_RAW.get_or_init(|| compile(scene_10(), false).unwrap())
}
fn bin_badges() -> &'static [u8] {
    BIN_BADGES.get_or_init(|| compile(scene_badges(), true).unwrap())
}

// ── 1. compile: Scene → binary ───────────────────────────────────────────────

fn bench_compile(c: &mut Criterion) {
    let mut g = c.benchmark_group("compile");
    g.measurement_time(Duration::from_secs(5));
    g.sample_size(50);

    g.bench_function("circles_10_mbfa", |b| {
        b.iter(|| compile(black_box(scene_10()), true).unwrap())
    });
    g.bench_function("circles_10_raw", |b| {
        b.iter(|| compile(black_box(scene_10()), false).unwrap())
    });
    g.bench_function("badges_mbfa", |b| {
        b.iter(|| compile(black_box(scene_badges()), true).unwrap())
    });

    // Size stats (printed, not timed)
    let svg_10  = render(scene_10());
    let svg_bdg = render(scene_badges());
    println!(
        "\n[size] circles_10  binary_mbfa={:5}B  binary_raw={:5}B  svg={:5}B  ratio={:.1}%",
        bin_10_mbfa().len(), bin_10_raw().len(), svg_10.len(),
        bin_10_mbfa().len() as f64 / svg_10.len() as f64 * 100.0,
    );
    println!(
        "[size] badges      binary_mbfa={:5}B  svg={:5}B  ratio={:.1}%",
        bin_badges().len(), svg_bdg.len(),
        bin_badges().len() as f64 / svg_bdg.len() as f64 * 100.0,
    );

    g.finish();
}

// ── 2. decode: binary → Scene ─────────────────────────────────────────────────

fn bench_decode(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode");
    g.measurement_time(Duration::from_secs(5));
    g.sample_size(50);

    g.bench_function("circles_10_mbfa", |b| {
        b.iter(|| decode(black_box(bin_10_mbfa())).unwrap())
    });
    g.bench_function("circles_10_raw", |b| {
        b.iter(|| decode(black_box(bin_10_raw())).unwrap())
    });
    g.bench_function("badges_mbfa", |b| {
        b.iter(|| decode(black_box(bin_badges())).unwrap())
    });

    g.finish();
}

// ── 3. render: Scene → SVG string ────────────────────────────────────────────

fn bench_render(c: &mut Criterion) {
    let mut g = c.benchmark_group("render");
    g.measurement_time(Duration::from_secs(5));
    g.sample_size(50);

    // MSX render
    g.bench_function("msx_circles_10", |b| {
        b.iter(|| render(black_box(scene_10())))
    });
    g.bench_function("msx_badges", |b| {
        b.iter(|| render(black_box(scene_badges())))
    });

    // SVG string build baseline — same data, no MSX involved
    g.bench_function("svg_build_circles_10", |b| {
        b.iter(|| svg_circles_10())
    });
    g.bench_function("svg_build_badges", |b| {
        b.iter(|| svg_badges())
    });

    g.finish();
}

// ── 4. decode+render: binary → SVG (hot serving path) ────────────────────────

fn bench_decode_render(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_render");
    g.measurement_time(Duration::from_secs(5));
    g.sample_size(50);

    g.bench_function("circles_10_mbfa", |b| {
        b.iter(|| {
            let scene = decode(black_box(bin_10_mbfa())).unwrap();
            render(&scene)
        })
    });
    g.bench_function("badges_mbfa", |b| {
        b.iter(|| {
            let scene = decode(black_box(bin_badges())).unwrap();
            render(&scene)
        })
    });

    // Baseline: SVG string build (no binary decode needed)
    g.bench_function("svg_build_circles_10_baseline", |b| {
        b.iter(|| svg_circles_10())
    });
    g.bench_function("svg_build_badges_baseline", |b| {
        b.iter(|| svg_badges())
    });

    g.finish();
}

// ── 5. scale: n=10/50/100/200 — decode+render vs SVG build ───────────────────

fn bench_scale(c: &mut Criterion) {
    let mut g = c.benchmark_group("scale");
    g.measurement_time(Duration::from_secs(5));
    g.sample_size(20);

    // Pre-build scenes at startup (DixScript called once each)
    let scenes: Vec<(usize, Vec<u8>)> = [10usize, 50, 100, 200].iter().map(|&n| {
        let s = scene_n(n);
        let b = compile(&s, true).unwrap();
        (n, b)
    }).collect();

    for (n, bin) in &scenes {
        let svg = svg_circles(*n);

        g.throughput(Throughput::Elements(*n as u64));

        // MSX decode+render
        let bin_ref = bin.clone();
        g.bench_with_input(
            BenchmarkId::new("msx_decode_render", n),
            n,
            |b, _| {
                b.iter(|| {
                    let scene = decode(black_box(&bin_ref)).unwrap();
                    render(&scene)
                })
            },
        );

        // SVG string build baseline
        let n_cap = *n;
        g.bench_with_input(
            BenchmarkId::new("svg_build", n),
            n,
            |b, _| b.iter(|| svg_circles(black_box(n_cap))),
        );

        // Size comparison
        println!(
            "[scale n={:3}]  msx_binary={:5}B  svg={:5}B  ratio={:.1}%  savings={}B",
            n, bin.len(), svg.len(),
            bin.len() as f64 / svg.len() as f64 * 100.0,
            svg.len().saturating_sub(bin.len()),
        );
    }

    g.finish();
}

criterion_group!(
    benches,
    bench_compile,
    bench_decode,
    bench_render,
    bench_decode_render,
    bench_scale,
);
criterion_main!(benches);
