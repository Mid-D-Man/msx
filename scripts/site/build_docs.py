#!/usr/bin/env python3
"""
build_docs.py
Builds the docs section: a hub page, the guide pages written in
docs/guide/*.md, and the full specification rendered from
docs/format-spec.md.

Page order, grouping and summaries live in GROUPS and PAGES below. Guide
pages embed real files from examples/ through `<!-- example: name -->`
directives: the source is read from the repository and the rendered image
comes from the CI corpus (examples.json) when one is available, so an
example on a docs page cannot drift from the file it shows.
"""

import html
import pathlib
import re
import sys

from mdconvert import convert
from templates import page

GROUPS = [
    ("start", "Start here",
     "What MSX is, how to run it, and how a scene is put together."),
    ("author", "Authoring",
     "Source files, the canvas, styling and transforms."),
    ("elements", "Elements",
     "Every drawable element type and the fields it accepts."),
    ("defs", "Defs",
     "Gradients and shaders, referenced by id from fills and strokes."),
    ("animation", "Animation",
     "Keyframe tracks, easing and loop modes."),
    ("internals", "Internals",
     "The renderers and the binary format."),
    ("reference", "Reference",
     "Command line usage and the complete specification."),
]

# (group, slug, title, summary). Guide pages read docs/guide/<slug>.md;
# the full specification reads docs/format-spec.md.
PAGES = [
    ("start", "introduction", "Introduction",
     "What MSX is, the two layers of the format, and the path from source to pixels."),
    ("start", "quick-start", "Quick start",
     "Build the tools, render a first scene, compile it to binary and check the roundtrip."),
    ("start", "scene-anatomy", "How a scene fits together",
     "Canvas, defs, elements and animations, how ids connect them, and the coordinate system."),
    ("author", "source-files", "Source files and DixScript",
     "File sections, QuickFuncs, enums, interpolated strings and color literals."),
    ("author", "canvas", "Canvas and viewbox",
     "Scene size, background color and the optional viewbox."),
    ("author", "style-and-paint", "Style and paint",
     "Style keys, defaults, paint values and which renderer honors what."),
    ("author", "transforms", "Transforms",
     "The three accepted transform shapes and the order a chain applies in."),
    ("elements", "elements-overview", "Element overview",
     "All fourteen element types at a glance, with required fields and renderer support."),
    ("elements", "shapes", "Basic shapes",
     "rect, circle, ellipse and line."),
    ("elements", "paths", "Paths, polylines and polygons",
     "Path data, point lists and closed shapes."),
    ("elements", "text", "Text",
     "The text element, its style keys, and where it renders."),
    ("elements", "groups-and-use", "Groups and use",
     "Grouping, shared transforms and reusing an element by id."),
    ("elements", "layers", "Layers, blend modes and effects",
     "Isolated compositing buffers, blend modes, effects, clipping and paint order."),
    ("elements", "sdf", "Signed distance fields",
     "Shape trees built from primitives and boolean operations, evaluated per pixel."),
    ("elements", "splats", "Gaussian splats",
     "Soft elliptical blobs with a Gaussian falloff."),
    ("elements", "images-and-audio", "Images and audio",
     "Embedded or referenced image elements and audio defs."),
    ("defs", "gradients", "Gradients",
     "Linear, radial and conic gradients, stops and bounding-box units."),
    ("defs", "shaders", "Shaders",
     "WGSL fragment shaders as fills, uniforms, the time clock and the fallback color."),
    ("animation", "animation", "Keyframe animation",
     "Tracks, properties, easing, loop modes and how deltas compose with static transforms."),
    ("internals", "rendering-backends", "Rendering backends",
     "What the SVG, CPU and GPU renderers each support today."),
    ("internals", "binary-format", "Binary format",
     "The compiled layer: header, string pool, tags, compression and versioning."),
    ("reference", "cli", "Command line reference",
     "Every msx subcommand with its flags."),
    ("reference", "format-spec", "Full specification",
     "The complete format reference, rendered from docs/format-spec.md."),
]

SLUGS = {p[1] for p in PAGES}
GITHUB_BLOB = "https://github.com/Mid-D-Man/msx/blob/main/"


def _href(slug: str) -> str:
    return f"/docs/{slug}/"


def _source_path(slug: str) -> str:
    return "docs/format-spec.md" if slug == "format-spec" else f"docs/guide/{slug}.md"


