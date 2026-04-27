#!/usr/bin/env python3
"""
generate_report.py
Parses MSX CI output and generates a self-contained HTML report.
"""

import argparse
import json
import os
import re
import sys
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


# ── Parsers ───────────────────────────────────────────────────────────────────

# Shared ANSI escape-sequence stripper — used by both parse_tests and parse_bench.
_RE_ANSI = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')

def _strip_ansi(s: str) -> str:
    return _RE_ANSI.sub('', s)


def parse_tests(path: str) -> dict:
    """
    Parse cargo test output.

    Cargo may emit ANSI colour codes around test lines even when piped through
    tee (e.g. `\x1b[32mtest foo ... ok\x1b[0m`).  The DixScript runtime also
    writes structured log lines to stdout that are captured alongside the test
    output.  We strip ANSI first, then only match lines that start with 'test '.
    """
    tests = []
    passed = failed = 0
    # Match both the normal one-line format:
    #   test foo::bar ... ok
    # and the two-line criterion-style format (handled by falling through):
    #   test foo::bar ...
    #   bench: ...
    RE_TEST_LINE = re.compile(
        r'^test (.+?) \.\.\. (ok|FAILED|ignored)'
    )
    try:
        with open(path, errors='replace') as f:
            for raw_line in f:
                line = _strip_ansi(raw_line).rstrip()
                m = RE_TEST_LINE.match(line)
                if m:
                    name, status = m.group(1).strip(), m.group(2)
                    tests.append({"name": name, "status": status})
                    if status == "ok":       passed += 1
                    elif status == "FAILED": failed += 1
    except FileNotFoundError:
        pass
    return {"passed": passed, "failed": failed, "tests": tests}


def parse_bench(path: str) -> list:
    """
    Parse Criterion --output-format bencher output.
    Also handles verbose MBFA diagnostics interleaved with bench output.
    Returns [{name, ns, var}]
    """
    results = []
    try:
        with open(path, errors='replace') as f:
            lines = f.readlines()
    except FileNotFoundError:
        return results

    RE_FULL  = re.compile(
        r'^test (.+?) \.\.\. bench:\s+([\d,]+) ns/iter \(\+/- ([\d,]+)\)'
    )
    RE_TEST  = re.compile(r'^test (.+?) \.\.\.')
    RE_BENCH = re.compile(r'^\s*bench:\s+([\d,]+) ns/iter \(\+/- ([\d,]+)\)')

    pending_name = None

    for raw_line in lines:
        line = _strip_ansi(raw_line).rstrip()

        m = RE_FULL.match(line)
        if m:
            results.append({
                "name": m.group(1).strip(),
                "ns":   int(m.group(2).replace(",", "")),
                "var":  int(m.group(3).replace(",", "")),
            })
            pending_name = None
            continue

        m = RE_TEST.match(line)
        if m:
            pending_name = m.group(1).strip()
            continue

        m = RE_BENCH.match(line)
        if m:
            ns  = int(m.group(1).replace(",", ""))
            var = int(m.group(2).replace(",", ""))
            name = pending_name if pending_name else f"bench_{len(results) + 1}"
            results.append({"name": name, "ns": ns, "var": var})
            pending_name = None
            continue

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

    def arc_path(start_deg: float, end_deg: float, ro: int, ri: int) -> str:
        def pt(deg: float, radius: int):
            rad = math.radians(deg - 90)
            return cx + radius * math.cos(rad), cy + radius * math.sin(rad)
        x1, y1 = pt(start_deg, ro)
        x2, y2 = pt(end_deg,   ro)
        x3, y3 = pt(end_deg,   ri)
        x4, y4 = pt(start_deg, ri)
        large  = 1 if (end_deg - start_deg) > 180 else 0
        return (f"M {x1:.2f} {y1:.2f} "
                f"A {ro} {ro} 0 {large} 1 {x2:.2f} {y2:.2f} "
                f"L {x3:.2f} {y3:.2f} "
                f"A {ri} {ri} 0 {large} 0 {x4:.2f} {y4:.2f} Z")

    pass_deg = 360 * passed / total
    lines = [
        f'<svg viewBox="0 0 {size} {size}" xmlns="http://www.w3.org/2000/svg" '
        f'width="{size}" height="{size}">',
    ]
    if failed == 0:
        lines.append(f'<path d="{arc_path(0, 359.99, r_out, r_in)}" fill="#22c55e"/>')
    else:
        lines.append(f'<path d="{arc_path(0, pass_deg, r_out, r_in)}" fill="#22c55e"/>')
        lines.append(f'<path d="{arc_path(pass_deg, 360, r_out, r_in)}" fill="#ef4444"/>')
    lines.append(
        f'<text x="{cx}" y="{cy-4}" text-anchor="middle" '
        f'font-size="14" font-weight="bold" fill="#f1f5f9">{passed}</text>'
    )
    lines.append(
        f'<text x="{cx}" y="{cy+12}" text-anchor="middle" '
        f'font-size="9" fill="#94a3b8">passed</text>'
    )
    lines.append("</svg>")
    return "\n".join(lines)


