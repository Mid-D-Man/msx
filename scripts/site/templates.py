#!/usr/bin/env python3
"""
templates.py
Shared design system (CSS) and page shell for the MSX site. One stylesheet,
inlined into every page (this project's own convention — see the existing
.github/pages/generate_report.py, which does the same for the CI report —
kept here for consistency, and because a static site this size gains
nothing from a separate cacheable stylesheet request).

Color tokens continue --bg/--accent from the existing CI report
(generate_report.py) rather than picking a new palette — the report page
stays part of this same site (linked from the nav as "Build Report"), so
the two should look like one product, not two different tools bolted
together.
"""

CSS = """
:root {
  --bg:        #0a0f1e;
  --bg-raised: #0f1729;
  --surface:   #131c30;
  --surface2:  #1a2438;
  --border:    #232f47;
  --text:      #dbe4f3;
  --muted:     #8291ab;
  --accent:    #4a9eff;
  --accent-dim: #2d6bb3;
  --good:      #3ecf8e;
  --warn:      #f5a623;
  --bad:       #f2545b;
  --mono: 'Cascadia Code','Fira Code','Consolas',ui-monospace,monospace;
  --sans: -apple-system,BlinkMacSystemFont,'Segoe UI',system-ui,sans-serif;
  --max-w: 1180px;
}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; scroll-padding-top: 76px; }
body {
  background: var(--bg); color: var(--text); font-family: var(--sans);
  font-size: 15px; line-height: 1.65; margin: 0;
}
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
code, pre { font-family: var(--mono); }
code { background: var(--surface2); border-radius: 4px; padding: 1px 6px; font-size: .88em; color: #b9d4ff; }
pre.code { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 16px 18px; overflow-x: auto; font-size: 13px; line-height: 1.55; }
pre.code code { background: none; padding: 0; color: inherit; }

/* ── Top nav ─────────────────────────────────────────────────────────── */
.site-nav {
  position: sticky; top: 0; z-index: 50;
  display: flex; align-items: center; gap: 4px;
  padding: 0 24px; height: 60px;
  background: rgba(10,15,30,.92); backdrop-filter: blur(10px);
  border-bottom: 1px solid var(--border);
}
.site-nav .brand { font-family: var(--mono); font-weight: 700; font-size: 17px; color: var(--text); margin-right: 20px; white-space: nowrap; }
.site-nav .brand span { color: var(--accent); }
.site-nav .links { display: flex; gap: 2px; flex: 1; }
.site-nav a.nav-link {
  color: var(--muted); font-size: 14px; font-weight: 500; padding: 8px 14px;
  border-radius: 6px; transition: background .12s, color .12s;
}
.site-nav a.nav-link:hover { background: var(--surface2); color: var(--text); text-decoration: none; }
.site-nav a.nav-link.active { color: var(--accent); background: rgba(74,158,255,.1); }
.site-nav .gh-link { margin-left: auto; color: var(--muted); font-size: 13px; font-family: var(--mono); }
.site-nav .gh-link:hover { color: var(--text); }

/* ── Page shell ──────────────────────────────────────────────────────── */
.page { max-width: var(--max-w); margin: 0 auto; padding: 48px 24px 80px; }
.page.wide { max-width: 1400px; }
h1 { font-size: 2.1rem; font-weight: 800; letter-spacing: -.01em; margin: 0 0 8px; }
h2 { font-size: 1.5rem; font-weight: 700; margin: 44px 0 14px; padding-top: 8px; border-top: 1px solid var(--border); }
h2:first-of-type { border-top: none; padding-top: 0; }
h3 { font-size: 1.15rem; font-weight: 700; margin: 30px 0 10px; color: var(--text); }
h3 code { color: var(--accent); }
h4 { font-size: .95rem; font-weight: 700; margin: 22px 0 8px; color: var(--muted); text-transform: uppercase; letter-spacing: .04em; }
p { margin: 0 0 14px; color: var(--text); }
hr { border: none; border-top: 1px solid var(--border); margin: 32px 0; }
ul { margin: 0 0 14px; padding-left: 22px; }
li { margin-bottom: 6px; }
.lede { font-size: 1.15rem; color: var(--muted); margin-bottom: 28px; }

table.doc-table { width: 100%; border-collapse: collapse; margin: 0 0 20px; font-size: 14px; }
table.doc-table th { text-align: left; color: var(--muted); font-weight: 600; padding: 8px 12px; border-bottom: 2px solid var(--border); }
table.doc-table td { padding: 8px 12px; border-bottom: 1px solid var(--border); vertical-align: top; }
table.doc-table tr:hover td { background: var(--surface); }

/* ── Docs layout: sidebar + content ─────────────────────────────────── */
.docs-layout { display: grid; grid-template-columns: 240px minmax(0,1fr) 200px; gap: 40px; align-items: start; }
.docs-sidenav { position: sticky; top: 76px; font-size: 13.5px; }
.docs-sidenav .group-title { color: var(--muted); text-transform: uppercase; font-size: 11px; letter-spacing: .06em; font-weight: 700; margin: 18px 0 6px; }
.docs-sidenav .group-title:first-child { margin-top: 0; }
.docs-sidenav a { display: block; color: var(--muted); padding: 5px 0; }
.docs-sidenav a:hover { color: var(--text); text-decoration: none; }
.docs-sidenav a.active { color: var(--accent); font-weight: 600; }
.docs-toc { position: sticky; top: 76px; font-size: 13px; }
.docs-toc .toc-title { color: var(--muted); text-transform: uppercase; font-size: 11px; letter-spacing: .06em; font-weight: 700; margin-bottom: 8px; }
.docs-toc a { display: block; color: var(--muted); padding: 3px 0; border-left: 2px solid transparent; padding-left: 10px; }
.docs-toc a.toc-h3 { padding-left: 20px; font-size: 12.5px; }
.docs-toc a:hover { color: var(--text); text-decoration: none; border-left-color: var(--border); }
.docs-content { min-width: 0; }
@media (max-width: 980px) { .docs-layout { grid-template-columns: 1fr; } .docs-sidenav, .docs-toc { position: static; } }

/* ── Cards / grids (home, samples) ──────────────────────────────────── */
.card-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(270px, 1fr)); gap: 16px; margin: 20px 0; }
.card {
  background: var(--surface); border: 1px solid var(--border); border-radius: 10px;
  padding: 20px; transition: border-color .12s, transform .12s;
}
.card:hover { border-color: var(--accent-dim); }
.card h3 { margin: 0 0 8px; font-size: 1.02rem; }
.card p { color: var(--muted); font-size: 13.5px; margin: 0; }

.hero { padding: 64px 0 40px; text-align: center; }
.hero h1 { font-size: 2.8rem; margin-bottom: 14px; }
.hero .lede { max-width: 620px; margin: 0 auto 30px; }
.hero .actions { display: flex; gap: 12px; justify-content: center; flex-wrap: wrap; }
.btn {
  display: inline-block; padding: 11px 22px; border-radius: 8px; font-weight: 600; font-size: 14.5px;
  font-family: var(--sans);
}
.btn:hover { text-decoration: none; }
.btn-primary { background: var(--accent); color: #04101f; }
.btn-primary:hover { background: #6bb0ff; }
.btn-ghost { background: var(--surface2); color: var(--text); border: 1px solid var(--border); }
.btn-ghost:hover { border-color: var(--accent-dim); }

.stat-row { display: flex; gap: 28px; justify-content: center; margin: 36px 0 8px; flex-wrap: wrap; }
.stat { text-align: center; }
.stat .n { font-family: var(--mono); font-size: 1.7rem; font-weight: 700; color: var(--accent); }
.stat .l { font-size: 12.5px; color: var(--muted); text-transform: uppercase; letter-spacing: .04em; }

/* ── Samples gallery ─────────────────────────────────────────────────── */
.sample-card { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; overflow: hidden; }
.sample-preview { aspect-ratio: 4/3; background: #05070f; display: flex; align-items: center; justify-content: center; overflow: hidden; }
.sample-preview img, .sample-preview svg { max-width: 100%; max-height: 100%; }
.sample-meta { padding: 14px 16px; }
.sample-meta .name { font-family: var(--mono); font-weight: 700; color: var(--text); font-size: 13.5px; }
.sample-meta .badges { margin-top: 6px; display: flex; gap: 6px; flex-wrap: wrap; }
.badge { font-size: 10.5px; padding: 2px 7px; border-radius: 4px; background: var(--surface2); color: var(--muted); font-family: var(--mono); }
.badge.accent { color: var(--accent); }

/* ── Playground ──────────────────────────────────────────────────────── */
.pg-shell { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: var(--border); border: 1px solid var(--border); border-radius: 10px; overflow: hidden; height: 72vh; min-height: 480px; }
.pg-pane { background: var(--bg-raised); display: flex; flex-direction: column; min-width: 0; }
.pg-pane-header { padding: 10px 16px; border-bottom: 1px solid var(--border); font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: .05em; color: var(--muted); display: flex; align-items: center; justify-content: space-between; }
#pg-editor { flex: 1; width: 100%; background: var(--bg-raised); color: var(--text); border: none; resize: none; font-family: var(--mono); font-size: 13.5px; line-height: 1.6; padding: 16px; outline: none; }
.pg-preview-body { flex: 1; display: flex; align-items: center; justify-content: center; overflow: auto; padding: 16px; background: repeating-conic-gradient(#0d1424 0% 25%, #0a0f1e 0% 50%) 50%/16px 16px; }
.pg-preview-body svg { max-width: 100%; max-height: 100%; }
.pg-status { font-size: 12px; font-family: var(--mono); }
.pg-status.ok { color: var(--good); }
.pg-status.err { color: var(--bad); }
.pg-status.loading { color: var(--muted); }
.pg-examples { display: flex; gap: 6px; flex-wrap: wrap; margin: 14px 0 0; }
.pg-examples button { font: inherit; font-size: 12px; padding: 5px 10px; border-radius: 6px; background: var(--surface2); border: 1px solid var(--border); color: var(--muted); cursor: pointer; }
.pg-examples button:hover { color: var(--text); border-color: var(--accent-dim); }
.pg-error-pane { padding: 16px; font-family: var(--mono); font-size: 12.5px; color: var(--bad); white-space: pre-wrap; overflow: auto; }

footer.site-footer { border-top: 1px solid var(--border); padding: 28px 24px; text-align: center; color: var(--muted); font-size: 13px; }
footer.site-footer a { color: var(--muted); }
footer.site-footer a:hover { color: var(--accent); }
"""


