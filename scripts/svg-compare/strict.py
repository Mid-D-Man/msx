#!/usr/bin/env python3
"""Strict comparison of one resvg test group, for features that cover few pixels.

compare.py counts a file as matching when under 2 percent of its pixels differ
strongly, which hides small features such as markers. This script reports the
share of strongly different pixels against the resvg reference image with a
much lower limit.

Usage: strict.py <group> [limit]        (group such as painting/marker, limit default 0.002)
Environment: MSX_BIN, RESVG_TESTS as for compare.py.
"""
import glob, os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from compare import MSX, run_msx, white, arr, strong_frac  # noqa: E402
from PIL import Image

root = os.environ.get("RESVG_TESTS", "/tmp/resvg-tests/crates/resvg/tests/tests")
group = sys.argv[1]
limit = float(sys.argv[2]) if len(sys.argv) > 2 else 0.002
bad, total = [], 0
with tempfile.TemporaryDirectory() as tmp:
    for svg in sorted(glob.glob(f"{root}/{group}/*.svg")):
        png = svg[:-4] + ".png"
        out = os.path.join(tmp, "o.png")
        if os.path.exists(out):
            os.remove(out)
        rc, warnings, _ = run_msx(svg, out)
        if rc != 0 or not os.path.exists(out) or not os.path.exists(png):
            continue
        mine = white(Image.open(out))
        ref = white(Image.open(png))
        if ref.size != mine.size:
            ref = ref.resize(mine.size, Image.LANCZOS)
        strong, _ = strong_frac(arr(mine), arr(ref))
        total += 1
        if strong > limit:
            bad.append((strong, os.path.basename(svg), warnings[:1]))
print(f"{group}: {total - len(bad)} of {total} within {limit}")
for strong, name, w in sorted(bad, reverse=True):
    print(f"  {strong:.4f}  {name}  {'WARN ' + w[0][:50] if w else ''}")
