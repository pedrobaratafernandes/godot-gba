#!/bin/bash
# Linux (x86_64) GDExtension build inside Docker: GodotProject/bin/libgodot_gba.so
# Uses the same image as the Web build (Rust pinned in rust-toolchain.toml).
# On Windows: run it from Git Bash, with Docker Desktop open.
set -e
cd "$(dirname "$0")/.."

IMAGE=godot-gba-web
DOCKER_TARGET=target/docker-linux

echo "Building the Docker image..."
docker build -t "$IMAGE" -f Dockerfile.web .

echo "Building for Linux (Release)..."
MSYS_NO_PATHCONV=1 docker run --rm \
    -v "/$(pwd):/src" \
    -v godot-gba-cargo-registry:/root/.cargo/registry -v godot-gba-cargo-git:/root/.cargo/git \
    -e CARGO_TARGET_DIR="/src/$DOCKER_TARGET" \
    "$IMAGE" cargo build --release --target x86_64-unknown-linux-gnu

mkdir -p GodotProject/bin
cp "$DOCKER_TARGET/x86_64-unknown-linux-gnu/release/libgodot_gba.so" GodotProject/bin/libgodot_gba.so

echo "------------------------------------------------"
echo "Linux build finished: GodotProject/bin/libgodot_gba.so"
echo "------------------------------------------------"
