#!/usr/bin/env bash
# verify_corpus.sh — roundtrip every .msx in corpus/ through binary and back
set -euo pipefail

BIN=./target/release/msx
[ -f "$BIN" ] || cargo build --release

PASS=0; FAIL=0

for src in corpus/**/*.msx; do
  FILE=$(basename "${src%.msx}")
  if "$BIN" roundtrip "$src" 2>/dev/null; then
    echo "PASS  $FILE"
    PASS=$((PASS + 1))
  else
    echo "FAIL  $FILE"
    FAIL=$((FAIL + 1))
  fi
done

echo
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
