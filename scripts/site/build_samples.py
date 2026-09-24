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


def _preview(ex: dict) -> str:
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
    if ex.get("anim_gif_base64"):
        badges.append('<span class="badge">animated</span>')
    if ex.get("gpu_png_base64"):
        badges.append('<span class="badge">gpu</span>')
    if ex.get("bin_pct"):
        badges.append(f'<span class="badge">{ex["bin_pct"]:.0f}% of source</span>')
    return '<div class="badges">' + "".join(badges) + "</div>"


def build_samples_page(examples: list) -> str:
    cards = []
    for ex in examples:
        cards.append(f'''<div class="sample-card">
  <div class="sample-preview">{_preview(ex)}</div>
  <div class="sample-meta">
    <div class="name">{html.escape(ex["name"])}</div>
    {_badges(ex)}
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
