#!/bin/sh
# Restore prior versions of Playora *.sh entries from .bak files.
set -eu
PORTS_DIR="${PORTS_DIR:-/roms/ports}"
[ -d "$PORTS_DIR" ] || exit 0
for f in "$PORTS_DIR"/Playora\ *.sh; do
    [ -f "$f" ] || continue
    rm -f "$f"
    bak="$(ls -1t "$f".bak.* 2>/dev/null | head -1 || true)"
    [ -n "$bak" ] && mv "$bak" "$f" && echo "restored $f"
done
