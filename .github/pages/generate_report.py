#!/usr/bin/env python3
"""
generate_report.py
Parses MSX CI output and generates a self-contained HTML report.
"""

import argparse
import json
import os
import re
from datetime import datetime, timezone
from typing import Optional


# ── Argument parsing ──────────────────────────────────────────────────────────

def parse_args():
    p = argparse.ArgumentParser()
    p.add_argument("--build",    required=True)
    p.add_argument("--commit",   required=True)
    p.add_argument("--branch",   required=True)
    p.add_argument("--tests-d",  required=True, dest="tests_d")
    p.add_argument("--tests-r",  required=True, dest="tests_r")
    p.add_argument("--bench",    required=True)
    p.add_argument("--corpus",   required=True)
    p.add_argument("--examples", required=False, default=None)
    p.add_argument("--out",      required=True)
    return p.parse_args()


# ── ANSI stripper ─────────────────────────────────────────────────────────────

_RE_ANSI = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')

def _strip(s: str) -> str:
    return _RE_ANSI.sub('', s)


# ── Parsers ───────────────────────────────────────────────────────────────────

def parse_tests(path: str) -> dict:
    tests  = []
    passed = 0
    failed = 0
    RE_SUMMARY = re.compile(r'test result:.*?(\d+)\s+passed[^;]*;\s*(\d+)\s+failed')
    RE_LINE    = re.compile(r'^test (.+?) \.\.\. (ok|FAILED|ignored)\s*$')
    try:
        with open(path, errors='replace') as f:
            for raw in f:
                line = _strip(raw).rstrip()
                m = RE_SUMMARY.search(line)
                if m:
                    passed = int(m.group(1))
                    failed = int(m.group(2))
                    continue
                m = RE_LINE.match(line)
                if m:
                    tests.append({"name": m.group(1).strip(), "status": m.group(2)})
    except FileNotFoundError:
        pass
    return {"passed": passed, "failed": failed, "tests": tests}


def parse_bench(path: str) -> list:
    results = []
    try:
        with open(path, errors='replace') as f:
            lines = f.readlines()
    except FileNotFoundError:
        return results
    RE_FULL  = re.compile(r'^test (.+?) \.\.\. bench:\s+([\d,]+) ns/iter \(\+/- ([\d,]+)\)')
    RE_TEST  = re.compile(r'^test (.+?) \.\.\.')
    RE_BENCH = re.compile(r'^\s*bench:\s+([\d,]+) ns/iter \(\+/- ([\d,]+)\)')
    pending  = None
    for raw in lines:
        line = _strip(raw).rstrip()
        m = RE_FULL.match(line)
        if m:
            results.append({"name": m.group(1).strip(), "ns": int(m.group(2).replace(",", "")), "var": int(m.group(3).replace(",", ""))})
            pending = None
            continue
        m = RE_TEST.match(line)
        if m:
            pending = m.group(1).strip()
            continue
        m = RE_BENCH.match(line)
        if m:
            results.append({"name": pending or f"bench_{len(results)+1}", "ns": int(m.group(1).replace(",", "")), "var": int(m.group(2).replace(",", ""))})
            pending = None
    return results


def parse_corpus(path: str) -> list:
    rows = []
    try:
        with open(path, errors='replace') as f:
            lines = f.readlines()
        for line in lines[1:]:
            line = line.strip()
            if not line:
                continue
            parts = line.split(",")
            if len(parts) < 6:
                continue
            rows.append({
                "name":         parts[0].strip(),
                "source_bytes": int(parts[1].strip()),
                "binary_bytes": int(parts[2].strip()),
                "svg_bytes":    int(parts[3].strip()),
                "bin_pct":      float(parts[4].strip()),
                "svg_pct":      float(parts[5].strip()),
                "pass":         len(parts) > 6 and parts[6].strip() == "PASS",
            })
    except (FileNotFoundError, ValueError):
        pass
    return rows


def parse_examples(path: Optional[str]) -> list:
    if not path:
        return []
    try:
        with open(path, errors='replace') as f:
            data = json.load(f)
        return data if isinstance(data, list) else []
    except (FileNotFoundError, json.JSONDecodeError):
        return []


# ── SVG chart builders ────────────────────────────────────────────────────────