def link_rewrite(url: str) -> str:
    """Relative `page.md#anchor` links in guide files become site URLs."""
    if re.match(r"^(https?:|mailto:|#|/)", url):
        return url
    m = re.match(r"^(?:\.\./)?([a-z0-9-]+)\.md(#.*)?$", url)
    if m and m.group(1) in SLUGS:
        return _href(m.group(1)) + (m.group(2) or "")
    m = re.match(r"^\.\./(.+)$", url)
    if m:
        return GITHUB_BLOB + "docs/" + m.group(1)
    print(f"  warning: unresolved docs link {url!r}", file=sys.stderr)
    return url


# ── Navigation ──────────────────────────────────────────────────────────────

def _nav_groups(active_slug: str, open_all: bool = False) -> str:
    active_group = next((p[0] for p in PAGES if p[1] == active_slug), None)
    out = []
    for key, label, _blurb in GROUPS:
        items = [p for p in PAGES if p[0] == key]
        is_open = open_all or key == active_group
        out.append(f'<details class="nav-group"{" open" if is_open else ""}>')
        out.append(f"<summary>{html.escape(label)}</summary>")
        for _g, slug, title, _s in items:
            cls = ' class="active"' if slug == active_slug else ""
            out.append(f'<a{cls} href="{_href(slug)}">{html.escape(title)}</a>')
        out.append("</details>")
    return "\n".join(out)


def _sidenav(active_slug: str) -> str:
    return (
        '<nav class="docs-sidenav" aria-label="Documentation">'
        '<a class="nav-home" href="/docs/">Documentation home</a>'
        f"{_nav_groups(active_slug)}</nav>"
    )


def _mobile_nav(active_slug: str, toc: list) -> str:
    current = next((p[2] for p in PAGES if p[1] == active_slug), "Documentation")
    out = [
        '<details class="docs-mobile">',
        f"<summary>Docs menu: {html.escape(current)}</summary>",
        '<a class="nav-home" href="/docs/">Documentation home</a>',
        _nav_groups(active_slug),
        "</details>",
    ]
    if toc:
        out.append('<details class="docs-mobile">')
        out.append("<summary>On this page</summary>")
        for e in toc:
            cls = ' class="toc-h3"' if e["level"] == 3 else ""
            out.append(f'<a{cls} href="#{e["id"]}">{_plain(e["text"])}</a>')
        out.append("</details>")
    return "\n".join(out)


def _plain(text: str) -> str:
    return html.escape(text.replace("`", ""))


def _toc_rail(toc: list) -> str:
    if not toc:
        return ""
    out = ['<nav class="docs-toc" aria-label="On this page"><div class="toc-title">On this page</div>']
    for e in toc:
        cls = "toc-h3" if e["level"] == 3 else ""
        out.append(f'<a class="{cls}" href="#{e["id"]}">{_plain(e["text"])}</a>')
    out.append("</nav>")
    return "\n".join(out)


def _pager(slug: str) -> str:
    idx = next((i for i, p in enumerate(PAGES) if p[1] == slug), None)
    if idx is None:
        return ""
    parts = ['<nav class="doc-pager" aria-label="Previous and next page">']
    if idx > 0:
        _g, s, t, _ = PAGES[idx - 1]
        parts.append(f'<a class="prev" href="{_href(s)}"><span>Previous</span>{html.escape(t)}</a>')
    else:
        parts.append("<span></span>")
    if idx < len(PAGES) - 1:
        _g, s, t, _ = PAGES[idx + 1]
        parts.append(f'<a class="next" href="{_href(s)}"><span>Next</span>{html.escape(t)}</a>')
    parts.append("</nav>")
    return "".join(parts)


# ── Embedded examples ───────────────────────────────────────────────────────

def _example_image(ex: dict, animated: bool) -> str:
    """The most faithful image for an example. A shader example shows its
    GPU render, since only the GPU renderer executes the WGSL."""
    name = html.escape(ex.get("name", ""))
    shader = bool(ex.get("uses_shader"))
    if animated:
        order = [("gpu_gif_base64", "gif"), ("anim_gif_base64", "gif")] if shader else [("anim_gif_base64", "gif"), ("gpu_gif_base64", "gif")]
    else:
        order = []
    if shader:
        order += [("gpu_png_base64", "png"), ("png_base64", "png")]
    else:
        order += [("png_base64", "png")]
    for key, kind in order:
        if ex.get(key):
            return f'<img src="data:image/{kind};base64,{ex[key]}" alt="{name} rendered">'
    return ""


