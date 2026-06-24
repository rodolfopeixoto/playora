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

cat > "$DARKOS_DIR/env.sh" <<'EOF'
export DARKOS_HOME=/roms/.darkOs
export DARKOS_RELEASE_URL=https://github.com/rodolfopeixoto/playora/releases/latest/download/latest.json
[ -f /roms/.darkOs/firmware-manifest-dArkOSRE-R36.json ] \
    && export DARKOS_FIRMWARE_MANIFEST=/roms/.darkOs/firmware-manifest-dArkOSRE-R36.json
EOF
chmod 0644 "$DARKOS_DIR/env.sh"

FETCH_MANIFEST_URL="${DARKOS_FIRMWARE_MANIFEST_URL:-https://github.com/rodolfopeixoto/playora/releases/latest/download/firmware-manifest-dArkOSRE-R36.json}"
cat > "$BIN_DIR/refresh-firmware-manifest.sh" <<EOF
#!/bin/sh
set -eu
mkdir -p /roms/.darkOs
curl -fsSL --max-time 60 "$FETCH_MANIFEST_URL" -o /roms/.darkOs/firmware-manifest-dArkOSRE-R36.json
echo "manifest -> /roms/.darkOs/firmware-manifest-dArkOSRE-R36.json"
EOF
chmod 0755 "$BIN_DIR/refresh-firmware-manifest.sh"

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
write_port "darkOs Firmware Check"  'exec sh -c ". /roms/.darkOs/env.sh; /roms/.darkOs/bin/darkos firmware check dArkOSRE-R36 | /roms/.darkOs/bin/darkos-view-wrap.sh"'
write_port "darkOs Firmware Refresh" 'exec sh -c "sudo /roms/.darkOs/bin/refresh-firmware-manifest.sh | /roms/.darkOs/bin/darkos-view-wrap.sh"'
write_port "darkOs Firmware Fetch"  'exec sh -c ". /roms/.darkOs/env.sh; sudo -E /roms/.darkOs/bin/darkos firmware fetch dArkOSRE-R36 | /roms/.darkOs/bin/darkos-view-wrap.sh"'

printf '%s\n' "$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)" > "$DARKOS_DIR/VERSION"

du -sh "$DARKOS_DIR"
echo "[ok] darkOs staged on $SD ($BIN_DIR/darkos)"
