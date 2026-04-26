// benches/compare.rs
//! Criterion benchmarks: MSX parse/compile/render vs equivalent SVG.

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};
use msx::{compile, decode, parse_scene, render};
use std::time::Duration;

// ── Test scenes ───────────────────────────────────────────────────────────────

const CIRCLES_10_MSX: &str = r##"
@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~dot<object>(cx, cy, r, color) {
    return { type = "circle", cx = cx, cy = cy, r = r,
             style = { fill = color, stroke = "none", stroke_width = 0, opacity = 0.9 } }
  }
)
@DATA(
  scene: { width = 600, height = 400, background = "#1a1a2e" }
  elements::
    dot(60,  200, 40, "#e94560")  dot(140, 200, 35, "#533483")
    dot(220, 200, 45, "#0f3460")  dot(300, 200, 30, "#4a9eff")
    dot(380, 200, 50, "#22c55e")  dot(460, 200, 25, "#f5a623")
    dot(520, 200, 40, "#a78bfa")  dot(100, 100, 30, "#ef4444")
    dot(300, 100, 45, "#3b82f6")  dot(500, 100, 35, "#10b981")
)
"##;

fn circles_10_svg() -> String {
    let circles = [
        (60,  200, 40, "#e94560"), (140, 200, 35, "#533483"),
        (220, 200, 45, "#0f3460"), (300, 200, 30, "#4a9eff"),
        (380, 200, 50, "#22c55e"), (460, 200, 25, "#f5a623"),
        (520, 200, 40, "#a78bfa"), (100, 100, 30, "#ef4444"),
        (300, 100, 45, "#3b82f6"), (500, 100, 35, "#10b981"),
    ];

    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"600\" height=\"400\" \
         viewBox=\"0 0 600 400\">\n\
         <rect width=\"600\" height=\"400\" fill=\"#1a1a2e\"/>\n"
    );
    for (cx, cy, r, color) in &circles {
        svg.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" \
             stroke=\"none\" stroke-width=\"0\" opacity=\"0.9\"/>\n",
            cx, cy, r, color
        ));
    }
    svg.push_str("</svg>");
    svg
}

const BADGES_MSX: &str = r##"
@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~badge<object>(x, y, label, color) {
    return {
      type = "group"
      elements = [
        { type = "rect", x = x, y = y, width = 90, height = 30, rx = 15,
          style = { fill = color, stroke = "none", stroke_width = 0, opacity = 1.0 } }
        { type = "text", x = x + 45, y = y + 20, content = label,
          style = { fill = "#ffffff", font_size = 12, text_anchor = "middle",
                    font_weight = "bold", stroke = "none", stroke_width = 0, opacity = 1.0 } }
      ]
    }
  }
)
@DATA(
  scene: { width = 700, height = 200, background = "#f4f5f7" }
  elements::
    badge(20,  80, "primary", "#007bff")
    badge(130, 80, "success", "#28a745")
    badge(240, 80, "warning", "#ffc107")
    badge(350, 80, "danger",  "#dc3545")
    badge(460, 80, "info",    "#17a2b8")
    badge(570, 80, "dark",    "#343a40")
)
"##;

fn badges_svg() -> String {
    let badges = [
        (20,  "primary", "#007bff"),
        (130, "success", "#28a745"),
        (240, "warning", "#ffc107"),
        (350, "danger",  "#dc3545"),
        (460, "info",    "#17a2b8"),
        (570, "dark",    "#343a40"),
    ];
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"700\" height=\"200\" \
         viewBox=\"0 0 700 200\">\n\
         <rect width=\"700\" height=\"200\" fill=\"#f4f5f7\"/>\n"
    );
    for (x, label, color) in &badges {
        let y = 80;
        svg.push_str(&format!(
            "<g>\n  \
             <rect x=\"{}\" y=\"{}\" width=\"90\" height=\"30\" rx=\"15\" \
             fill=\"{}\" stroke=\"none\" stroke-width=\"0\" opacity=\"1\"/>\n  \
             <text x=\"{}\" y=\"{}\" fill=\"#ffffff\" font-size=\"12\" \
             text-anchor=\"middle\" font-weight=\"bold\" stroke=\"none\" \
             stroke-width=\"0\" opacity=\"1\">{}</text>\n\
             </g>\n",
            x, y, color,
            x + 45, y + 20,
            label,
        ));
    }
    svg.push_str("</svg>");
    svg
}