def make_directive(repo_root: pathlib.Path, examples: list):
    by_name = {e.get("name"): e for e in (examples or [])}

    def directive(kind: str, arg: str) -> str:
        if kind != "example":
            print(f"  warning: unknown docs directive {kind!r}", file=sys.stderr)
            return ""
        parts = [p.strip() for p in arg.split("|")]
        name, opts = parts[0], set(parts[1:])
        src_file = repo_root / "examples" / f"{name}.msx"
        if not src_file.exists():
            print(f"  warning: docs example {name!r} not found at {src_file}", file=sys.stderr)
            return f'<div class="callout callout-warning"><p>The example <code>{html.escape(name)}</code> is missing from the repository.</p></div>'
        source = src_file.read_text(errors="replace").rstrip("\n")
        line_count = source.count("\n") + 1
        img = _example_image(by_name[name], "animated" in opts) if name in by_name else ""
        media = f'<div class="doc-example-media">{img}</div>' if img else ""
        open_attr = " open" if line_count <= 30 else ""
        caption = (
            f'<figcaption><code>examples/{html.escape(name)}.msx</code> · {line_count} lines · '
            f'<a href="/samples/">all samples</a></figcaption>'
        )
        if "nosource" in opts:
            return f'<figure class="doc-example">{media}{caption}</figure>'
        return (
            '<figure class="doc-example">'
            f"{media}"
            f"{caption}"
            f'<details class="doc-example-src"{open_attr}>'
            f"<summary>Source</summary>"
            f'<pre class="code lang-dixscript"><code>{html.escape(source)}</code></pre>'
            "</details></figure>"
        )

    return directive


# ── Pages ───────────────────────────────────────────────────────────────────

def _layout(slug: str, body_html: str, toc: list) -> str:
    return (
        '<div class="docs-layout">\n'
        f"{_sidenav(slug)}\n"
        '<div class="docs-content">\n'
        f"{_mobile_nav(slug, toc)}\n"
        f"{body_html}\n"
        f"{_pager(slug)}\n"
        "</div>\n"
        f"{_toc_rail(toc)}\n"
        "</div>"
    )


def build_guide_page(repo_root: pathlib.Path, group: str, slug: str, title: str, summary: str, directive):
    src_path = repo_root / _source_path(slug)
    md = src_path.read_text(errors="replace")
    body_html, toc = convert(md, link_rewrite=link_rewrite, directive=directive)
    if slug == "format-spec":
        note = (
            '<div class="callout callout-note"><p>This page is the complete reference, rendered from '
            f'<a href="{GITHUB_BLOB}docs/format-spec.md"><code>docs/format-spec.md</code></a>. '
            "The guide pages explain the concepts and link here for exact byte layouts.</p></div>"
        )
        body_html = body_html.replace("</h1>", "</h1>\n" + note, 1)
    else:
        lede = f'<p class="lede">{html.escape(summary)}</p>'
        body_html = body_html.replace("</h1>", "</h1>\n" + lede, 1)
    return f"docs/{slug}/index.html", page(title=title, active="docs", body=_layout(slug, body_html, toc), wide=True)


def build_hub_page() -> tuple[str, str]:
    cards = []
    for key, label, blurb in GROUPS:
        cards.append(f'<h2 id="{key}">{html.escape(label)}</h2>')
        cards.append(f"<p>{html.escape(blurb)}</p>")
        cards.append('<div class="card-grid">')
        for _g, slug, title, summary in [p for p in PAGES if p[0] == key]:
            cards.append(
                f'<a class="card" href="{_href(slug)}"><h3>{html.escape(title)}</h3>'
                f"<p>{html.escape(summary)}</p></a>"
            )
        cards.append("</div>")
    intro = (
        "<h1>Documentation</h1>"
        '<p class="lede">MSX is a vector graphics format with a DixScript source layer and a compact binary layer. '
        "These pages explain how to write scenes, what each element and def does, and what each renderer supports.</p>"
        '<div class="hub-actions">'
        f'<a class="btn btn-primary" href="{_href("quick-start")}">Quick start</a>'
        '<a class="btn btn-ghost" href="/playground/">Open the playground</a>'
        '<a class="btn btn-ghost" href="/samples/">Browse samples</a>'
        "</div>"
    )
    toc = [{"level": 2, "id": k, "text": label} for k, label, _ in GROUPS]
    return "docs/index.html", page(
        title="Documentation", active="docs", body=_layout("", intro + "\n".join(cards), toc), wide=True
    )


def build_all(repo_root, examples=None):
    repo_root = pathlib.Path(repo_root)
    directive = make_directive(repo_root, examples or [])
    pages = [build_hub_page()]
    for group, slug, title, summary in PAGES:
        if not (repo_root / _source_path(slug)).exists():
            print(f"  warning: docs source {_source_path(slug)} not found, page skipped", file=sys.stderr)
            continue
        pages.append(build_guide_page(repo_root, group, slug, title, summary, directive))
    return pages


if __name__ == "__main__":
    for out_path, html_content in build_all(pathlib.Path(".")):
        print(f"=== {out_path} ({len(html_content)} bytes) ===")
