#!/bin/sh
# Builds the browser demo (WebAssembly) into web/dist. Linux / macOS / CI.
# Needs rustup / cargo, the wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.130
# (cargo install wasm-bindgen-cli --version 0.2.130 --locked).
# With --single, also writes dist/featheraudio-demo.html: one file with everything inside.
set -e
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target no-modules --no-typescript --out-dir dist target/wasm32-unknown-unknown/release/featheraudio_web.wasm
cp index.html dist/
if [ "$1" = "--single" ]; then python3 single.py; fi
ls -l dist
