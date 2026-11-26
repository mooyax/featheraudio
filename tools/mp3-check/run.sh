#!/bin/sh
# Compares featheraudio's MP3 decoder (a port of minimp3) with minimp3 itself (without SIMD,
# float output, through mp3dec_load_buf): every file given must decode to exactly the same
# samples, also after going back to the start. Needs a C compiler, cargo and network access
# (to fetch minimp3 once).
#
#   tools/mp3-check/run.sh some.mp3 more/*.mp3
set -e
here=$(cd "$(dirname "$0")" && pwd)
# the minimp3 version the port follows
MP3=48e3a085f0e1a99446b34614e1e7f17f09dc2e82
for h in minimp3.h minimp3_ex.h; do
  [ -f "$here/$h" ] || curl -sfL -o "$here/$h" "https://raw.githubusercontent.com/lieff/minimp3/$MP3/$h"
done
# the float operations must stay as written: no -ffast-math, and no fusing a*b+c into one
# FMA instruction (GCC does that by default where the CPU target allows it)
cc -O2 -ffp-contract=off -I"$here" -o "$here/cref" "$here/cref.c" -lm 2>/dev/null
cc --version | head -1
cargo build -q --release --manifest-path "$here/rs/Cargo.toml"
ok=0; bad=0; skipped=0
for f in "$@"; do
  for rounds in 1 2; do
    "$here/cref" "$f" "$here/c.out" $rounds
    if [ "$(head -c 4 "$here/c.out")" = "MEM" ]; then skipped=$((skipped+1)); continue; fi
    "$here/rs/target/release/mp3-check" "$f" "$here/r.out" $rounds
    if cmp -s "$here/c.out" "$here/r.out"; then ok=$((ok+1)); else bad=$((bad+1)); echo "DIFFERENT ($rounds x): $f: $(cmp "$here/c.out" "$here/r.out" 2>&1 | head -1)"; fi
  done
done
rm -f "$here/c.out" "$here/r.out"
echo "identical: $ok, different: $bad, skipped (minimp3 out of memory): $skipped"
[ $bad -eq 0 ]
