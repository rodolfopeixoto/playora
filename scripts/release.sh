#!/bin/sh
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep -m1 '^version' Cargo.toml | head -1 | cut -d'"' -f2)"
[ -n "$VERSION" ] || { echo "[err] cannot read version"; exit 1; }
BASE_URL="${PLAYORA_RELEASE_BASE_URL:-https://github.com/rodolfopeixoto/playora/releases/download/v$VERSION}"

sh scripts/build-container.sh

OUT="$ROOT/dist/release/$VERSION"
rm -rf "$OUT"
mkdir -p "$OUT"

cp "$ROOT/dist/playora-agent-aarch64"  "$OUT/playora-agent-$VERSION-aarch64"
cp "$ROOT/dist/playora-server-aarch64" "$OUT/playora-server-$VERSION-aarch64"
cp "$ROOT/dist/darkos-aarch64"         "$OUT/darkos-$VERSION-aarch64"

STAGE="$ROOT/dist/release-stage"
rm -rf "$STAGE" && mkdir -p "$STAGE/darkos"
cp "$ROOT/dist/darkos-aarch64"             "$STAGE/darkos/darkos"
cp "$ROOT/scripts/darkos-view-wrap.sh"     "$STAGE/darkos/"
cp "$ROOT/scripts/darkos-view.gptk"        "$STAGE/darkos/"
cp "$ROOT/scripts/install-on-device.sh"    "$STAGE/darkos/"
chmod +x "$STAGE/darkos/darkos" "$STAGE/darkos/darkos-view-wrap.sh" "$STAGE/darkos/install-on-device.sh"
TAR_NAME="darkos-$VERSION-aarch64.tar.gz"
tar -C "$STAGE/darkos" -czf "$OUT/$TAR_NAME" .

sha() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

SHA_AGENT="$(sha "$OUT/playora-agent-$VERSION-aarch64")"
SHA_SERVER="$(sha "$OUT/playora-server-$VERSION-aarch64")"
SHA_DARKOS="$(sha "$OUT/darkos-$VERSION-aarch64")"
SHA_TAR="$(sha "$OUT/$TAR_NAME")"

for f in "$OUT/playora-agent-$VERSION-aarch64" "$OUT/playora-server-$VERSION-aarch64" "$OUT/darkos-$VERSION-aarch64" "$OUT/$TAR_NAME"; do
    printf '%s  %s\n' "$(sha "$f")" "$(basename "$f")" > "$f.sha256"
done

cat > "$OUT/latest.json" <<EOF
{
  "version": "$VERSION",
  "binaries": {
    "playora_agent":  { "url": "$BASE_URL/playora-agent-$VERSION-aarch64",  "sha256": "$SHA_AGENT" },
    "playora_server": { "url": "$BASE_URL/playora-server-$VERSION-aarch64", "sha256": "$SHA_SERVER" },
    "darkos":         { "url": "$BASE_URL/darkos-$VERSION-aarch64",         "sha256": "$SHA_DARKOS" }
  },
  "darkos_tarball": { "url": "$BASE_URL/$TAR_NAME", "sha256": "$SHA_TAR" },
  "binary_url": "$BASE_URL/darkos-$VERSION-aarch64",
  "sha256": "$SHA_DARKOS",
  "tarball_url": "$BASE_URL/$TAR_NAME",
  "tarball_sha256": "$SHA_TAR"
}
EOF

ls -lh "$OUT"
echo "[ok] release $VERSION at $OUT"
echo "    upload all files to: $BASE_URL/"
