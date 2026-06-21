#!/bin/sh
# Install Playora menu entries into dArkOSRE Ports so they show up in EmulationStation.
# Each entry is a tiny launcher .sh that calls playora-agent. User clicks → menu opens.
set -eu
PORTS_DIR="${PORTS_DIR:-/roms/ports}"
[ -d "$PORTS_DIR" ] || { echo "[playora] no $PORTS_DIR; skipping"; exit 0; }

write_port() {
    name="$1"; cmd="$2"
    file="$PORTS_DIR/Playora ${name}.sh"
    [ -f "$file" ] && cp -p "$file" "$file.bak.$(date +%Y%m%d_%H%M%S)"
    cat > "$file" <<EOF
#!/bin/sh
exec /usr/local/bin/playora-agent ${cmd}
EOF
    chmod +x "$file"
    echo "[playora] wrote $file"
}

# Primary entry — opens the interactive TUI (the rest is in there).
write_port "Hub"             "tui"
# Quick-actions for users who want one-clicks without going into the TUI:
write_port "PortMaster"      "tui"   # also accessible from Hub menu
write_port "Update"          "self-update"
write_port "Saves Backup"    "saves upload"
write_port "Doctor"          "doctor"

echo "[playora] done. Restart EmulationStation to see the entries."
