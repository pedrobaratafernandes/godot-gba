#!/bin/bash
# Native Linux build (run it on a Linux machine): GodotProject/bin/libgodot_gba.so
set -e
cd "$(dirname "$0")/.."
rustup toolchain install
cargo build --release
mkdir -p GodotProject/bin
cp target/release/libgodot_gba.so GodotProject/bin/libgodot_gba.so
echo "Linux build finished: GodotProject/bin/libgodot_gba.so"
