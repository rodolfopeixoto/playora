#!/bin/sh
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SD="${SD:-/Volumes/EASYROMS}"
BIN="$ROOT/dist/darkos-aarch64"

[ -f "$BIN" ] || { echo "[err] missing $BIN — run scripts/build-container.sh"; exit 1; }
[ -d "$SD" ] || { echo "[err] $SD not mounted"; exit 1; }

DARKOS_DIR="$SD/.darkOs"
BIN_DIR="$DARKOS_DIR/bin"
PROFILE_DIR="$DARKOS_DIR/gptokeyb"
PORTS_DIR="$SD/ports"

mkdir -p "$BIN_DIR" "$PROFILE_DIR" "$PORTS_DIR" "$DARKOS_DIR/logs"

install -m 0755 "$BIN"                              "$BIN_DIR/darkos"
install -m 0755 "$ROOT/scripts/darkos-view-wrap.sh" "$BIN_DIR/darkos-view-wrap.sh"
install -m 0755 "$ROOT/scripts/install-on-device.sh" "$BIN_DIR/install-on-device.sh"
install -m 0755 "$ROOT/scripts/console-install.sh"  "$BIN_DIR/console-install.sh"
install -m 0644 "$ROOT/scripts/darkos-view.gptk"    "$PROFILE_DIR/darkos-view.gptk"

write_port() {
    OUT="$PORTS_DIR/$1.sh"
    printf '#!/bin/sh\n%s\n' "$2" > "$OUT"
    chmod 0755 "$OUT"
}

write_port "darkOs Menu"            'exec /roms/.darkOs/bin/darkos tui'
write_port "darkOs System Log"      'exec /roms/.darkOs/bin/darkos-view-wrap.sh /var/log/messages'
write_port "darkOs Kernel Log"      'exec sh -c "dmesg | /roms/.darkOs/bin/darkos-view-wrap.sh"'
write_port "darkOs Update"          'exec sudo /roms/.darkOs/bin/console-install.sh'
write_port "darkOs Self-Update"     'exec sudo /roms/.darkOs/bin/darkos update --self'
write_port "darkOs Firmware Check"  'exec sh -c "/roms/.darkOs/bin/darkos firmware check dArkOSRE-R36 | /roms/.darkOs/bin/darkos-view-wrap.sh"'
write_port "darkOs Firmware Fetch"  'exec sh -c "sudo /roms/.darkOs/bin/darkos firmware fetch dArkOSRE-R36 | /roms/.darkOs/bin/darkos-view-wrap.sh"'

printf '%s\n' "$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)" > "$DARKOS_DIR/VERSION"

du -sh "$DARKOS_DIR"
echo "[ok] darkOs staged on $SD ($BIN_DIR/darkos)"