def test_rows_html(tests: list) -> str:
    if not tests:
        return "<p class='no-data'>No test data.</p>"
    rows = []
    for t in tests:
        icon = "✓" if t["status"] == "ok" else ("⚠" if t["status"] == "ignored" else "✗")
        cls  = ("pass" if t["status"] == "ok"
                else ("ignore" if t["status"] == "ignored" else "fail"))
        rows.append(
            f'<tr class="{cls}"><td class="icon">{icon}</td>'
            f'<td class="tname">{t["name"]}</td>'
            f'<td class="tstatus">{t["status"]}</td></tr>'
        )
    return "\n".join(rows)


def svg_throughput_bars(bench_rows: list) -> str:
    def estimate_bytes(name: str) -> int:
        m = re.search(r"(\d+)_circles", name)
        if m:
            return int(m.group(1)) * 20
        if re.search(r"badges?", name):
            return 6 * 120
        return 1024

    items = []
    for r in bench_rows:
        if r["ns"] == 0:
            continue
        raw_bytes = estimate_bytes(r["name"])
        mbps = (raw_bytes / (r["ns"] / 1e9)) / (1024 * 1024)
        items.append({"name": r["name"], "mbps": mbps, "ns": r["ns"]})

    if not items:
        return "<p class='no-data'>No benchmark data yet — run <code>cargo bench</code>.</p>"

    items.sort(key=lambda x: x["mbps"])
    max_mbps = max(i["mbps"] for i in items)

    bar_h    = 22
    gap      = 6
    label_w  = 340
    chart_w  = 700
    bar_area = chart_w - label_w - 80
    total_h  = len(items) * (bar_h + gap) + 50

    lines = [
        f'<svg viewBox="0 0 {chart_w} {total_h}" '
        f'xmlns="http://www.w3.org/2000/svg" class="chart">',
        f'<text x="{label_w + bar_area//2}" y="18" text-anchor="middle" '
        f'class="chart-title">Throughput (MB/s)  —  higher is better</text>',
    ]
    y = 30
    for item in items:
        w = item["mbps"] / max(max_mbps, 1) * bar_area
        short = item["name"].replace("parse_render/", "parse+render/")
        ns_ms = (f"{item['ns']/1e6:.1f}ms" if item['ns'] >= 1_000_000
                 else f"{item['ns']/1e3:.0f}µs")
        lines.append(
            f'<text x="{label_w-6}" y="{y+bar_h//2+4}" '
            f'text-anchor="end" class="bar-label">{short}</text>'
        )
        lines.append(
            f'<rect x="{label_w}" y="{y}" width="{max(w, 2):.1f}" height="{bar_h}" '
            f'fill="#4a9eff" rx="3"/>'
        )
        lines.append(
            f'<text x="{label_w+max(w,2)+6}" y="{y+bar_h//2+4}" '
            f'class="bar-val">{item["mbps"]:.1f} MB/s ({ns_ms}/iter)</text>'
        )
        y += bar_h + gap
    lines.append("</svg>")
    return "\n".join(lines)


