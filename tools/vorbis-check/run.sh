#!/bin/sh
# Compares featheraudio's OGG Vorbis decoder (a port of stb_vorbis) with stb_vorbis itself:
# every file given must decode to exactly the same samples, also after going back to the
# start. Needs a C compiler, cargo and network access (to fetch stb_vorbis.c once).
#
#   tools/vorbis-check/run.sh some.ogg more/*.ogg
set -e
here=$(cd "$(dirname "$0")" && pwd)
# stb_vorbis 1.25, the version the port follows
STB=6b5799bae8a1fe3792691ef4b72ae26434c3e799
[ -f "$here/stb_vorbis.c" ] || curl -sfL -o "$here/stb_vorbis.c" "https://raw.githubusercontent.com/nothings/stb/$STB/stb_vorbis.c"
# the float operations must stay as written: no -ffast-math, and no fusing a*b+c into one
# FMA instruction (GCC does that by default where the CPU target allows it)
cc -O2 -ffp-contract=off -I"$here" -o "$here/cref" "$here/cref.c" -lm 2>/dev/null
cc --version | head -1
cargo build -q --release --manifest-path "$here/rs/Cargo.toml"
ok=0; bad=0
for f in "$@"; do
  for rounds in 1 2; do
    "$here/cref" "$f" "$here/c.out" $rounds
    "$here/rs/target/release/vorbis-check" "$f" "$here/r.out" $rounds
    if cmp -s "$here/c.out" "$here/r.out"; then ok=$((ok+1)); else bad=$((bad+1)); echo "DIFFERENT ($rounds x): $f: $(cmp "$here/c.out" "$here/r.out" 2>&1 | head -1)"; fi
  done
done
rm -f "$here/c.out" "$here/r.out"
echo "identical: $ok, different: $bad"
[ $bad -eq 0 ]
