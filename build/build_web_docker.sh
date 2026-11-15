#!/bin/bash
# Web (WASM) build of Godot GBA inside Docker.
# Produces both GDExtension variants in GodotProject/bin/:
#   - godot_gba.wasm          (no threads, for the Web export with "Thread Support" off)
#   - godot_gba.threads.wasm  (threads, for the Web export with "Thread Support" on)
# Uses the Rust pinned in rust-toolchain.toml and the Emscripten pinned in Dockerfile.web.
# On Windows: run it from Git Bash, with Docker Desktop open.
set -e

cd "$(dirname "$0")/.."

IMAGE=godot-gba-web
# Docker's own build folder (kept apart from the host 'cargo test'/'cargo build')
DOCKER_TARGET=target/docker
# Cargo caches in Docker volumes: dependencies are only downloaded once
CACHE_VOLUMES=(-v godot-gba-cargo-registry:/root/.cargo/registry -v godot-gba-cargo-git:/root/.cargo/git)
WASM_OUT="$DOCKER_TARGET/wasm32-unknown-emscripten/release/godot_gba.wasm"
BASE_FLAGS="-C link-args=-sSIDE_MODULE=2 -C llvm-args=-enable-emscripten-cxx-exceptions=0 -Z default-visibility=hidden -Z link-native-libraries=no -Z emscripten-wasm-eh=false"

echo "Building the Docker image..."
docker build -t "$IMAGE" -f Dockerfile.web .

mkdir -p GodotProject/bin

# --- 1. Threaded build ---
echo "Building threaded WASM (godot_gba.threads.wasm)..."
MSYS_NO_PATHCONV=1 docker run --rm \
    -v "/$(pwd):/src" "${CACHE_VOLUMES[@]}" \
    -e CARGO_TARGET_DIR="/src/$DOCKER_TARGET" \
    -e RUSTFLAGS="-C link-args=-pthread -C target-feature=+atomics $BASE_FLAGS" \
    "$IMAGE" cargo build --release --config profile.release.panic=\"abort\" -Zbuild-std=std,panic_abort --target wasm32-unknown-emscripten

if [ -f "$WASM_OUT" ]; then
    cp "$WASM_OUT" GodotProject/bin/godot_gba.threads.wasm
else
    echo "Error: the threaded WASM was not produced!"
    exit 1
fi

# --- 2. Non-threaded build ---
echo "Building non-threaded WASM (godot_gba.wasm)..."
MSYS_NO_PATHCONV=1 docker run --rm \
    -v "/$(pwd):/src" "${CACHE_VOLUMES[@]}" \
    -e CARGO_TARGET_DIR="/src/$DOCKER_TARGET" \
    "$IMAGE" cargo build --release --config profile.release.panic=\"abort\" --features nothreads -Zbuild-std=std,panic_abort --target wasm32-unknown-emscripten

if [ -f "$WASM_OUT" ]; then
    cp "$WASM_OUT" GodotProject/bin/godot_gba.wasm
else
    echo "Error: the non-threaded WASM was not produced!"
    exit 1
fi

echo "------------------------------------------------"
echo "Web build finished! Files in GodotProject/bin/:"
ls -l GodotProject/bin/*.wasm
echo "------------------------------------------------"