def nav(active: str) -> str:
    items = [
        ("home", "/", "MSX"),
        ("docs", "/docs/", "Docs"),
        ("samples", "/samples/", "Samples"),
        ("playground", "/playground/", "Playground"),
        ("report", "/report/", "Build Report"),
    ]
    links = []
    for key, href, label in items:
        if key == "home":
            continue
        cls = "nav-link active" if key == active else "nav-link"
        links.append(f'<a class="{cls}" href="{href}">{label}</a>')
    return f'''<nav class="site-nav">
  <a class="brand" href="/">M<span>S</span>X</a>
  <div class="links">{''.join(links)}</div>
  <a class="gh-link" href="https://github.com/Mid-D-Man/msx" target="_blank" rel="noopener">github.com/Mid-D-Man/msx</a>
</nav>'''


def page(title: str, active: str, body: str, wide: bool = False, extra_head: str = "", extra_scripts: str = "") -> str:
    page_cls = "page wide" if wide else "page"
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — MSX</title>
<meta name="description" content="MSX — a DixScript-based vector graphics format with SVG, CPU and GPU renderers.">
<style>{CSS}</style>
{extra_head}
</head>
<body>
{nav(active)}
<main class="{page_cls}">
{body}
</main>
<footer class="site-footer">
  MSX — MidStroke eXtension · <a href="https://github.com/Mid-D-Man/msx">Source on GitHub</a> · <a href="/report/">Latest build report</a>
</footer>
{extra_scripts}
</body>
</html>"""