def svg_size_comparison_bars(corpus_rows: list) -> str:
    if not corpus_rows:
        return "<p class='no-data'>No corpus data available.</p>"

    bar_h    = 18
    gap      = 6
    label_w  = 180
    chart_w  = 620
    bar_area = chart_w - label_w - 60
    group_h  = bar_h * 3 + gap * 2 + 12
    total_h  = len(corpus_rows) * group_h + 60
    max_bytes = max(max(r["source_bytes"], r["svg_bytes"]) for r in corpus_rows) or 1
    scale = bar_area / max_bytes

    lines = [
        f'<svg viewBox="0 0 {chart_w} {total_h}" '
        f'xmlns="http://www.w3.org/2000/svg" class="chart">',
        f'<rect x="{label_w}" y="8" width="12" height="12" fill="#94a3b8"/>',
        f'<text x="{label_w+16}" y="19" class="legend">Source .msx</text>',
        f'<rect x="{label_w+110}" y="8" width="12" height="12" fill="#4a9eff"/>',
        f'<text x="{label_w+126}" y="19" class="legend">Binary (MBFA)</text>',
        f'<rect x="{label_w+250}" y="8" width="12" height="12" fill="#a78bfa"/>',
        f'<text x="{label_w+266}" y="19" class="legend">SVG output</text>',
    ]
    y = 40
    for r in corpus_rows:
        lines.append(
            f'<text x="{label_w-8}" y="{y + bar_h + 4}" '
            f'text-anchor="end" class="bar-label">{r["name"]}</text>'
        )
        sw = max(r["source_bytes"] * scale, 2)
        lines.append(f'<rect x="{label_w}" y="{y}" width="{sw:.1f}" height="{bar_h}" fill="#94a3b8" rx="2"/>')
        lines.append(f'<text x="{label_w+sw+4}" y="{y+bar_h-3}" class="bar-val">{r["source_bytes"]}B</text>')
        bw = max(r["binary_bytes"] * scale, 2)
        lines.append(f'<rect x="{label_w}" y="{y+bar_h+gap}" width="{bw:.1f}" height="{bar_h}" fill="#4a9eff" rx="2"/>')
        lines.append(f'<text x="{label_w+bw+4}" y="{y+bar_h*2+gap-3}" class="bar-val">{r["binary_bytes"]}B ({r["bin_pct"]:.0f}% of src)</text>')
        vw = max(r["svg_bytes"] * scale, 2)
        lines.append(f'<rect x="{label_w}" y="{y+(bar_h+gap)*2}" width="{vw:.1f}" height="{bar_h}" fill="#a78bfa" rx="2"/>')
        lines.append(f'<text x="{label_w+vw+4}" y="{y+bar_h*3+gap*2-3}" class="bar-val">{r["svg_bytes"]}B ({r["svg_pct"]:.0f}% of src)</text>')
        colour  = "#22c55e" if r.get("pass") else "#ef4444"
        label_t = "✓" if r.get("pass") else "✗"
        lines.append(
            f'<text x="{chart_w-6}" y="{y+bar_h+gap+bar_h//2}" '
            f'fill="{colour}" class="rt-badge">{label_t}</text>'
        )
        y += group_h
    lines.append("</svg>")
    return "\n".join(lines)


# ── SVG preview gallery ───────────────────────────────────────────────────────

