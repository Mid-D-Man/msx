#!/usr/bin/env python3
"""Compare MSX's import + CPU render of SVG files against reference renderers.

Usage: compare.py icons <folder-of-svgs> <out.json>
       compare.py resvg x <out.json>      (needs a checkout of the resvg tests)
Environment: MSX_BIN (the msx binary), RESVG_TESTS (resvg crates/resvg/tests/tests).
"""
import json, os, subprocess, sys, tempfile, glob
import numpy as np
from PIL import Image

MSX = os.environ.get("MSX_BIN", "target/debug/msx")

def white(im):
    im = im.convert("RGBA")
    bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
    return Image.alpha_composite(bg, im).convert("RGB")

def arr(im): return np.asarray(im).astype(np.int16)

def strong_frac(a, b):
    return float((np.abs(a - b).max(axis=2) > 40).mean()), float(np.abs(a - b).mean())

def run_msx(svg, out):
    p = subprocess.run([MSX, "rasterize", svg, "-o", out], capture_output=True, text=True, timeout=30)
    warnings = [l[len("warning: "):] for l in p.stderr.splitlines() if l.startswith("warning: ")]
    return p.returncode, warnings, (p.stderr + p.stdout).strip().splitlines()[-1:]

def compare(svg, resvg_png, tmp):
    out = os.path.join(tmp, "out.png"); ref = os.path.join(tmp, "ref.png")
    for f in (out, ref):
        if os.path.exists(f): os.remove(f)
    try:
        rc, warnings, tail = run_msx(svg, out)
    except subprocess.TimeoutExpired:
        return {"svg": svg, "status": "timeout", "warnings": []}
    if rc != 0 or not os.path.exists(out):
        return {"svg": svg, "status": "error", "warnings": warnings, "detail": tail}
    mine = white(Image.open(out)); a = arr(mine)
    scores = []
    try:
        subprocess.run(["rsvg-convert", svg, "-o", ref], capture_output=True, timeout=30)
    except subprocess.TimeoutExpired:
        pass
    try:
        if os.path.exists(ref) and os.path.getsize(ref) > 0:
            r = white(Image.open(ref))
            if r.size == mine.size:
                scores.append(("rsvg",) + strong_frac(a, arr(r)))
    except Exception:
        pass
    try:
        if resvg_png and os.path.exists(resvg_png):
            r = white(Image.open(resvg_png))
            if r.size != mine.size and abs(r.size[0] / mine.size[0] - r.size[1] / mine.size[1]) < 0.02:
                r = r.resize(mine.size, Image.LANCZOS)
            if r.size == mine.size:
                scores.append(("resvg",) + strong_frac(a, arr(r)))
    except Exception:
        pass
    if not scores:
        return {"svg": svg, "status": "noref", "warnings": warnings, "detail": f"{mine.size}"}
    best = min(scores, key=lambda s: s[1])
    return {"svg": svg, "status": "ok", "warnings": warnings, "strong": best[1], "mean": best[2], "ref": best[0],
            "scores": {s[0]: s[1] for s in scores}}

def main():
    mode = sys.argv[1]; results = []
    with tempfile.TemporaryDirectory() as tmp:
        if mode == "resvg":
            root = os.environ.get("RESVG_TESTS", "/tmp/resvg-tests/crates/resvg/tests/tests")
            for f in sorted(glob.glob(f"{root}/*/*/*.svg")):
                r = compare(f, f[:-4] + ".png", tmp)
                rel = os.path.relpath(f, root).split("/")
                r["group"] = rel[0] + "/" + rel[1]
                results.append(r)
        else:
            for f in sorted(glob.glob(sys.argv[2] + "/*.svg")):
                r = compare(f, None, tmp)
                r["group"] = os.path.basename(f).split("-")[0]
                results.append(r)
    json.dump(results, open(sys.argv[3], "w"))
    print("compared", len(results))

if __name__ == "__main__":
    main()
