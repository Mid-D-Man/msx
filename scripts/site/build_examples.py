#!/usr/bin/env python3
"""
build_examples.py
Publishes the files in examples/ to the playground.

The playground loads an example when a visitor picks it, so each file is
copied next to the page as a plain text file and described by one
`manifest.json`:

    site/playground/examples/manifest.json
    site/playground/examples/<file>.msx

The descriptions come from examples/index.json (categories, titles, one
line each, an optional note). A `.msx` file that the index does not list is
still published, under "More", so dropping a file into examples/ is enough
to see it in the playground; `tests/examples.rs` is what insists that the
index names every file.

An index entry that cannot be used (a missing file, a duplicate id, an
unknown category) stops the build with a message, because a broken manifest
would leave the playground's example list empty.

Usage (also called by build_site.py):
    python3 build_examples.py --repo-root . --out site/playground/examples
"""

import argparse
import json
import pathlib
import re
import sys

MAX_EXAMPLE_BYTES = 200_000
_ANIMATED = re.compile(r"^\s*animations\s*::", re.MULTILINE)
UNLISTED_CATEGORY = "more"


def _stem(name: str) -> str:
    return name[:-4] if name.endswith(".msx") else name


def _title_from(stem: str) -> str:
    words = re.sub(r"[_\-]+", " ", re.sub(r"\(\d+\)$", "", stem)).strip()
    return words[:1].upper() + words[1:]


def collect(repo_root: pathlib.Path):
    """Returns (manifest dict, {file name: source text}) for the repository's examples."""
    ex_dir = repo_root / "examples"
    index_path = ex_dir / "index.json"
    index = json.loads(index_path.read_text(encoding="utf-8")) if index_path.exists() else {}

    categories = list(index.get("categories", []))
    cat_ids = [c["id"] for c in categories]
    if len(set(cat_ids)) != len(cat_ids):
        raise ValueError(f"{index_path}: duplicate category ids")
    hidden = set(index.get("hidden", []))

    on_disk = sorted(p.name for p in ex_dir.glob("*.msx"))
    entries, seen_ids, seen_files = [], set(), set()
    for raw in index.get("examples", []):
        for key in ("file", "title", "category", "description"):
            if not str(raw.get(key, "")).strip():
                raise ValueError(f"{index_path}: an entry has no {key!r}: {raw}")
        file = raw["file"]
        if file not in on_disk:
            raise ValueError(f"{index_path}: {file!r} is listed but is not in examples/")
        if file in seen_files:
            raise ValueError(f"{index_path}: {file!r} is listed twice")
        if raw["category"] not in cat_ids:
            raise ValueError(f"{index_path}: {file!r} has the unknown category {raw['category']!r}")
        ex_id = raw.get("id") or _stem(file)
        if ex_id in seen_ids:
            raise ValueError(f"{index_path}: the id {ex_id!r} is used twice")
        seen_files.add(file)
        seen_ids.add(ex_id)
        entries.append({"id": ex_id, "file": file, "title": raw["title"], "category": raw["category"],
                        "description": raw["description"], **({"note": raw["note"]} if raw.get("note") else {})})

    unlisted = [f for f in on_disk if f not in seen_files and f not in hidden]
    if unlisted:
        print(f"  note: not in examples/index.json, shown under More: {', '.join(unlisted)}", file=sys.stderr)
        if UNLISTED_CATEGORY not in cat_ids:
            categories.append({"id": UNLISTED_CATEGORY, "title": "More"})
        for f in unlisted:
            ex_id = _stem(f)
            if ex_id in seen_ids:
                continue
            seen_ids.add(ex_id)
            entries.append({"id": ex_id, "file": f, "title": _title_from(ex_id), "category": UNLISTED_CATEGORY,
                            "description": "Not described yet."})

    files = {}
    for e in entries:
        text = (ex_dir / e["file"]).read_text(encoding="utf-8", errors="replace")
        if len(text.encode("utf-8")) > MAX_EXAMPLE_BYTES:
            print(f"  warning: {e['file']} is over {MAX_EXAMPLE_BYTES} bytes", file=sys.stderr)
        e["animated"] = bool(_ANIMATED.search(text))
        files[e["file"]] = text

    # Only categories that have an example, in the index's order.
    used = {e["category"] for e in entries}
    manifest = {
        "version": 1,
        "categories": [c for c in categories if c["id"] in used],
        "examples": entries,
    }
    return manifest, files


def write(repo_root: pathlib.Path, out_dir: pathlib.Path) -> int:
    manifest, files = collect(repo_root)
    out_dir.mkdir(parents=True, exist_ok=True)
    for name, text in files.items():
        (out_dir / name).write_text(text, encoding="utf-8")
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")
    return len(files)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo-root", default=".")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    n = write(pathlib.Path(args.repo_root), pathlib.Path(args.out))
    print(f"  wrote {n} example(s) and manifest.json to {args.out}")


if __name__ == "__main__":
    main()