def svg_test_donut(passed: int, failed: int) -> str:
    import math
    total = passed + failed
    if total == 0:
        return "<p class='no-data'>No test data.</p>"
    r_out, r_in = 48, 30
    cx = cy = 60
    size = 120

    def arc(start_deg, end_deg, ro, ri):
        def pt(deg, r):
            rad = math.radians(deg - 90)
            return cx + r * math.cos(rad), cy + r * math.sin(rad)
        x1,y1 = pt(start_deg, ro); x2,y2 = pt(end_deg, ro)
        x3,y3 = pt(end_deg, ri);   x4,y4 = pt(start_deg, ri)
        lg = 1 if (end_deg - start_deg) > 180 else 0
        return (f"M {x1:.2f} {y1:.2f} A {ro} {ro} 0 {lg} 1 {x2:.2f} {y2:.2f} "
                f"L {x3:.2f} {y3:.2f} A {ri} {ri} 0 {lg} 0 {x4:.2f} {y4:.2f} Z")

    pass_deg = 360 * passed / total
    out = [f'<svg viewBox="0 0 {size} {size}" xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}">']
    if failed == 0:
        out.append(f'<path d="{arc(0, 359.99, r_out, r_in)}" fill="#22c55e"/>')
    else:
        out.append(f'<path d="{arc(0, pass_deg, r_out, r_in)}" fill="#22c55e"/>')
        out.append(f'<path d="{arc(pass_deg, 360, r_out, r_in)}" fill="#ef4444"/>')
    out.append(f'<text x="{cx}" y="{cy-4}" text-anchor="middle" font-size="14" font-weight="bold" fill="#f1f5f9">{passed}</text>')
    out.append(f'<text x="{cx}" y="{cy+12}" text-anchor="middle" font-size="9" fill="#94a3b8">passed</text>')
    out.append("</svg>")
    return "\n".join(out)


def test_rows_html(tests: list) -> str:
    if not tests:
        return "<p class='no-data'>No individual test data.</p>"
    rows = []
    for t in tests:
        icon = "✓" if t["status"] == "ok" else ("⚠" if t["status"] == "ignored" else "✗")
        cls  = "pass" if t["status"] == "ok" else ("ignore" if t["status"] == "ignored" else "fail")
        rows.append(f'<tr class="{cls}"><td class="icon">{icon}</td><td class="tname">{t["name"]}</td><td class="tstatus">{t["status"]}</td></tr>')
    return "\n".join(rows)


def svg_throughput_bars(bench: list) -> str:
    if not bench:
        return "<p class='no-data'>No benchmark data.</p>"
    max_ns  = max(b["ns"] for b in bench) or 1
    bar_h   = 22
    pad_l   = 220
    pad_r   = 80
    bar_w   = 560
    gap     = 6
    h_total = len(bench) * (bar_h + gap) + 40
    lines   = [f'<svg class="chart" viewBox="0 0 {pad_l+bar_w+pad_r} {h_total}" xmlns="http://www.w3.org/2000/svg">']
    lines.append(f'<text class="chart-title" x="{pad_l}" y="14">ns/iter — lower is better</text>')
    COLORS = ["#4a9eff","#a78bfa","#22c55e","#f5a623","#e94560","#06d6a0"]
    for i, b in enumerate(bench):
        y       = 24 + i * (bar_h + gap)
        w       = max(2, int(b["ns"] / max_ns * bar_w))
        color   = COLORS[i % len(COLORS)]
        label   = b["name"][-38:] if len(b["name"]) > 38 else b["name"]
        ns_fmt  = f'{b["ns"]:,}'
        lines.append(f'<text class="bar-label" x="{pad_l-6}" y="{y+bar_h//2+4}" text-anchor="end">{label}</text>')
        lines.append(f'<rect x="{pad_l}" y="{y}" width="{w}" height="{bar_h}" fill="{color}" rx="3" opacity="0.85"/>')
        lines.append(f'<text class="bar-val" x="{pad_l+w+6}" y="{y+bar_h//2+4}">{ns_fmt} ns</text>')
    lines.append("</svg>")
    return "\n".join(lines)


