#!/usr/bin/env python3
"""build_home.py — the landing page."""

from templates import page


def build_home_page(stats: dict) -> str:
    """`stats` — whatever the caller has on hand from the same build
    (test pass count, example count); every key is optional, each stat
    is only rendered if present, so this degrades gracefully when called
    from a context (e.g. a docs-only rebuild) that doesn't have fresh CI
    numbers to hand."""
    stat_row = ""
    entries = [
        ("tests_passed", "tests passing"),
        ("example_count", "gallery examples"),
        ("element_tags", "element/def tags"),
    ]
    chips = [f'<div class="stat"><div class="n">{stats[k]}</div><div class="l">{label}</div></div>'
             for k, label in entries if stats.get(k) is not None]
    if chips:
        stat_row = f'<div class="stat-row">{"".join(chips)}</div>'

    body = f'''<div class="hero">
  <h1>M<span style="color:var(--accent)">S</span>X</h1>
  <p class="lede">A vector graphics format with a DixScript source layer and a compact binary interchange layer — three independent renderers (SVG, CPU, GPU), a keyframe animation system, and image/audio embedding.</p>
  <div class="actions">
    <a class="btn btn-primary" href="/playground/">Try the Playground</a>
    <a class="btn btn-ghost" href="/docs/">Read the Docs</a>
    <a class="btn btn-ghost" href="/samples/">Browse Samples</a>
  </div>
  {stat_row}
</div>

<h2>Why MSX</h2>
<div class="card-grid">
  <div class="card"><h3>DixScript source</h3><p>Scenes are authored as DixScript — parametric, QuickFunc-driven, and readable — not hand-written XML.</p></div>
  <div class="card"><h3>Compact binary</h3><p>Compiles to a typed binary stream, optionally MBFA-compressed, built for tool-to-tool transfer.</p></div>
  <div class="card"><h3>Three renderers</h3><p>SVG export for the browser, a pure-CPU rasterizer (tiny-skia), and a GPU path (wgpu) that executes real WGSL shader fills.</p></div>
  <div class="card"><h3>Real animation</h3><p>Keyframe tracks with per-property easing, resolved the same way whether you're exporting a GIF or scrubbing live in the viewer.</p></div>
</div>

<h2>Get started</h2>
<pre class="code lang-bash"><code># Requires ../mbfa and ../DixScript-Rust as sibling directories
cargo build --release

# Compile MSX source to binary
cargo run --release -- compile examples/basic_shapes.msx -o out.msx

# Or just try it in the browser — no toolchain needed
</code></pre>
<p><a href="/playground/">Open the playground →</a></p>'''

    return page(title="MSX", active="home", body=body, wide=False)


if __name__ == "__main__":
    print(build_home_page({"tests_passed": 350, "example_count": 26, "element_tags": 19}))
