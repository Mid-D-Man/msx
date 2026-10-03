#!/usr/bin/env python3
"""
build_samples.py
Renders the samples gallery from examples.json — the exact schema
pages.yml's own "Build examples.json" step already produces (name,
source, svg, png_base64, anim_gif_base64, gpu_png_base64, gpu_error,
gpu_gif_base64, gpu_gif_error, uses_shader, source_bytes, binary_bytes,
svg_bytes, bin_pct, svg_pct, pass) — this file doesn't change that
pipeline at all, only what consumes its output. Falls back to the
inline SVG when no native PNG exists for an example (mirrors
generate_report.py's own existing fallback for the same reason: a
rasterize failure is non-fatal to the corpus step).
"""

import html
import json

from templates import page


def _gpu_fell_back(ex: dict) -> bool:
    """True when msx-render-gpu printed its graceful-fallback diagnostic.

    A shader def whose WGSL could not run is painted flat with
    `fallback_color` and the process still exits 0, so the only trace is
    the stderr line the corpus step now keeps (results/gpu_errors/*.txt,
    surfaced here as `gpu_error` / `gpu_gif_error`).
    """
    return any("falling back" in (ex.get(k) or "") for k in ("gpu_error", "gpu_gif_error"))


def _shown_backend(ex: dict) -> str:
    """Which render `_preview` draws: 'gpu', 'cpu' or 'svg'.

    Only msx-render-gpu executes a `Def::Shader` fill; the CPU raster and
    the SVG export both paint the def's flat `fallback_color`. A shader
    example therefore shows its GPU render whenever one exists (GIF first,
    same as the CPU order below), everything else keeps the CPU raster.
    """
    if ex.get("uses_shader") and (ex.get("gpu_gif_base64") or ex.get("gpu_png_base64")):
        return "gpu"
    if ex.get("anim_gif_base64") or ex.get("png_base64"):
        return "cpu"
    return "svg"


def _preview(ex: dict) -> str:
    if _shown_backend(ex) == "gpu":
        if ex.get("gpu_gif_base64"):
            return f'<img src="data:image/gif;base64,{ex["gpu_gif_base64"]}" alt="{html.escape(ex["name"])} (GPU, animated)">'
        return f'<img src="data:image/png;base64,{ex["gpu_png_base64"]}" alt="{html.escape(ex["name"])} (GPU)">'
    if ex.get("anim_gif_base64"):
        return f'<img src="data:image/gif;base64,{ex["anim_gif_base64"]}" alt="{html.escape(ex["name"])} (animated)">'
    if ex.get("png_base64"):
        return f'<img src="data:image/png;base64,{ex["png_base64"]}" alt="{html.escape(ex["name"])}">'
    # Last-resort fallback: the raw SVG, inlined directly (no native
    # raster exists for this one — see corpus.csv's own WARN for why,
    # surfaced nowhere on this card on purpose; a missing PNG isn't this
    # page's failure to explain, msx-cli's own build log already has it).
    return ex.get("svg", "")


def _badges(ex: dict) -> str:
    badges = []
    if ex.get("pass"):
        badges.append('<span class="badge accent">roundtrip OK</span>')
    else:
        badges.append('<span class="badge" style="color:var(--bad)">roundtrip FAIL</span>')
    if ex.get("uses_shader"):
        badges.append('<span class="badge">shader</span>')
    if ex.get("anim_gif_base64") or ex.get("gpu_gif_base64"):
        badges.append('<span class="badge">animated</span>')
    backend = _shown_backend(ex)
    if backend == "gpu" and _gpu_fell_back(ex):
        # The image shown is the GPU output, but msx-render-gpu said it
        # painted the flat fallback_color instead of running the shader.
        badges.append('<span class="badge" style="color:var(--bad)">GPU fell back to flat color</span>')
    elif backend == "gpu":
        badges.append('<span class="badge accent">GPU render</span>')
    elif backend == "cpu":
        label = "CPU render"
        if ex.get("uses_shader"):
            # No GPU image to show for this shader example: the CPU raster
            # paints the def's flat fallback_color, which is expected here.
            label = "CPU render (flat shader fallback)"
        badges.append(f'<span class="badge">{label}</span>')
    if ex.get("bin_pct"):
        badges.append(f'<span class="badge">{ex["bin_pct"]:.0f}% of source</span>')
    return '<div class="badges">' + "".join(badges) + "</div>"


def _gpu_note(ex: dict) -> str:
    """Visible (tap-to-open, no hover needed) GPU stderr for this example."""
    parts = []
    for key, label in (("gpu_error", "rasterize-gpu"), ("gpu_gif_error", "animate-gpu")):
        text = (ex.get(key) or "").strip()
        if text:
            parts.append(f"[{label}]\n{text}")
    if not parts:
        return ""
    body = html.escape("\n\n".join(parts))
    return (
        '<details class="gpu-note" style="margin-top:8px">'
        '<summary style="cursor:pointer;font-size:11px;color:var(--muted)">GPU stderr</summary>'
        '<pre style="white-space:pre-wrap;word-break:break-word;font-size:11px;margin:6px 0 0;'
        f'color:var(--muted)">{body}</pre></details>'
    )


def build_samples_page(examples: list) -> str:
    cards = []
    for ex in examples:
        cards.append(f'''<div class="sample-card">
  <div class="sample-preview">{_preview(ex)}</div>
  <div class="sample-meta">
    <div class="name">{html.escape(ex["name"])}</div>
    {_badges(ex)}
    {_gpu_note(ex)}
  </div>
</div>''')

    body = f'''<h1>Samples</h1>
<p class="lede">Every example in <code>examples/</code>, rendered fresh on each build — native raster output where available, animated where the scene has a timeline, and cross-checked against a binary round-trip.</p>
<div class="card-grid">
{"".join(cards) if cards else "<p>No examples available in this build.</p>"}
</div>'''

    return page(title="Samples", active="samples", body=body, wide=True)


if __name__ == "__main__":
    import sys
    examples = json.load(open(sys.argv[1])) if len(sys.argv) > 1 else []
    print(build_samples_page(examples))
