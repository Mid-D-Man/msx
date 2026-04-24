#!/usr/bin/env bash
# gen_corpus.sh — generate synthetic MSX test corpus
set -euo pipefail

BIN=./target/release/msx
[ -f "$BIN" ] || cargo build --release
mkdir -p corpus/synthetic

echo 'Generating MSX synthetic corpus...'

for COUNT in 10 50 100 500 2000; do
  OUT="corpus/synthetic/circles_${COUNT}.msx"
  python3 - "$OUT" <<PYEOF
import random, sys
random.seed(int('${COUNT}'))
n = int('${COUNT}')
lines = ['@CONFIG( version -> "1.0.0" )']
lines.append('@DATA(')
lines.append('  scene: { width = 1000, height = 1000, background = "#ffffff" }')
lines.append('  elements::')
for _ in range(n):
    x = random.randint(5, 990)
    y = random.randint(5, 990)
    r = random.randint(3, 40)
    c = f'#{random.randint(0, 0xFFFFFF):06x}'
    o = round(random.uniform(0.4, 1.0), 2)
    lines.append(f'    {{ type = "circle", cx = {x}, cy = {y}, r = {r}, style = {{ fill = "{c}", stroke = "none", stroke_width = 0, opacity = {o} }} }}')
lines.append(')')
with open(sys.argv[1], 'w') as f:
    f.write('\n'.join(lines) + '\n')
print(f'Wrote {sys.argv[1]}')
PYEOF
done

echo 'Corpus ready in corpus/synthetic/'
