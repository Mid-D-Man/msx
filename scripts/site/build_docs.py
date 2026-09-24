#!/usr/bin/env python3
"""
build_docs.py
Renders the docs section from REAL repo markdown (README.md,
docs/format-spec.md) — deliberately not hand-authored page prose, so the
site can never drift from what those files actually say the way
docs/format-spec.md itself drifted from the real parser source once
already (see that file's own history). If the markdown changes, the next
build picks it up automatically.
"""

from mdconvert import convert
from templates import page

DOC_PAGES = [
    {"slug": "", "source": "README.md", "nav_label": "Getting Started", "group": "Guide"},
    {"slug": "format-spec/", "source": "docs/format-spec.md", "nav_label": "Format Specification", "group": "Reference"},
]


def _sidenav(active_slug: str) -> str:
    groups = {}
    for d in DOC_PAGES:
        groups.setdefault(d["group"], []).append(d)
    out = ['<nav class="docs-sidenav">']
    for group, items in groups.items():
        out.append(f'<div class="group-title">{group}</div>')
        for d in items:
            href = f"/docs/{d['slug']}"
            cls = "active" if d["slug"] == active_slug else ""
            out.append(f'<a class="{cls}" href="{href}">{d["nav_label"]}</a>')
    out.append("</nav>")
    return "\n".join(out)


def _toc_rail(toc: list) -> str:
    if not toc:
        return ""
    out = ['<nav class="docs-toc"><div class="toc-title">On this page</div>']
    for entry in toc:
        cls = "toc-h3" if entry["level"] == 3 else ""
        out.append(f'<a class="{cls}" href="#{entry["id"]}">{entry["text"]}</a>')
    out.append("</nav>")
    return "\n".join(out)


def build_doc_page(repo_root, doc: dict) -> tuple[str, str]:
    """Returns (output_relative_path, html)."""
    src_path = repo_root / doc["source"]
    md = src_path.read_text(errors="replace")
    body_html, toc = convert(md)

    content = f'''<div class="docs-layout">
{_sidenav(doc["slug"])}
<div class="docs-content">
{body_html}
</div>
{_toc_rail(toc)}
</div>'''

    html = page(title=doc["nav_label"], active="docs", body=content, wide=True)
    out_path = f"docs/{doc['slug']}index.html"
    return out_path, html


def build_all(repo_root):
    return [build_doc_page(repo_root, d) for d in DOC_PAGES]


if __name__ == "__main__":
    import pathlib
    root = pathlib.Path(".")
    for out_path, html_content in build_all(root):
        print(f"=== {out_path} ({len(html_content)} bytes) ===")
