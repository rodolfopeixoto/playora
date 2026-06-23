#!/bin/sh
set -eu

DARKOS_BIN="${DARKOS_BIN:-/roms/.darkOs/bin/darkos}"
[ -x "$DARKOS_BIN" ] || DARKOS_BIN="/usr/local/bin/darkos"
[ -x "$DARKOS_BIN" ] || { echo "darkos binary not found" >&2; exit 1; }

PROFILE="${DARKOS_VIEW_PROFILE:-}"
if [ -z "$PROFILE" ]; then
    for c in \
        /roms/.darkOs/gptokeyb/darkos-view.gptk \
        /opt/dArkOSRE/gptokeyb/profiles/darkos-view.gptk \
        /opt/DarkOS/gptokeyb/profiles/darkos-view.gptk \
        /opt/ArkOS/gptokeyb/profiles/darkos-view.gptk \
        /etc/gptokeyb/profiles/darkos-view.gptk; do
        [ -f "$c" ] && { PROFILE="$c"; break; }
    done
fi

run_viewer() {
    if [ -n "$PROFILE" ] && command -v gptokeyb >/dev/null 2>&1; then
        exec gptokeyb -c "$PROFILE" -- "$DARKOS_BIN" view "$1"
    else
        exec "$DARKOS_BIN" view "$1"
    fi
}

if [ -p /dev/stdin ] || [ ! -t 0 ]; then
    TMP="$(mktemp -t darkos-view.XXXXXX)"
    trap 'rm -f "$TMP"' EXIT
    cat > "$TMP"
    run_viewer "$TMP"
fi

[ $# -ge 1 ] || { echo "usage: $(basename "$0") <file>  |  <cmd> | $(basename "$0")" >&2; exit 2; }
run_viewer "$1"
