#!/bin/sh
set -eu

URL="${DARKOS_RELEASE_URL:-https://github.com/rodolfopeixoto/playora/releases/latest/download/latest.json}"
WORK="/tmp/darkOs-install"

need() { command -v "$1" >/dev/null 2>&1 || { echo "[err] $1 missing"; exit 1; }; }
need curl
need tar
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 \
    || { echo "[err] need sha256sum or shasum"; exit 1; }

MANIFEST="$(curl -fsSL "$URL")"
get_field() {
    printf '%s' "$MANIFEST" | sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" | head -1
}
TARBALL_URL="$(get_field tarball_url)"
TAR_SHA="$(get_field tarball_sha256)"
VERSION="$(get_field version)"
[ -n "$TARBALL_URL" ] && [ -n "$TAR_SHA" ] || { echo "[err] manifest missing fields"; exit 1; }

echo "[info] installing darkOs $VERSION"
rm -rf "$WORK" && mkdir -p "$WORK"
TAR="$WORK/payload.tar.gz"
curl -fsSL "$TARBALL_URL" -o "$TAR"

if command -v sha256sum >/dev/null 2>&1; then
    GOT="$(sha256sum "$TAR" | awk '{print $1}')"
else
    GOT="$(shasum -a 256 "$TAR" | awk '{print $1}')"
fi
[ "$GOT" = "$TAR_SHA" ] || { echo "[err] sha256 mismatch"; exit 2; }

tar -xzf "$TAR" -C "$WORK"
sh "$WORK/install-on-device.sh"