fn gen_many_circles_msx(n: usize) -> String {
    let colors = ["#e94560", "#533483", "#0f3460", "#4a9eff", "#22c55e"];
    let mut s = String::from(
        r##"@CONFIG( version -> "1.0.0" )
@QUICKFUNCS(
  ~dot<object>(cx, cy, r, color) {
    return { type = "circle", cx = cx, cy = cy, r = r,
             style = { fill = color, stroke = "none", stroke_width = 0, opacity = 0.9 } }
  }
)
@DATA(
  scene: { width = 1000, height = 1000, background = "#ffffff" }
  elements::
"##);
    for i in 0..n {
        let x = (i % 20) * 50 + 25;
        let y = (i / 20) * 50 + 25;
        let r = 15 + (i % 5) * 3;
        let c = colors[i % colors.len()];
        s.push_str(&format!("    dot({}, {}, {}, \"{}\")\n", x, y, r, c));
    }
    s.push(')');
    s
}

fn gen_many_circles_svg(n: usize) -> String {
    let colors = ["#e94560", "#533483", "#0f3460", "#4a9eff", "#22c55e"];
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\" \
         viewBox=\"0 0 1000 1000\">\n\
         <rect width=\"1000\" height=\"1000\" fill=\"#ffffff\"/>\n"
    );
    for i in 0..n {
        let x = (i % 20) * 50 + 25;
        let y = (i / 20) * 50 + 25;
        let r = 15 + (i % 5) * 3;
        let c = colors[i % colors.len()];
        svg.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" \
             stroke=\"none\" stroke-width=\"0\" opacity=\"0.9\"/>\n",
            x, y, r, c
        ));
    }
    svg.push_str("</svg>");
    svg
}

// ── Benchmark groups ──────────────────────────────────────────────────────────

fn bench_parse_and_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_render");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);

    group.bench_function("msx_source_to_svg_10_circles", |b| {
        b.iter(|| {
            let scene = parse_scene(black_box(CIRCLES_10_MSX)).unwrap();
            render(&scene)
        })
    });

    group.bench_function("svg_build_10_circles", |b| {
        b.iter(|| circles_10_svg())
    });

    group.bench_function("msx_source_to_svg_badges", |b| {
        b.iter(|| {
            let scene = parse_scene(black_box(BADGES_MSX)).unwrap();
            render(&scene)
        })
    });

    group.bench_function("svg_build_badges", |b| {
        b.iter(|| badges_svg())
    });

    group.finish();
}

fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);

    let scene_10 = parse_scene(CIRCLES_10_MSX).unwrap();

    group.throughput(Throughput::Elements(10));
    group.bench_function("msx_compile_10_circles", |b| {
        b.iter(|| compile(black_box(&scene_10), true).unwrap())
    });

    group.bench_function("msx_compile_10_circles_no_compress", |b| {
        b.iter(|| compile(black_box(&scene_10), false).unwrap())
    });

    let scene_badges = parse_scene(BADGES_MSX).unwrap();
    group.bench_function("msx_compile_badges", |b| {
        b.iter(|| compile(black_box(&scene_badges), true).unwrap())
    });

    group.finish();
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);

    let scene_10  = parse_scene(CIRCLES_10_MSX).unwrap();
    let binary_10 = compile(&scene_10, true).unwrap();
    let binary_10_raw = compile(&scene_10, false).unwrap();

    group.bench_function("msx_decode_10_circles_mbfa", |b| {
        b.iter(|| decode(black_box(&binary_10)).unwrap())
    });

    group.bench_function("msx_decode_10_circles_raw", |b| {
        b.iter(|| decode(black_box(&binary_10_raw)).unwrap())
    });

    let scene_badges  = parse_scene(BADGES_MSX).unwrap();
    let binary_badges = compile(&scene_badges, true).unwrap();

    group.bench_function("msx_decode_badges_mbfa", |b| {
        b.iter(|| decode(black_box(&binary_badges)).unwrap())
    });

    group.finish();
}

