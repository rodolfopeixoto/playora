#!/bin/sh
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v container >/dev/null 2>&1; then
    echo "Apple 'container' CLI not found; falling back to docker"
    exec sh "$ROOT/scripts/build-arm64.sh"
fi

container system status >/dev/null 2>&1 || container system start

IMAGE="${RUST_IMAGE:-rust:1.90}"
TARGET_DIR="$ROOT/target-arm64"
mkdir -p "$TARGET_DIR" "$ROOT/dist"

echo "[container] building playora-agent + playora-server + darkos (aarch64)"
container run --rm \
    --arch arm64 \
    --cpus 4 \
    --memory 8G \
    --mount type=bind,source="$ROOT",target=/work \
    --workdir /work \
    --env CARGO_TARGET_DIR=/work/target-arm64 \
    --env CARGO_PROFILE_RELEASE_LTO=thin \
    --env CARGO_PROFILE_RELEASE_CODEGEN_UNITS=4 \
    "$IMAGE" \
    sh -c "cargo build --release --bins"

cp "$TARGET_DIR/release/playora-agent"  "$ROOT/dist/playora-agent-aarch64"
cp "$TARGET_DIR/release/playora-server" "$ROOT/dist/playora-server-aarch64"
cp "$TARGET_DIR/release/darkos"         "$ROOT/dist/darkos-aarch64"
ls -lh "$ROOT/dist/"
