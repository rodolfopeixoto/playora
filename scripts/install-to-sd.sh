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

cp "$BIN" "$PLAYORA_DIR/playora-agent"
chmod 0755 "$PLAYORA_DIR/playora-agent"

write_port() {
    name="$1"; mode="$2"; cmd="$3"
    file="$PORTS_DIR/Playora ${name}.sh"
    safe_name="$(echo "$name" | tr ' ' _)"

    cat > "$file" <<EOF
#!/bin/bash
set +e
LOG="/roms/.playora/logs/${safe_name}_\$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
CFG=/roms/.playora/agent.toml
MODE="${mode}"
DISPLAY_SECS=20

restart_es() {
    sudo systemctl start emulationstation 2>/dev/null
    sudo systemctl restart emulationstation 2>/dev/null
    if ! pgrep -x emulationstation >/dev/null; then
        (cd /; nohup sudo -u ark emulationstation >/dev/null 2>&1 &)
    fi
    if ! pgrep -x emulationstation >/dev/null; then
        (cd /; nohup emulationstation >/dev/null 2>&1 &)
    fi
}

trap 'restart_es' EXIT INT TERM HUP

(
    sleep 120
    echo "[watchdog] forcing ES restart" >> "\$LOG" 2>/dev/null
    sudo killall -9 playora-agent 2>/dev/null
    sudo killall -9 gptokeyb 2>/dev/null
    restart_es
) &
WATCHDOG_PID=\$!

sudo systemctl stop emulationstation 2>/dev/null
sudo killall -9 emulationstation 2>/dev/null
sleep 1

sudo chvt 1 2>/dev/null
TTY=/dev/tty1
[ -w "\$TTY" ] || TTY=/dev/console

CTL=""
for c in /opt/system/PortMaster /opt/portmaster /roms/tools/PortMaster /roms2/tools/PortMaster; do
    if [ -x "\$c/gptokeyb" ]; then CTL="\$c"; break; fi
done
if [ -n "\$CTL" ]; then
    sudo chmod 666 /dev/uinput 2>/dev/null
    KEYS="\$CTL/keys.gptk"
    [ -f "\$KEYS" ] || KEYS=""
    "\$CTL/gptokeyb" -1 "playora-agent" \${KEYS:+-c "\$KEYS"} >/dev/null 2>&1 &
    GPID=\$!
fi

if [ "\$MODE" = "tui" ]; then
    DISPLAY_SECS=600
    {
        clear
        echo "Playora — ${name}"
        echo
        cd /roms/.playora
        timeout 60 ./playora-agent --config "\$CFG" ${cmd} 2>&1 | tee -a "\$LOG"
        echo
        echo "==== finished — auto-exit in 20s ===="
    } <"\$TTY" >"\$TTY" 2>&1
    sleep 20
else
    {
        echo "==== \$(date) ===="
        echo "Command: ${cmd}"
        echo
        cd /roms/.playora
        timeout 30 ./playora-agent --config "\$CFG" ${cmd}
        echo
        echo "Exit code: \$?"
    } > "\$LOG" 2>&1

    {
        clear
        cat "\$LOG"
        echo
        echo "============================================================"
        echo " auto-exit in \$DISPLAY_SECS seconds (or press POWER to skip) "
        echo "============================================================"
    } <"\$TTY" >"\$TTY" 2>&1
    sleep \$DISPLAY_SECS

    cd /roms/.playora && timeout 10 ./playora-agent --config "\$CFG" sync >/dev/null 2>&1
fi

kill \$WATCHDOG_PID 2>/dev/null
[ -n "\${GPID:-}" ] && kill -9 \$GPID 2>/dev/null
sudo killall -9 gptokeyb 2>/dev/null
EOF
    chmod 0755 "$file"
    echo "[install] wrote ${file}"
}

write_port "Hub"             "tui"  "tui"
write_port "PortMaster"      "tui"  "tui"
write_port "Restore Backup"  "tui"  "restore-tar"
write_port "Update"          "tui"  "self-update"
write_port "Doctor"          "task" "doctor"
write_port "Hardware"        "task" "hardware snapshot --save"
write_port "Saves Backup"    "task" "saves upload"
write_port "Kodi Setup"      "task" "kodi setup"

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

cat > "$PLAYORA_DIR/autostart.sh" <<'EOF'
#!/bin/sh
mkdir -p /roms/.playora/logs
nohup /roms/.playora/playora-agent --config /roms/.playora/agent.toml run \
    > /roms/.playora/logs/run.log 2>&1 &
echo $! > /tmp/playora-agent.pid
EOF
chmod 0755 "$PLAYORA_DIR/autostart.sh"

cat > "$PORTS_DIR/Playora Autosync Enable.sh" <<EOF
#!/bin/bash
LOG="/roms/.playora/logs/autosync_enable_\$(date +%Y%m%d_%H%M%S).log"
{
    /roms/.playora/autostart.sh
    AUTOSTART_DIR="/storage/.config/autostart"
    [ -d "\$AUTOSTART_DIR" ] || AUTOSTART_DIR="/etc/runlevels/default"
    [ -d "\$AUTOSTART_DIR" ] || sudo mkdir -p /etc/playora && AUTOSTART_DIR="/etc/playora"
    sudo cp /roms/.playora/autostart.sh "\$AUTOSTART_DIR/playora.sh" 2>/dev/null
    sudo chmod 0755 "\$AUTOSTART_DIR/playora.sh" 2>/dev/null
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
    echo "autosync enabled at \$AUTOSTART_DIR"
    /roms/.playora/playora-agent --config /roms/.playora/agent.toml status
} > "\$LOG" 2>&1
TTY=/dev/tty1
[ -w "\$TTY" ] || TTY=/dev/console
{ /roms/.playora/playora-agent --config /roms/.playora/agent.toml show-log "\$LOG"; } <"\$TTY" >"\$TTY" 2>&1
EOF
chmod 0755 "$PORTS_DIR/Playora Autosync Enable.sh"

sync
echo
echo "Playora installed. Eject SD, boot R36S, ES → Ports → Playora Hub"