def svg_size_comparison_bars(corpus: list) -> str:
    if not corpus:
        return "<p class='no-data'>No corpus data yet.</p>"
    max_svg  = max((r["svg_bytes"] for r in corpus), default=1) or 1
    bar_h    = 16
    pad_l    = 160
    bar_w    = 500
    gap      = 20
    h_total  = len(corpus) * (bar_h * 3 + gap) + 50
    lines    = [f'<svg class="chart" viewBox="0 0 {pad_l+bar_w+120} {h_total}" xmlns="http://www.w3.org/2000/svg">']
    lines.append(f'<text class="legend" x="{pad_l}" y="16">■ Source</text>')
    lines.append(f'<text class="legend" x="{pad_l+80}" y="16">■ Binary (MBFA)</text>')
    lines.append(f'<text class="legend" x="{pad_l+200}" y="16">■ SVG export</text>')
    for i, r in enumerate(corpus):
        y_base  = 30 + i * (bar_h * 3 + gap)
        rt_sym  = "✓" if r["pass"] else "✗"
        rt_col  = "#22c55e" if r["pass"] else "#ef4444"
        lines.append(f'<text class="bar-label" x="{pad_l-6}" y="{y_base + bar_h + 4}" text-anchor="end">{r["name"]}</text>')
        lines.append(f'<text class="rt-badge" x="{pad_l+bar_w+10}" y="{y_base + bar_h + 4}" fill="{rt_col}">{rt_sym}</text>')
        for j, (key, color, label) in enumerate([
            ("source_bytes", "#4a9eff", "src"),
            ("binary_bytes", "#a78bfa", "bin"),
            ("svg_bytes",    "#22c55e", "svg"),
        ]):
            w   = max(2, int(r[key] / max_svg * bar_w))
            y   = y_base + j * bar_h
            val = r[key]
            lines.append(f'<rect x="{pad_l}" y="{y}" width="{w}" height="{bar_h-2}" fill="{color}" rx="2" opacity="0.8"/>')
            lines.append(f'<text class="bar-val" x="{pad_l+w+4}" y="{y+bar_h-4}">{val}B</text>')
    lines.append("</svg>")
    return "\n".join(lines)


def svg_gallery_html(examples: list) -> str:
    if not examples:
        return "<p class='no-data'>No example SVG data available.</p>"

    cards = []
    for ex in examples:
        name            = ex.get("name", "unknown")
        source          = ex.get("source", "")
        svg_content     = ex.get("svg", "")
        png_base64      = ex.get("png_base64", "")
        anim_gif_base64 = ex.get("anim_gif_base64", "")
        uses_shader     = ex.get("uses_shader", False)
        source_bytes    = ex.get("source_bytes", 0)
        binary_bytes    = ex.get("binary_bytes", 0)
        svg_bytes       = ex.get("svg_bytes", 0)
        passed          = ex.get("pass", False)

        msx_escaped = source.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        svg_escaped = svg_content.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

        # Pane 3 — the actual MSX render. MSX's primary output is a native
        # pixel buffer; SVG is one optional export target. This pane shows
        # what msx-render-cpu actually produced. An animated example (msx-
        # anim's resolve_at_time sampled across the timeline) takes priority
        # as a self-looping GIF — <img> loops it natively, no extra JS.
        # Falls back to the static PNG, then to inline SVG rendered by the
        # browser if neither raster exists.
        if anim_gif_base64:
            img_id = f"render-{name}"
            # Lets you flip between the looping GIF and the static
            # (unanimated, t=0) PNG for the same example without leaving
            # the card — GIF encoding forces a quantized/dithered palette,
            # so this is the fast way to tell "the render is actually
            # wrong" apart from "GIF compression is just muddying a fine
            # render" for anything that looks off in the animated version.
            compare_html = ""
            if png_base64:
                compare_html = (
                    f'<button type="button" class="compare-toggle" '
                    f'data-gif="data:image/gif;base64,{anim_gif_base64}" '
                    f'data-png="data:image/png;base64,{png_base64}" '
                    f'data-target="{img_id}" onclick="msxToggleRender(this)">'
                    f'compare static frame</button>'
                )
            rendered_visual = (
                f'<img id="{img_id}" class="native-render native-render--anim" '
                f'src="data:image/gif;base64,{anim_gif_base64}" '
                f'alt="{name} — animated native MSX render (msx-anim + msx-render-cpu)">'
                f'{compare_html}'
            )
        elif png_base64:
            rendered_visual = (
                f'<img class="native-render" '
                f'src="data:image/png;base64,{png_base64}" '
                f'alt="{name} — native MSX render (msx-render-cpu)">'
            )
        elif svg_content:
            rendered_visual = (
                '<p class="native-render-note">Native CPU raster unavailable for this build — '
                'showing the SVG export rendered by your browser instead.</p>'
                + svg_content
            )
        else:
            rendered_visual = "<p style='color:#94a3b8;font-size:0.8rem;text-align:center'>No render available</p>"

        bin_pct  = (binary_bytes / max(svg_bytes, 1)) * 100 if svg_bytes > 0 else 0
        rt_badge = ('<span class="stat-chip green">✓ roundtrip</span>' if passed
                    else '<span class="stat-chip red">✗ roundtrip failed</span>')
        anim_badge = '<span class="stat-chip accent">▶ animated</span>' if anim_gif_base64 else ''
        shader_badge = (
            '<span class="stat-chip accent" '
            'title="Uses a Def::Shader fill — no renderer executes WGSL yet, '
            'so this paints the def\'s flat fallback_color instead of the real shader.">'
            '⚡ shader (fallback)</span>'
        ) if uses_shader else ''

        cards.append(f"""
<div class="example-card">
  <div class="example-header">
    <div class="example-title">{name}</div>
    <div class="example-chips">
      <span class="stat-chip">Source {source_bytes}B</span>
      <span class="stat-chip accent">Binary {binary_bytes}B</span>
      <span class="stat-chip purple">SVG {svg_bytes}B</span>
      <span class="stat-chip dim">bin/svg: {bin_pct:.1f}%</span>
      {rt_badge}
      {anim_badge}
      {shader_badge}
    </div>
  </div>
  <div class="example-body">
    <div class="example-pane pane-msx">
      <div class="pane-label"><span class="pane-dot pane-dot--msx"></span>MSX Source (.msx)</div>
      <pre class="source-code source-code--msx"><code>{msx_escaped}</code></pre>
    </div>
    <div class="example-divider">
      <span class="divider-label">compiles&nbsp;to</span>
      <div class="divider-arrow">→</div>
    </div>
    <div class="example-pane pane-svg-src">
      <div class="pane-label"><span class="pane-dot pane-dot--svg-src"></span>Generated SVG Code (export)</div>
      <pre class="source-code source-code--svg"><code>{svg_escaped}</code></pre>
    </div>
    <div class="example-divider">
      <span class="divider-label">renders&nbsp;as</span>
      <div class="divider-arrow">→</div>
    </div>
    <div class="example-pane pane-svg-visual">
      <div class="pane-label"><span class="pane-dot pane-dot--svg-vis"></span>Rendered Visual — native CPU raster</div>
      <div class="svg-preview">{rendered_visual}</div>
    </div>
  </div>
</div>""")

    return '<div class="example-gallery">' + "\n".join(cards) + "</div>"