def svg_gallery_html(examples: list) -> str:
    if not examples:
        return "<p class='no-data'>No example SVG data available.</p>"

    cards = []
    for ex in examples:
        name         = ex.get("name", "unknown")
        source       = ex.get("source", "")
        svg_content  = ex.get("svg", "")
        source_bytes = ex.get("source_bytes", 0)
        binary_bytes = ex.get("binary_bytes", 0)
        svg_bytes    = ex.get("svg_bytes", 0)
        passed       = ex.get("pass", False)

        src_escaped = (source
                       .replace("&", "&amp;")
                       .replace("<", "&lt;")
                       .replace(">", "&gt;"))

        svg_display = (svg_content if svg_content
                       else "<p style='color:#94a3b8;font-size:0.8rem'>No SVG rendered</p>")
        bin_pct = (binary_bytes / max(svg_bytes, 1)) * 100 if svg_bytes > 0 else 0
        rt_badge = (
            '<span class="stat-chip green">✓ roundtrip</span>'
            if passed else
            '<span class="stat-chip red">✗ roundtrip failed</span>'
        )

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
    </div>
  </div>
  <div class="example-body">
    <div class="example-pane example-pane--source">
      <div class="pane-label">
        <span class="pane-label-dot pane-label-dot--msx"></span>
        MSX Source
      </div>
      <pre class="source-code"><code>{src_escaped}</code></pre>
    </div>
    <div class="example-divider">
      <div class="divider-arrow">→</div>
    </div>
    <div class="example-pane example-pane--svg">
      <div class="pane-label">
        <span class="pane-label-dot pane-label-dot--svg"></span>
        Rendered SVG
      </div>
      <div class="svg-preview">{svg_display}</div>
    </div>
  </div>
