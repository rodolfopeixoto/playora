#!/bin/sh
# Cross-build playora-agent (aarch64-linux-gnu) via Docker.
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
IMAGE="rust:1.81"
docker run --rm -v "$ROOT":/work -w /work --platform linux/arm64 "$IMAGE" \
    sh -c "cargo build --release --bin playora-agent && ls -lh target/release/playora-agent"
mkdir -p "$ROOT/dist"
cp target/release/playora-agent "$ROOT/dist/playora-agent-aarch64"
echo "dist/playora-agent-aarch64 ready"