# ── HTML template ─────────────────────────────────────────────────────────────

HTML_TEMPLATE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>MSX Build #{build} — {branch}@{commit}</title>
<style>
  :root {{
    --bg:       #0a0f1e;
    --surface:  #0f1729;
    --surface2: #141e33;
    --border:   #1e2d4a;
    --text:     #e2e8f0;
    --muted:    #94a3b8;
    --accent:   #4a9eff;
    --purple:   #a78bfa;
    --green:    #22c55e;
    --yellow:   #f5a623;
    --orange:   #f97316;
    --red:      #ef4444;
    --msx-tint: rgba(74,158,255,.06);
    --svg-tint: rgba(167,139,250,.06);
  }}
  *, *::before, *::after {{ box-sizing:border-box; margin:0; padding:0; }}
  body {{ background:var(--bg); color:var(--text); font-family:system-ui,-apple-system,sans-serif; font-size:14px; line-height:1.6; }}

  /* ── Header ── */
  .site-header {{
    background:linear-gradient(135deg,#0a0f1e 0%,#0d1729 60%,#111a35 100%);
    border-bottom:1px solid var(--border);
    padding:32px 40px;
  }}
  .site-header h1 {{ font-size:1.75rem; font-weight:800; letter-spacing:-.02em; }}
  .site-header h1 span {{ color:var(--accent); }}
  .tagline {{ color:var(--muted); margin-top:6px; font-size:.9rem; }}
  .build-meta {{ display:flex; gap:10px; flex-wrap:wrap; margin-top:14px; }}
  .badge {{
    background:var(--surface2); border:1px solid var(--border);
    border-radius:6px; padding:4px 12px; font-size:.78rem; color:var(--muted);
  }}
  .badge b {{ color:var(--text); }}

  /* ── Layout ── */
  .container {{ max-width:1600px; margin:0 auto; padding:28px 40px; }}
  .grid-2 {{ display:grid; grid-template-columns:1fr 1fr; gap:20px; }}

  /* ── Cards ── */
  .card {{
    background:var(--surface); border:1px solid var(--border);
    border-radius:12px; padding:20px;
  }}
  .card-title {{
    display:flex; align-items:center; gap:10px;
    font-weight:700; font-size:1rem; margin-bottom:16px;
  }}
  .dot {{
    width:10px; height:10px; border-radius:50%; flex-shrink:0;
    background:var(--accent);
  }}
  .dot.green  {{ background:var(--green); }}
  .dot.yellow {{ background:var(--yellow); }}
  .dot.purple {{ background:var(--purple); }}
  .dot.orange {{ background:var(--orange); }}
  .dot.red    {{ background:var(--red); }}

  /* ── Test summary ── */
  .test-summary {{ display:flex; align-items:center; gap:20px; margin-bottom:16px; }}
  .test-counts {{ display:flex; flex-direction:column; gap:6px; }}
  .count-num {{ font-size:1.6rem; font-weight:800; line-height:1; }}
  .count-num.green {{ color:var(--green); }}
  .count-num.red   {{ color:var(--red); }}
  .count-label {{ color:var(--muted); font-size:.8rem; }}

  .test-scroll {{ max-height:280px; overflow-y:auto; border:1px solid var(--border); border-radius:8px; }}
  table.tests {{ width:100%; border-collapse:collapse; }}
  table.tests tr {{ border-bottom:1px solid var(--surface2); }}
  table.tests tr:last-child {{ border-bottom:none; }}
  table.tests tr.pass  td.icon {{ color:var(--green); }}
  table.tests tr.fail  td.icon {{ color:var(--red); }}
  table.tests tr.ignore td.icon {{ color:var(--yellow); }}
  table.tests tr.fail {{ background:rgba(239,68,68,.06); }}
  table.tests td {{ padding:6px 10px; font-size:.82rem; }}
  td.icon    {{ width:20px; text-align:center; }}
  td.tname   {{ color:var(--muted); font-family:monospace; }}
  td.tstatus {{ width:60px; color:var(--muted); text-align:right; }}

  /* ── Charts ── */
  .chart {{ width:100%; height:auto; overflow:visible; }}
  .chart .bar-label  {{ font-size:11px; fill:#94a3b8; font-family:monospace; }}
  .chart .bar-val    {{ font-size:11px; fill:#cbd5e1; }}
  .chart .legend     {{ font-size:11px; fill:#94a3b8; }}
  .chart .chart-title {{ font-size:11px; fill:#64748b; }}
  .chart .rt-badge   {{ font-size:14px; font-weight:bold; dominant-baseline:middle; }}

  /* ── Example gallery ── */
  .example-gallery {{ display:flex; flex-direction:column; gap:32px; }}
  .example-card {{
    background:var(--surface2); border:1px solid var(--border);
    border-radius:12px; overflow:hidden;
  }}
  .example-header {{
    display:flex; align-items:center; justify-content:space-between;
    flex-wrap:wrap; gap:10px; padding:14px 20px;
    border-bottom:1px solid var(--border); background:rgba(255,255,255,.03);
  }}
  .example-title {{ font-weight:700; font-size:1rem; color:var(--accent); font-family:monospace; letter-spacing:.02em; }}
  .example-chips {{ display:flex; gap:6px; flex-wrap:wrap; align-items:center; }}

  .example-body {{
    display:grid;
    grid-template-columns: 1fr 52px 1fr 52px 1fr;
    min-height:300px;
  }}
  .example-pane {{ display:flex; flex-direction:column; min-width:0; }}
  .pane-msx     {{ border-right:1px solid var(--border); }}
  .pane-svg-src {{ border-right:1px solid var(--border); }}

  .pane-label {{
    display:flex; align-items:center; gap:8px; padding:7px 14px;
    font-size:.7rem; color:var(--muted); text-transform:uppercase;
    letter-spacing:.08em; border-bottom:1px solid var(--border);
    background:rgba(0,0,0,.2); flex-shrink:0;
  }}
  .pane-dot {{ width:8px; height:8px; border-radius:50%; flex-shrink:0; }}
  .pane-dot--msx     {{ background:var(--accent); }}
  .pane-dot--svg-src {{ background:var(--purple); }}
  .pane-dot--svg-vis {{ background:var(--green); }}

  .source-code {{
    flex:1; padding:12px 14px;
    font-family:'Consolas','Fira Code','Cascadia Code',monospace;
    font-size:.71rem; line-height:1.55; overflow:auto; white-space:pre; margin:0;
  }}
  .source-code--msx {{ background:var(--msx-tint); color:#93c5fd; }}
  .source-code--svg {{ background:var(--svg-tint); color:#c4b5fd; }}

  .svg-preview {{
    flex:1; background:#ffffff; display:flex; flex-direction:column;
    align-items:center; justify-content:center; padding:10px; min-height:180px; gap:8px;
  }}
  /* A native render (PNG/GIF) already has its own opaque background baked
     in from the scene's own `background` — wrapping it in a bright white
     frame is wasted contrast at best and jarring on a dark-themed page at
     worst. Raw inline <svg> content (which can have transparent regions)
     still gets the white backdrop so it stays visible. */
  .svg-preview:has(img.native-render) {{ background:var(--surface2); }}
  .svg-preview svg {{ max-width:100%; max-height:360px; height:auto; width:auto; display:block; }}
  .svg-preview img.native-render {{ max-width:100%; max-height:360px; height:auto; width:auto; display:block; }}
  .svg-preview img.native-render--anim {{ border-radius:6px; box-shadow:0 0 0 1px var(--accent), 0 0 16px -4px var(--accent); }}
  .compare-toggle {{
    padding:4px 10px; font-size:.7rem;
    font-family:inherit; color:var(--accent); background:transparent;
    border:1px solid var(--accent); border-radius:4px; cursor:pointer;
    flex-shrink:0;
  }}
  .compare-toggle:hover {{ background:var(--accent); color:var(--bg, #0a0f1e); }}
  .native-render-note {{ color:var(--muted); font-size:.7rem; text-align:center; }}

  .example-divider {{
    display:flex; flex-direction:column; align-items:center; justify-content:center;
    gap:6px; background:rgba(0,0,0,.15);
    border-left:1px solid var(--border); border-right:1px solid var(--border);
    padding:8px 4px; flex-shrink:0;
  }}
  .divider-label {{
    font-size:.6rem; color:#64748b; text-transform:uppercase; letter-spacing:.06em;
    writing-mode:vertical-rl; text-orientation:mixed; transform:rotate(180deg); white-space:nowrap;
  }}
  .divider-arrow {{ font-size:1rem; color:var(--muted); user-select:none; }}

  .stat-chip {{
    background:var(--surface); border:1px solid var(--border);
    border-radius:4px; padding:2px 9px; font-size:.72rem; color:var(--muted); white-space:nowrap;
  }}
  .stat-chip.accent {{ border-color:var(--accent); color:var(--accent); }}
  .stat-chip.purple {{ border-color:var(--purple); color:var(--purple); }}
  .stat-chip.green  {{ border-color:var(--green);  color:var(--green);  }}
  .stat-chip.red    {{ border-color:var(--red);    color:var(--red);    }}
  .stat-chip.dim    {{ color:#64748b; }}

  .rationale {{
    background:linear-gradient(135deg,#1e293b 0%,#1a2744 100%);
    border:1px solid #2d4a7a; border-radius:12px; padding:28px; margin-top:24px;
  }}
  .rationale h2 {{ font-size:1.1rem; font-weight:700; color:var(--accent); margin-bottom:16px; }}
  .rationale p {{ color:var(--muted); margin-bottom:12px; font-size:.9rem; }}
  .rationale p b {{ color:var(--text); }}
  .rationale code {{ background:var(--surface2); border-radius:4px; padding:1px 5px; font-family:monospace; font-size:.85em; color:var(--accent); }}
  .pipeline {{ display:flex; flex-direction:column; margin:16px 0; }}
  .pipeline-step {{ display:flex; align-items:flex-start; gap:16px; padding:10px 16px; background:var(--surface2); border-left:3px solid var(--accent); }}
  .pipeline-step:nth-child(2) {{ border-color:var(--purple); }}
  .pipeline-step:nth-child(3) {{ border-color:var(--green); }}
  .pipeline-step:nth-child(4) {{ border-color:var(--yellow); }}
  .pipeline-step:nth-child(5) {{ border-color:var(--orange); }}
  .pipeline-step + .pipeline-step {{ border-top:1px solid var(--border); }}
  .step-num {{ font-size:.75rem; font-weight:700; color:var(--muted); min-width:20px; }}
  .step-title {{ font-weight:600; font-size:.88rem; color:var(--text); margin-bottom:2px; }}
  .step-desc  {{ font-size:.8rem; color:var(--muted); }}
  .no-data {{ color:var(--muted); font-style:italic; padding:16px 0; }}

  footer {{
    border-top:1px solid var(--border); padding:20px 40px;
    color:var(--muted); font-size:.8rem; display:flex; justify-content:space-between; flex-wrap:wrap; gap:8px;
  }}
  footer a {{ color:var(--accent); text-decoration:none; }}

  @media (max-width:1100px) {{
    .example-body {{ grid-template-columns: 1fr 44px 1fr 44px 1fr; }}
    .source-code {{ font-size:.65rem; }}
  }}
  @media (max-width:860px) {{
    .grid-2 {{ grid-template-columns:1fr; }}
    .site-header {{ padding:24px 20px; }}
    .site-header h1 {{ font-size:1.5rem; }}
    .container {{ padding:20px 16px; }}
    .example-body {{ grid-template-columns: 1fr 40px 1fr; }}
    .pane-svg-src {{ display:none; }}
    .example-divider:first-of-type {{ display:none; }}
    .divider-label {{ display:none; }}
  }}
  @media (max-width:600px) {{
    .example-body {{ grid-template-columns: 1fr; grid-template-rows: auto; }}
    .pane-svg-src {{ display:flex; }}
    .example-divider:first-of-type {{ display:flex; }}
    .example-divider {{ flex-direction:row; height:36px; border-left:none; border-right:none; border-top:1px solid var(--border); border-bottom:1px solid var(--border); padding:0 14px; }}
    .divider-label {{ writing-mode:horizontal-tb; transform:none; display:block; }}
    .divider-arrow {{ transform:rotate(90deg); }}
    .pane-msx {{ border-right:none; border-bottom:1px solid var(--border); }}
    .pane-svg-src {{ border-right:none; border-bottom:1px solid var(--border); }}
    .source-code {{ max-height:200px; font-size:.67rem; }}
    .svg-preview {{ min-height:140px; }}
    .chart {{ overflow:hidden; }}
    .example-header {{ flex-direction:column; align-items:flex-start; }}
    footer {{ padding:16px 20px; flex-direction:column; gap:4px; }}
  }}
  @media (max-width:380px) {{
    .site-header h1 {{ font-size:1.2rem; }}
    .stat-chip {{ font-size:.62rem; padding:2px 5px; }}
  }}
</style>
</head>
<body>

<header class="site-header">
  <h1><span>MSX</span> — MidStroke eXchange</h1>
  <p class="tagline">Vector image format co-designed with DixScript and MBFA instruction-chain compression</p>
  <div class="build-meta">
    <div class="badge">Build <b>#{build}</b></div>
    <div class="badge">Commit <b>{commit}</b></div>
    <div class="badge">Branch <b>{branch}</b></div>
    <div class="badge">Generated <b>{timestamp}</b></div>
  </div>
</header>

<main class="container">

  <div class="grid-2" style="margin-bottom:24px">
    <div class="card">
      <div class="card-title"><div class="dot green"></div>Tests — Debug Build</div>
      <div class="test-summary">
        {donut_d}
        <div class="test-counts">
          <div><span class="count-num green">{passed_d}</span><span class="count-label"> passed</span></div>
          <div><span class="count-num red">{failed_d}</span><span class="count-label"> failed</span></div>
        </div>
      </div>
      <div class="test-scroll"><table class="tests"><tbody>{test_rows_d}</tbody></table></div>
    </div>
    <div class="card">
      <div class="card-title"><div class="dot green"></div>Tests — Release Build</div>
      <div class="test-summary">
        {donut_r}
        <div class="test-counts">
          <div><span class="count-num green">{passed_r}</span><span class="count-label"> passed</span></div>
          <div><span class="count-num red">{failed_r}</span><span class="count-label"> failed</span></div>
        </div>
      </div>
      <div class="test-scroll"><table class="tests"><tbody>{test_rows_r}</tbody></table></div>
    </div>
  </div>

  <div class="card" style="margin-bottom:24px">
    <div class="card-title">
      <div class="dot yellow"></div>
      Encode / Decode / Render Throughput (Criterion)
      <span style="font-size:.75rem;color:var(--muted);font-weight:400;margin-left:auto">higher = better</span>
    </div>
    {throughput_chart}
  </div>

  <div class="card" style="margin-bottom:24px">
    <div class="card-title">
      <div class="dot purple"></div>
      File Size Comparison — Source vs Binary vs SVG
      <span style="font-size:.75rem;color:var(--muted);font-weight:400;margin-left:auto">binary is MBFA-compressed MSX · ✓ = roundtrip verified</span>
    </div>
    {size_chart}
  </div>

  <div class="card" style="margin-bottom:24px">
    <div class="card-title">
      <div class="dot orange"></div>
      Example Gallery — MSX Source → SVG Code → Native Render
    </div>
    <p style="font-size:.8rem;color:var(--muted);margin-bottom:20px">
      Three panes per example: the original
      <code style="background:var(--surface2);border-radius:4px;padding:1px 5px;font-family:monospace;font-size:.85em;color:var(--accent)">.msx</code>
      DixScript source, the generated SVG export markup, and the actual MSX render —
      a PNG produced by <code style="background:var(--surface2);border-radius:4px;padding:1px 5px;font-family:monospace;font-size:.85em;color:var(--accent)">msx rasterize</code>
      (msx-render-cpu), not a browser reinterpreting the SVG next to it. A native pixel
      buffer is MSX's primary output — SVG is one optional export target, not the source
      of truth for what a file looks like. QuickFuncs are resolved at compile time —
      the binary contains only the flat scene graph.
    </p>
    {example_gallery}
  </div>

  <div class="rationale">
    <h2>Why MSX — DixScript + MBFA co-design</h2>
    <p>SVG is XML written by hand or generated by tools. MSX source files are
    <b>DixScript</b> — the same format powering configs, now driving vectors.
    QuickFuncs become parametric shape generators. MBFA compresses the typed binary stream.</p>
    <div class="pipeline">
      <div class="pipeline-step">
        <div class="step-num">1</div>
        <div class="step-body"><div class="step-title">DixScript source (.msx)</div><div class="step-desc">QuickFuncs define reusable components. Evaluated at compile time — no runtime overhead.</div></div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">2</div>
        <div class="step-body"><div class="step-title">DixScript runtime evaluation</div><div class="step-desc">Full pipeline: tokenise → parse → semantic analysis → QuickFuncs resolve. Output: flat DixData.</div></div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">3</div>
        <div class="step-body"><div class="step-title">Scene AST construction</div><div class="step-desc">DixData → typed Scene graph. Elements, defs, canvas, transforms all resolved.</div></div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">4</div>
        <div class="step-body"><div class="step-title">Binary encoding + MBFA</div><div class="step-desc">Typed element streams: coordinate f32s, opcode bytes, color RGBA, string pool. MBFA fold-1 LZ finds repeating patterns across element boundaries.</div></div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">5</div>
        <div class="step-body"><div class="step-title">Native rasterizer / SVG export</div><div class="step-desc">msx-render-cpu produces a pixel buffer (primary output). msx-render-svg exports SVG. msx-render-gpu handles real-time display.</div></div>
      </div>
    </div>
  </div>

</main>

<footer>
  <span>MidManStudio · MSX Vector Format · Build #{build} · {timestamp}</span>
  <span>
    <a href="https://github.com/Mid-D-Man/msx">msx</a> ·
    <a href="https://github.com/Mid-D-Man/mbfa">mbfa</a> ·
    <a href="https://github.com/Mid-D-Man/DixScript-Rust">dixscript</a>
  </span>
</footer>

<script>
function msxToggleRender(btn) {{
  const img = document.getElementById(btn.dataset.target);
  const showingGif = img.src.startsWith("data:image/gif");
  img.src = showingGif ? btn.dataset.png : btn.dataset.gif;
  btn.textContent = showingGif ? "back to animated" : "compare static frame";
}}
</script>

</body>
</html>
"""


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    args = parse_args()

    tests_d  = parse_tests(args.tests_d)
    tests_r  = parse_tests(args.tests_r)
    bench    = parse_bench(args.bench)
    corpus   = parse_corpus(args.corpus)
    examples = parse_examples(args.examples)

    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    html = HTML_TEMPLATE.format(
        build     = args.build,
        commit    = args.commit,
        branch    = args.branch,
        timestamp = timestamp,

        donut_d      = svg_test_donut(tests_d["passed"], tests_d["failed"]),
        passed_d     = tests_d["passed"],
        failed_d     = tests_d["failed"],
        test_rows_d  = test_rows_html(tests_d["tests"]),

        donut_r      = svg_test_donut(tests_r["passed"], tests_r["failed"]),
        passed_r     = tests_r["passed"],
        failed_r     = tests_r["failed"],
        test_rows_r  = test_rows_html(tests_r["tests"]),

        throughput_chart = svg_throughput_bars(bench),
        size_chart       = svg_size_comparison_bars(corpus),
        example_gallery  = svg_gallery_html(examples),
    )

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        f.write(html)

    print(f"Report written to {args.out}")
    print(f"  Tests (debug):   {tests_d['passed']} passed / {tests_d['failed']} failed  ({len(tests_d['tests'])} lines)")
    print(f"  Tests (release): {tests_r['passed']} passed / {tests_r['failed']} failed  ({len(tests_r['tests'])} lines)")
    print(f"  Bench entries:   {len(bench)}")
    print(f"  Corpus rows:     {len(corpus)}")
    print(f"  Examples:        {len(examples)}")


if __name__ == "__main__":
    main()
