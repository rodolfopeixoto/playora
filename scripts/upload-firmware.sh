#!/bin/sh
set -eu

[ $# -ge 2 ] || { echo "usage: $0 <image-path> <gh-release-tag> [firmware-name]"; exit 1; }

IMG="$1"
TAG="$2"
NAME="${3:-dArkOSRE-R36}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

[ -f "$IMG" ] || { echo "[err] image not found: $IMG"; exit 1; }
command -v gh >/dev/null 2>&1 || { echo "[err] gh CLI required"; exit 1; }
command -v split >/dev/null 2>&1 || { echo "[err] split required"; exit 1; }

CHUNK_SIZE="${CHUNK_SIZE:-1900000000}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

BASE="$(basename "$IMG")"
SIZE="$(stat -f%z "$IMG" 2>/dev/null || stat -c%s "$IMG")"
if command -v shasum >/dev/null 2>&1; then
    FULL_SHA="$(shasum -a 256 "$IMG" | awk '{print $1}')"
else
    FULL_SHA="$(sha256sum "$IMG" | awk '{print $1}')"
fi

echo "[info] image  : $IMG ($SIZE bytes)"
echo "[info] sha256 : $FULL_SHA"
echo "[info] chunk  : $CHUNK_SIZE bytes"

PART_DIR="$WORK/parts"
mkdir -p "$PART_DIR"
PREFIX="$PART_DIR/$BASE."
split -b "$CHUNK_SIZE" -a 3 -d "$IMG" "$PREFIX"

N=0
for f in "$PREFIX"*; do
    N=$((N + 1))
done
echo "[info] parts  : $N"

REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
BASE_URL="https://github.com/$REPO/releases/download/$TAG"

PARTS_JSON=""
SEP=""
for f in "$PREFIX"*; do
    fname="$(basename "$f")"
    gh release upload "$TAG" "$f" --clobber >/dev/null
    PARTS_JSON="$PARTS_JSON$SEP\"$BASE_URL/$fname\""
    SEP=", "
    echo "[ok]   uploaded $fname"
done

MANIFEST="$ROOT/dist/firmware-manifest-$NAME.json"
mkdir -p "$(dirname "$MANIFEST")"
cat > "$MANIFEST" <<EOF
{
  "file_name": "$BASE",
  "sha256": "$FULL_SHA",
  "size_bytes": $SIZE,
  "parts": [ $PARTS_JSON ]
}
EOF

gh release upload "$TAG" "$MANIFEST" --clobber >/dev/null
echo "[ok] manifest -> $BASE_URL/$(basename "$MANIFEST")"
echo
echo "users on the handheld set:"
echo "  export DARKOS_FIRMWARE_MANIFEST=/roms/.darkOs/firmware-manifest-$NAME.json"
echo "or fetch it once:"
echo "  curl -fsSL $BASE_URL/$(basename "$MANIFEST") -o /roms/.darkOs/firmware-manifest-$NAME.json"
echo "then:"
echo "  darkos firmware fetch $NAME"
