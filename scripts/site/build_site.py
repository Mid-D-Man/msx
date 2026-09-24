#!/usr/bin/env python3
"""
build_site.py
Orchestrates the whole site build: home, docs (from real markdown),
samples (from examples.json — the CI pipeline's own existing output,
unchanged), and the playground shell (the compiled WASM module itself is
built by a separate step in .github/workflows/cloudflare-pages.yml and
copied into site/playground/ alongside this page — this script only
emits the HTML/JS wrapper around it, never touches Rust or wasm-pack).

Usage:
    python3 build_site.py --repo-root . --out site/ [--examples examples.json] [--stats stats.json]

`--examples` / `--stats` are optional: this script must still produce a
usable site (docs, home, playground, empty samples list) when a fresh
checkout has no CI artifacts yet to hand — never hard-fail the whole
site build over one missing corpus file, the same reasoning
generate_report.py's own optional inputs already follow.
"""

import argparse
import json
import pathlib
import shutil
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))

from build_docs import build_all as build_all_docs
from build_home import build_home_page
from build_playground import build_playground_page
from build_samples import build_samples_page


def _write(out_dir: pathlib.Path, rel_path: str, content: str):
    p = out_dir / rel_path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    print(f"  wrote {rel_path} ({len(content)} bytes)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo-root", default=".", help="Repo root, for reading README.md / docs/format-spec.md")
    ap.add_argument("--out", required=True, help="Output site directory")
    ap.add_argument("--examples", default=None, help="Path to examples.json from the CI corpus step")
    ap.add_argument("--stats", default=None, help="Optional JSON with {tests_passed, example_count, element_tags} for the home page stat row")
    args = ap.parse_args()

    repo_root = pathlib.Path(args.repo_root)
    out_dir = pathlib.Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    print("Building docs pages...")
    for rel_path, html in build_all_docs(repo_root):
        _write(out_dir, rel_path, html)

    print("Building home page...")
    stats = {}
    if args.stats:
        stats_path = pathlib.Path(args.stats)
        if stats_path.exists():
            stats = json.loads(stats_path.read_text())
        else:
            print(f"  note: --stats {stats_path} not found, home page stat row will be empty", file=sys.stderr)
    _write(out_dir, "index.html", build_home_page(stats))

    print("Building samples page...")
    examples = []
    if args.examples:
        examples_path = pathlib.Path(args.examples)
        if examples_path.exists():
            examples = json.loads(examples_path.read_text())
        else:
            print(f"  note: --examples {examples_path} not found, samples page will be empty", file=sys.stderr)
    _write(out_dir, "samples/index.html", build_samples_page(examples))

    print("Building playground page...")
    _write(out_dir, "playground/index.html", build_playground_page())

    static_src = pathlib.Path(__file__).parent / "static"
    if static_src.exists():
        print("Copying static assets...")
        for f in static_src.rglob("*"):
            if f.is_file():
                rel = f.relative_to(static_src)
                dest = out_dir / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(f, dest)
                print(f"  copied {rel}")

    print(f"\nSite built at {out_dir}/ — {sum(1 for _ in out_dir.rglob('*.html'))} pages.")


if __name__ == "__main__":
    main()
