#!/bin/sh
# Builds featheraudio for web pages: the C API as WebAssembly (no wasm-bindgen: plain numbers
# and memory, so it runs inside an AudioWorklet), next to featheraudio.js and
# featheraudio-processor.js, in web/worklet/dist/ (with test.html and its sounds).
#
#   web/worklet/build.sh                      all features (tracker, ogg, mp3, effects)
#   web/worklet/build.sh xm,ogg,effects       only these (a smaller .wasm)
set -e
here=$(cd "$(dirname "$0")" && pwd)
root="$here/../.."
features="${1:-tracker,ogg,mp3,effects}"
cargo build --release --manifest-path "$root/capi/Cargo.toml" --target wasm32-unknown-unknown --no-default-features --features "$features"
mkdir -p "$here/dist"
cp "$root/capi/target/wasm32-unknown-unknown/release/featheraudio.wasm" "$here/featheraudio.js" "$here/featheraudio-processor.js" "$here/test.html" "$here/dist/"
cp "$root/tests/data/stereo.ogg" "$root/tests/data/stereo.mp3" "$root/tests/data/loop-end.mod" "$here/dist/"
ls -l "$here/dist"
