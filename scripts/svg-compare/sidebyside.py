import sys, subprocess, os
from PIL import Image, ImageDraw
import numpy as np
MSX = os.environ.get("MSX_BIN", "target/debug/msx")
def white(p):
    im = Image.open(p).convert("RGBA"); bg = Image.new("RGBA", im.size, (255,255,255,255))
    return Image.alpha_composite(bg, im).convert("RGB")
R = os.environ.get("RESVG_TESTS", "/tmp/resvg-tests/crates/resvg/tests/tests") + "/"
tiles = []
for rel in sys.argv[1:]:
    svg = R + rel + ".svg"; out = "/tmp/sbs/o.png"; ref = "/tmp/sbs/r.png"
    for f in (out, ref):
        if os.path.exists(f): os.remove(f)
    subprocess.run([MSX, "rasterize", svg, "-o", out], capture_output=True)
    subprocess.run(["rsvg-convert", svg, "-o", ref], capture_output=True)
    a = white(out); b = white(ref) if os.path.exists(ref) else Image.new("RGB", a.size, (200, 0, 200))
    if b.size != a.size: b = b.resize(a.size)
    d = Image.fromarray((255 - np.abs(np.asarray(a).astype(int) - np.asarray(b).astype(int)).max(axis=2)).astype("uint8")).convert("RGB")
    w, h = a.size; tile = Image.new("RGB", (w*3 + 20, h + 14), (60, 60, 60))
    tile.paste(a, (0, 14)); tile.paste(b, (w + 10, 14)); tile.paste(d, (2*w + 20, 14))
    ImageDraw.Draw(tile).text((2, 1), rel.split("/")[-1][:60] + "   msx | reference | diff", fill=(255, 255, 255))
    tiles.append(tile)
W = max(t.width for t in tiles); H = sum(t.height + 6 for t in tiles)
sheet = Image.new("RGB", (W, H), (30, 30, 30)); y = 0
for t in tiles: sheet.paste(t, (0, y)); y += t.height + 6
sheet.save("/tmp/sbs/sheet.png"); print(sheet.size)