</div>""")

    return '<div class="example-gallery">' + "\n".join(cards) + "</div>"


# ── HTML template — identical to response 3 ──────────────────────────────────
# (Full template omitted here for brevity — use the one from response 3 verbatim)

HTML_TEMPLATE = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>MSX — MidStroke eXchange | Build #{build}</title>
<style>
  :root {{
    --bg:        #0f172a;
    --surface:   #1e293b;
    --surface2:  #334155;
    --border:    #475569;
    --text:      #f1f5f9;
    --muted:     #94a3b8;
    --accent:    #4a9eff;
    --purple:    #a78bfa;
    --green:     #22c55e;
    --red:       #ef4444;
    --yellow:    #eab308;
    --orange:    #f97316;
    --gallery-gap: 28px;
  }}
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    background: var(--bg);
    color: var(--text);
    font-family: 'Segoe UI', system-ui, -apple-system, sans-serif;
    font-size: 14px;
    line-height: 1.6;
  }}
  .site-header {{
    background: linear-gradient(135deg, #0f172a 0%, #1a2f5f 50%, #0f172a 100%);
    border-bottom: 1px solid var(--border);
    padding: 32px 40px;
    position: relative;
    overflow: hidden;
  }}
  .site-header::before {{
    content: '';
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(
      -55deg, transparent, transparent 8px,
      rgba(74,158,255,0.03) 8px, rgba(74,158,255,0.03) 9px
    );
    pointer-events: none;
  }}
  .site-header h1 {{ font-size: 2rem; font-weight: 800; letter-spacing: -0.02em; }}
  .site-header h1 span {{ color: var(--accent); }}
  .site-header .tagline {{ color: var(--muted); margin-top: 4px; font-size: 0.95rem; }}
  .build-meta {{ margin-top: 12px; display: flex; gap: 12px; flex-wrap: wrap; }}
  .build-meta .badge {{
    background: var(--surface2); border: 1px solid var(--border);
    border-radius: 6px; padding: 4px 10px; font-size: 0.8rem; color: var(--muted);
  }}
  .build-meta .badge b {{ color: var(--text); }}
  .container {{ max-width: 1300px; margin: 0 auto; padding: 32px 24px; }}
  .grid-2 {{ display: grid; grid-template-columns: 1fr 1fr; gap: 24px; }}
  .card {{
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 12px; padding: 24px;
  }}
  .card-title {{
    font-size: 1rem; font-weight: 700; margin-bottom: 16px;
    display: flex; align-items: center; gap: 10px;
  }}
  .card-title .dot {{ width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex-shrink: 0; }}
  .card-title .dot.green  {{ background: var(--green); }}
  .card-title .dot.purple {{ background: var(--purple); }}
  .card-title .dot.yellow {{ background: var(--yellow); }}
  .card-title .dot.orange {{ background: var(--orange); }}
  .test-summary {{ display: flex; align-items: center; gap: 24px; margin-bottom: 20px; }}
  .test-counts {{ display: flex; flex-direction: column; gap: 6px; }}
  .count-num {{ font-size: 1.6rem; font-weight: 800; line-height: 1; }}
  .count-num.green {{ color: var(--green); }}
  .count-num.red   {{ color: var(--red); }}
  .count-label {{ color: var(--muted); font-size: 0.8rem; }}
  .test-scroll {{ max-height: 280px; overflow-y: auto; border: 1px solid var(--border); border-radius: 8px; }}
  table.tests {{ width: 100%; border-collapse: collapse; }}
  table.tests tr {{ border-bottom: 1px solid var(--surface2); }}
  table.tests tr:last-child {{ border-bottom: none; }}
  table.tests tr.pass  td.icon {{ color: var(--green); }}
  table.tests tr.fail  td.icon {{ color: var(--red); }}
  table.tests tr.ignore td.icon {{ color: var(--yellow); }}
  table.tests tr.fail {{ background: rgba(239,68,68,0.06); }}
  table.tests td {{ padding: 6px 10px; font-size: 0.82rem; }}
  td.icon    {{ width: 20px; text-align: center; }}
  td.tname   {{ color: var(--muted); font-family: monospace; }}
  td.tstatus {{ width: 60px; color: var(--muted); text-align: right; }}
  .chart {{ width: 100%; height: auto; overflow: visible; }}
  .chart .bar-label  {{ font-size: 11px; fill: #94a3b8; font-family: monospace; }}
  .chart .bar-val    {{ font-size: 11px; fill: #cbd5e1; }}
  .chart .legend     {{ font-size: 11px; fill: #94a3b8; }}
  .chart .chart-title {{ font-size: 11px; fill: #64748b; }}
  .chart .rt-badge   {{ font-size: 14px; font-weight: bold; dominant-baseline: middle; }}
  .example-gallery {{ display: flex; flex-direction: column; gap: var(--gallery-gap); }}
  .example-card {{
    background: var(--surface2); border: 1px solid var(--border);
    border-radius: 12px; overflow: hidden;
  }}
  .example-header {{
    display: flex; align-items: center; justify-content: space-between;
    flex-wrap: wrap; gap: 10px; padding: 14px 20px;
    border-bottom: 1px solid var(--border);
    background: rgba(255,255,255,0.03);
  }}
  .example-title {{
    font-weight: 700; font-size: 1rem; color: var(--accent);
    font-family: monospace; letter-spacing: 0.02em;
  }}
  .example-chips {{ display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }}
  .example-body {{
    display: grid;
    grid-template-columns: 1fr 36px 1fr;
    min-height: 260px;
  }}
  .example-pane {{ display: flex; flex-direction: column; min-width: 0; }}
  .example-pane--source {{ border-right: 1px solid var(--border); }}
  .pane-label {{
    display: flex; align-items: center; gap: 7px;
    padding: 8px 14px; font-size: 0.72rem; color: var(--muted);
    text-transform: uppercase; letter-spacing: 0.08em;
    border-bottom: 1px solid var(--border);
    background: rgba(0,0,0,0.15); flex-shrink: 0;
  }}
  .pane-label-dot {{ width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }}
  .pane-label-dot--msx {{ background: var(--accent); }}
  .pane-label-dot--svg {{ background: var(--purple); }}
  .source-code {{
    flex: 1; background: #0a0f1e; padding: 14px 16px;
    font-family: 'Consolas', 'Fira Code', monospace; font-size: 0.73rem;
    color: #93c5fd; overflow: auto; white-space: pre; line-height: 1.55; margin: 0;
  }}
  .svg-preview {{
    flex: 1; background: #ffffff; display: flex;
    align-items: center; justify-content: center;
    padding: 12px; min-height: 200px;
  }}
  .svg-preview svg {{ max-width: 100%; max-height: 340px; height: auto; width: auto; display: block; }}
  .example-divider {{
    display: flex; align-items: center; justify-content: center;
    background: var(--surface2);
    border-left: 1px solid var(--border); border-right: 1px solid var(--border);
    flex-shrink: 0;
  }}
  .divider-arrow {{ font-size: 1.1rem; color: var(--muted); user-select: none; }}
  .stat-chip {{
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 4px; padding: 2px 9px; font-size: 0.72rem;
    color: var(--muted); white-space: nowrap;
  }}
  .stat-chip.accent {{ border-color: var(--accent); color: var(--accent); }}
  .stat-chip.purple {{ border-color: var(--purple); color: var(--purple); }}
  .stat-chip.green  {{ border-color: var(--green);  color: var(--green);  }}
  .stat-chip.red    {{ border-color: var(--red);    color: var(--red);    }}
  .stat-chip.dim    {{ color: #64748b; }}
  .rationale {{
    background: linear-gradient(135deg, #1e293b 0%, #1a2744 100%);
    border: 1px solid #2d4a7a; border-radius: 12px; padding: 28px; margin-top: 24px;
  }}
  .rationale h2 {{ font-size: 1.1rem; font-weight: 700; color: var(--accent); margin-bottom: 16px; }}
  .rationale p {{ color: var(--muted); margin-bottom: 12px; font-size: 0.9rem; }}
  .rationale p b {{ color: var(--text); }}
  .rationale code {{
    background: var(--surface2); border-radius: 4px; padding: 1px 5px;
    font-family: monospace; font-size: 0.85em; color: var(--accent);
  }}
  .pipeline {{ display: flex; flex-direction: column; gap: 0; margin: 16px 0; }}
  .pipeline-step {{
    display: flex; align-items: flex-start; gap: 16px; padding: 10px 16px;
    background: var(--surface2); border-left: 3px solid var(--accent);
  }}
  .pipeline-step:nth-child(2) {{ border-color: var(--purple); }}
  .pipeline-step:nth-child(3) {{ border-color: var(--green); }}
  .pipeline-step:nth-child(4) {{ border-color: var(--yellow); }}
  .pipeline-step:nth-child(5) {{ border-color: var(--orange); }}
  .pipeline-step + .pipeline-step {{ border-top: 1px solid var(--border); }}
  .step-num {{ font-size: 0.75rem; font-weight: 700; color: var(--muted); min-width: 20px; }}
  .step-body {{ flex: 1; }}
  .step-title {{ font-weight: 600; font-size: 0.88rem; color: var(--text); margin-bottom: 2px; }}
  .step-desc  {{ font-size: 0.8rem; color: var(--muted); }}
  .no-data {{ color: var(--muted); font-style: italic; padding: 16px 0; }}
  footer {{
    border-top: 1px solid var(--border); padding: 20px 40px;
    color: var(--muted); font-size: 0.8rem;
    display: flex; justify-content: space-between; flex-wrap: wrap; gap: 8px;
  }}
  footer a {{ color: var(--accent); text-decoration: none; }}
  @media (max-width: 900px) {{
    .grid-2 {{ grid-template-columns: 1fr; }}
    .site-header {{ padding: 24px 20px; }}
    .site-header h1 {{ font-size: 1.5rem; }}
    .container {{ padding: 20px 16px; }}
  }}
  @media (max-width: 680px) {{
    .example-body {{ grid-template-columns: 1fr; grid-template-rows: auto auto auto; }}
    .example-pane--source {{ border-right: none; border-bottom: 1px solid var(--border); }}
    .example-divider {{
      border-left: none; border-right: none;
      border-top: 1px solid var(--border); border-bottom: 1px solid var(--border);
      height: 32px;
    }}
    .divider-arrow {{ transform: rotate(90deg); }}
    .source-code {{ max-height: 220px; font-size: 0.68rem; }}
    .svg-preview {{ min-height: 160px; }}
    .example-header {{ flex-direction: column; align-items: flex-start; }}
    .build-meta {{ gap: 8px; }}
    footer {{ padding: 16px 20px; flex-direction: column; gap: 4px; }}
  }}
  @media (max-width: 400px) {{
    .site-header h1 {{ font-size: 1.2rem; }}
    .stat-chip {{ font-size: 0.65rem; padding: 2px 6px; }}
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
      <div class="test-scroll">
        <table class="tests"><tbody>{test_rows_d}</tbody></table>
      </div>
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
      <div class="test-scroll">
        <table class="tests"><tbody>{test_rows_r}</tbody></table>
      </div>
    </div>
  </div>
  <div class="card" style="margin-bottom:24px">
    <div class="card-title">
      <div class="dot yellow"></div>
      Encode / Decode / Render Throughput (Criterion)
      <span style="font-size:0.75rem;color:var(--muted);font-weight:400;margin-left:auto">higher = better</span>
    </div>
    {throughput_chart}
  </div>
  <div class="card" style="margin-bottom:24px">
    <div class="card-title">
      <div class="dot purple"></div>
      File Size Comparison — Source vs Binary vs SVG
      <span style="font-size:0.75rem;color:var(--muted);font-weight:400;margin-left:auto">binary is MBFA-compressed MSX · ✓ = roundtrip verified</span>
    </div>
    {size_chart}
  </div>
  <div class="card" style="margin-bottom:24px">
    <div class="card-title"><div class="dot orange"></div>Example Gallery — MSX Source → Rendered SVG</div>
    <p style="font-size:0.8rem;color:var(--muted);margin-bottom:20px">
      Each example is a real <code style="background:var(--surface2);border-radius:4px;padding:1px 5px;font-family:monospace;font-size:0.85em;color:var(--accent)">.msx</code>
      DixScript file compiled and rendered by the CI.
      QuickFuncs are evaluated at compile time — the binary contains only the resolved scene graph.
    </p>
    {example_gallery}
  </div>
  <div class="rationale">
    <h2>Why MSX — DixScript + MBFA co-design</h2>
    <p>SVG is XML written by hand or generated by tools. MSX source files are <b>DixScript</b> — the same format powering configs, now driving vectors. QuickFuncs become parametric shape generators. MBFA compresses the typed binary stream.</p>
    <div class="pipeline">
      <div class="pipeline-step">
        <div class="step-num">1</div>
        <div class="step-body">
          <div class="step-title">DixScript source (.msx)</div>
          <div class="step-desc">QuickFuncs define reusable components. Evaluated at compile time — no runtime overhead.</div>
        </div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">2</div>
        <div class="step-body">
          <div class="step-title">DixScript runtime evaluation</div>
          <div class="step-desc">Full pipeline: tokenise → parse → semantic analysis → QuickFuncs resolve. Output: flat DixData.</div>
        </div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">3</div>
        <div class="step-body">
          <div class="step-title">Scene AST construction</div>
          <div class="step-desc">DixData → typed Scene graph. Elements, defs, canvas, transforms all resolved.</div>
        </div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">4</div>
        <div class="step-body">
          <div class="step-title">Binary encoding + MBFA</div>
          <div class="step-desc">Typed element streams: coordinate f32s, opcode bytes, color RGBA, string pool. MBFA fold-1 LZ finds repeating patterns across element boundaries.</div>
        </div>
      </div>
      <div class="pipeline-step">
        <div class="step-num">5</div>
        <div class="step-body">
          <div class="step-title">SVG renderer</div>
          <div class="step-desc">Scene → pixel-perfect SVG 1.1. Runs from binary or source. Identical output either path.</div>
        </div>
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
    print(f"  Tests (debug):   {tests_d['passed']} passed, {tests_d['failed']} failed")
    print(f"  Tests (release): {tests_r['passed']} passed, {tests_r['failed']} failed")
    print(f"  Bench entries:   {len(bench)}")
    print(f"  Corpus rows:     {len(corpus)}")
    print(f"  Examples:        {len(examples)}")


if __name__ == "__main__":
    main()
