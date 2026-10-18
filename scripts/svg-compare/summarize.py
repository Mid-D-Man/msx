import json, sys, collections
res = json.load(open(sys.argv[1]))
limit = int(sys.argv[2]) if len(sys.argv) > 2 else 40
def status(r):
    if r["status"] != "ok": return r["status"]
    sc = r.get("scores", {})
    a, b = sc.get("resvg"), sc.get("rsvg")
    if a is not None:
        if a < 0.04: return "match"
        if b is not None and b < 0.02: return "disputed"   # only rsvg agrees
        return "close" if a < 0.10 else "differs"
    return "match" if b < 0.02 else ("close" if b < 0.10 else "differs")
cnt = collections.Counter(status(r) for r in res)
print("TOTAL", len(res), dict(cnt))
by = collections.defaultdict(collections.Counter)
for r in res: by[r["group"]][status(r)] += 1
for g, c in sorted(by.items()):
    print(f"  {g:34s} {sum(c.values()):4d}  " + " ".join(f"{k}={v}" for k, v in sorted(c.items())))
bad = [r for r in res if status(r) in ("differs", "close", "error", "timeout", "noref", "size")]
silent = [r for r in bad if not r["warnings"]]
print(f"\nnot matching: {len(bad)}; silent (no importer warning): {len(silent)}")
for r in sorted(silent, key=lambda r: -min(r.get("scores", {"x": 1}).values()))[:limit]:
    print(f"  {status(r):8s} {str(r.get('scores', r.get('detail',''))):44s} {r['svg'].split('tests/')[-1]}")
