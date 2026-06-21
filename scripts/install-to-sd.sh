#!/bin/sh
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/dist/playora-agent-aarch64"
[ -f "$BIN" ] || { echo "Build first: sh scripts/build-container.sh"; exit 1; }

SD="${SD:-/Volumes/EASYROMS}"
[ -d "$SD" ] || { echo "$SD not mounted. Insert the dArkOSRE SD."; exit 1; }

PLAYORA_DIR="$SD/.playora"
PORTS_DIR="$SD/ports"
LOG_DIR="$SD/.playora/logs"
mkdir -p "$PLAYORA_DIR" "$PORTS_DIR" "$LOG_DIR"

# Sweep any leftover Playora *.sh (and .bak.* files) from prior generators.
find "$PORTS_DIR" -maxdepth 1 -type f -name "Playora *.sh*" -print -delete 2>/dev/null || true

cp "$BIN" "$PLAYORA_DIR/playora-agent"
chmod 0755 "$PLAYORA_DIR/playora-agent"

write_port() {
    name="$1"; cmd="$2"
    file="$PORTS_DIR/Playora ${name}.sh"
    safe_name="$(echo "$name" | tr ' ' _)"
    cat > "$file" <<EOF
#!/bin/sh
LOG="/roms/.playora/logs/${safe_name}_\$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
{
    echo "==== \$(date) ===="
    echo "command: ${cmd}"
    cd /roms/.playora
    ./playora-agent --config /roms/.playora/agent.toml activity-begin "${name}"
    timeout 60 ./playora-agent --config /roms/.playora/agent.toml ${cmd}
    RC=\$?
    ./playora-agent --config /roms/.playora/agent.toml activity-end "${name}" "\$RC"
    timeout 10 ./playora-agent --config /roms/.playora/agent.toml sync >/dev/null 2>&1
    echo "exit: \$RC"
} > "\$LOG" 2>&1 &
sleep 1
exit 0
EOF
    chmod 0755 "$file"
    echo "[install] wrote ${file}"
}

write_port "Doctor"          "doctor"
write_port "Hardware"        "hardware snapshot --save"
write_port "Quick Sync"      "quick-sync"
write_port "Saves Backup"    "saves upload"
write_port "Restore Backup"  "restore-tar"
write_port "Update"          "self-update"
write_port "Kodi Setup"      "kodi setup"
write_port "Scan ROMs"       "scan"
write_port "Heartbeat"       "heartbeat"

cat > "$PORTS_DIR/Playora Recover.sh" <<'EOF'
#!/bin/sh
LOG="/roms/.playora/logs/recover_$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
{
    echo "==== $(date) ===="
    sudo killall -9 playora-agent 2>/dev/null
    sudo killall -9 gptokeyb 2>/dev/null
    sudo systemctl restart emulationstation 2>/dev/null \
        || sudo systemctl start emulationstation 2>/dev/null \
        || (cd /; nohup emulationstation >/dev/null 2>&1 &)
    echo "recover done"
} > "$LOG" 2>&1 &
sleep 1
exit 0
EOF
chmod 0755 "$PORTS_DIR/Playora Recover.sh"
echo "[install] wrote $PORTS_DIR/Playora Recover.sh"

cat > "$PLAYORA_DIR/autostart.sh" <<'EOF'
#!/bin/sh
mkdir -p /roms/.playora/logs
pgrep -fx '/roms/.playora/playora-agent --config /roms/.playora/agent.toml run' >/dev/null && exit 0
nohup /roms/.playora/playora-agent --config /roms/.playora/agent.toml run \
    > /roms/.playora/logs/run.log 2>&1 &
echo $! > /tmp/playora-agent.pid
EOF
chmod 0755 "$PLAYORA_DIR/autostart.sh"

cat > "$PORTS_DIR/Playora Autosync Enable.sh" <<EOF
#!/bin/sh
LOG="/roms/.playora/logs/autosync_\$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
{
    /roms/.playora/autostart.sh
    if command -v systemctl >/dev/null 2>&1; then
        sudo tee /etc/systemd/system/playora-agent.service > /dev/null <<UNIT
[Unit]
Description=Playora agent
After=network-online.target
[Service]
ExecStart=/roms/.playora/playora-agent --config /roms/.playora/agent.toml run
Restart=on-failure
RestartSec=10
StandardOutput=append:/roms/.playora/logs/run.log
StandardError=append:/roms/.playora/logs/run.log
[Install]
WantedBy=multi-user.target
UNIT
        sudo systemctl daemon-reload
        sudo systemctl enable --now playora-agent.service
    fi
    /roms/.playora/playora-agent --config /roms/.playora/agent.toml activity-end "Autosync Enable" 0
} > "\$LOG" 2>&1 &
sleep 1
exit 0
EOF
chmod 0755 "$PORTS_DIR/Playora Autosync Enable.sh"

CFG="$PLAYORA_DIR/agent.toml"
if [ ! -f "$CFG" ]; then
    SERVER_URL="${PLAYORA_SERVER_URL:-http://192.168.3.82:8080}"
    DEVICE_ID="dev_$(uuidgen | tr -d '-' | tr 'A-Z' 'a-z' | cut -c1-32)"
    cat > "$CFG" <<EOF
device_id = "$DEVICE_ID"
device_name = "R36S"
device_profile = "r36s-darkosre-clone"
os_family = "darkosre-r36"
server_url = "$SERVER_URL"
rom_paths = ["/roms"]
save_paths = ["/roms/savestates"]
metadata_paths = ["/roms"]
scan_interval_minutes = 60
sync_interval_seconds = 60
max_batch_size = 100
enable_runtime_probe = false
enable_retroarch_network_control = false
retroarch_udp_port = 55355
enable_catalog = true
enable_hardware_tests = true
enable_resource_sampling = true
log_level = "info"
EOF
    echo "[install] wrote config: $CFG"
fi

for s in "$SD/tools/R36S-Backup.sh" "$SD/tools/R36S-Search.sh" "$SD/tools/R36S-Install-Collections.sh" "$SD/tools/R36S-Smart.sh" "$SD/tools/R36S-Storage.sh"; do
    [ -f "$s" ] && mv "$s" "$s.disabled" 2>/dev/null && echo "[install] disabled $s"
done

find "$SD" -name "._*" -delete 2>/dev/null || true
find "$SD" -name ".DS_Store" -delete 2>/dev/null || true

sync
echo
echo "Playora installed. Every Port is fire-and-forget. ES never freezes."
echo "Watch results live: ${PLAYORA_SERVER_URL:-http://192.168.3.82:8080}/dashboard"