fn bench_render_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("render_only");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);

    let scene_10    = parse_scene(CIRCLES_10_MSX).unwrap();
    let binary_10   = compile(&scene_10, false).unwrap();
    let decoded_10  = decode(&binary_10).unwrap();

    group.bench_function("msx_render_10_circles", |b| {
        b.iter(|| render(black_box(&decoded_10)))
    });

    let scene_badges   = parse_scene(BADGES_MSX).unwrap();
    let binary_badges  = compile(&scene_badges, false).unwrap();
    let decoded_badges = decode(&binary_badges).unwrap();

    group.bench_function("msx_render_badges", |b| {
        b.iter(|| render(black_box(&decoded_badges)))
    });

    group.finish();
}

fn bench_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("scale_vs_svg");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(10);

    for &n in &[10usize, 50, 100, 200] {
        let msx_src = gen_many_circles_msx(n);
        let svg_src = gen_many_circles_svg(n);

        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(
            BenchmarkId::new("msx_full_pipeline_circles", n),
            &n,
            |b, _| {
                b.iter(|| {
                    let scene  = parse_scene(black_box(&msx_src)).unwrap();
                    let binary = compile(&scene, true).unwrap();
                    let scene2 = decode(&binary).unwrap();
                    render(&scene2)
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("svg_build_circles", n),
            &n,
            |b, &count| {
                b.iter(|| gen_many_circles_svg(black_box(count)))
            },
        );

        let scene  = parse_scene(&msx_src).unwrap();
        let binary = compile(&scene, true).unwrap();
        let svg    = gen_many_circles_svg(n);
        println!(
            "[scale n={}] msx_binary={}B svg={}B ratio={:.1}%  elements={}",
            n, binary.len(), svg.len(),
            binary.len() as f64 / svg.len() as f64 * 100.0,
            scene.element_count(),
        );
    }

    group.finish();
}

fn bench_binary_vs_svg_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("binary_vs_svg_size");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    let cases: &[(&str, &str)] = &[
        ("10_circles",  CIRCLES_10_MSX),
        ("badges",      BADGES_MSX),
    ];

    for (label, src) in cases {
        let scene      = parse_scene(src).unwrap();
        let binary_c   = compile(&scene, true).unwrap();
        let binary_raw = compile(&scene, false).unwrap();
        let svg        = render(&scene);

        println!(
            "[{}] source={}B  binary_mbfa={}B ({:.1}% of svg)  \
             binary_raw={}B ({:.1}% of svg)  svg={}B",
            label,
            src.len(),
            binary_c.len(),   binary_c.len()   as f64 / svg.len() as f64 * 100.0,
            binary_raw.len(), binary_raw.len() as f64 / svg.len() as f64 * 100.0,
            svg.len(),
        );

        group.bench_function(
            BenchmarkId::new("noop_size_check", label),
            |b| b.iter(|| binary_c.len() + svg.len()),
        );
    }

    for &n in &[50usize, 200] {
        let msx_src    = gen_many_circles_msx(n);
        let svg_src    = gen_many_circles_svg(n);
        let scene      = parse_scene(&msx_src).unwrap();
        let binary_c   = compile(&scene, true).unwrap();
        let binary_raw = compile(&scene, false).unwrap();

        println!(
            "[{}_circles] source={}B  binary_mbfa={}B ({:.1}%)  \
             binary_raw={}B ({:.1}%)  svg={}B  savings_vs_svg={}B",
            n,
            msx_src.len(),
            binary_c.len(),   binary_c.len()   as f64 / svg_src.len() as f64 * 100.0,
            binary_raw.len(), binary_raw.len() as f64 / svg_src.len() as f64 * 100.0,
            svg_src.len(),
            svg_src.len().saturating_sub(binary_c.len()),
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_and_render,
    bench_compile,
    bench_decode,
    bench_render_only,
    bench_scale,
    bench_binary_vs_svg_size,
);
criterion_main!(benches);
