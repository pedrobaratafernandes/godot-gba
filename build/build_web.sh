#!/bin/bash
# Native Web (WASM) build, without Docker. Requires Emscripten 4.0.20 to be active (emsdk_env).
# Only builds the non-threaded variant (godot_gba.wasm), the one used by the Web preset.
set -e
cd "$(dirname "$0")/.."

echo "Installing the Rust version pinned in rust-toolchain.toml..."
rustup toolchain install

RUSTC_BOOTSTRAP=1 cargo build --release --config profile.release.panic=\"abort\" --features nothreads -Zbuild-std=std,panic_abort --target wasm32-unknown-emscripten

mkdir -p GodotProject/bin
cp target/wasm32-unknown-emscripten/release/godot_gba.wasm GodotProject/bin/godot_gba.wasm
echo "Web build finished: GodotProject/bin/godot_gba.wasm"
