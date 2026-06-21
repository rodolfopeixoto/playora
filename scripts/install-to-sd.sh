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
INBOX_DIR="$SD/_inbox"
mkdir -p "$PLAYORA_DIR" "$PORTS_DIR" "$LOG_DIR" "$INBOX_DIR"

# Sweep leftover Playora *.sh (and .bak.*) from previous generators.
find "$PORTS_DIR" -maxdepth 1 -type f -name "Playora *.sh*" -print -delete 2>/dev/null || true

cp "$BIN" "$PLAYORA_DIR/playora-agent"
chmod 0755 "$PLAYORA_DIR/playora-agent"

# Shared helper: every port sources this for tty feedback + trap-guaranteed activity-end.
cat > "$PLAYORA_DIR/port-runner.sh" <<'RUNNER'
#!/bin/sh
# Args: NAME CMD [TIMEOUT_SECONDS]
# Writes status lines to /dev/tty1 + log file, posts Activity events to server.
NAME="$1"; shift
CMD="$1"; shift
TIMEOUT="${1:-30}"

SAFE="$(echo "$NAME" | tr ' /' '__')"
LOG="/roms/.playora/logs/${SAFE}_$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
AGENT="/roms/.playora/playora-agent --config /roms/.playora/agent.toml"

# tty1 = console framebuffer text on RK3326 dArkOSRE
TTY=/dev/tty1
[ -w "$TTY" ] || TTY=/dev/null

say() {
    printf '\033[36m[Playora]\033[0m %s\n' "$*" > "$TTY" 2>/dev/null || true
    echo "[$(date +%H:%M:%S)] $*" >> "$LOG"
}

# Trap guarantees activity-end fires even on timeout/kill.
END_RC=1
trap '
    RC=$END_RC
    say "exit $RC"
    $AGENT activity-end "$NAME" "$RC" --log "$LOG" >/dev/null 2>&1 || true
    $AGENT sync >/dev/null 2>&1 || true
' EXIT INT TERM

say "===== $NAME ====="
say "starting at $(date)"
say "command: $CMD"
say "timeout: ${TIMEOUT}s"
say "log: $LOG"
say ""

$AGENT activity-begin "$NAME" >/dev/null 2>&1 || true
say "> sending start event..."

say "> running '$CMD' (max ${TIMEOUT}s)..."
if [ "$TIMEOUT" = "none" ]; then
    $AGENT $CMD >> "$LOG" 2>&1
    END_RC=$?
else
    timeout "$TIMEOUT" $AGENT $CMD >> "$LOG" 2>&1
    END_RC=$?
fi

if [ "$END_RC" = "0" ]; then
    say "> ok"
else
    say "> FAILED (exit $END_RC)"
    # Show last 3 log lines on screen for quick debug.
    tail -n 3 "$LOG" 2>/dev/null | while IFS= read -r line; do
        printf '\033[31m  %s\033[0m\n' "$line" > "$TTY" 2>/dev/null || true
    done
fi
say "> syncing to dashboard..."
exit $END_RC
RUNNER
chmod 0755 "$PLAYORA_DIR/port-runner.sh"

# Generator: each port is a thin wrapper that backgrounds port-runner.sh
# and exits within 1 second so EmulationStation never freezes.
write_port() {
    name="$1"; cmd="$2"; timeout_s="${3:-30}"
    file="$PORTS_DIR/Playora ${name}.sh"
    cat > "$file" <<EOF
#!/bin/sh
# Fires port-runner in background — ES splash transitions cleanly, runner draws tty1 text.
/roms/.playora/port-runner.sh "${name}" "${cmd}" "${timeout_s}" &
sleep 1
exit 0
EOF
    chmod 0755 "$file"
    echo "[install] wrote ${file}"
}

# name | command | timeout-seconds (or "none" for no timeout)
write_port "Doctor"          "doctor"                   30
write_port "Hardware"        "hardware snapshot --save" 30
write_port "Quick Sync"      "quick-sync"               45
write_port "Heartbeat"       "heartbeat"                10
write_port "Saves Backup"    "saves upload"             120
write_port "Restore Backup"  "restore-tar"              none
write_port "Update"          "self-update"              180
write_port "Kodi Setup"      "kodi setup"               60
write_port "Scan ROMs"       "scan"                     300
write_port "Extract ROMs"    "extract-roms"             600

# Autosync triple: Status / Enable / Disable
write_port "Autosync Status" "status"                   10

cat > "$PORTS_DIR/Playora Autosync Enable.sh" <<'EOF'
#!/bin/sh
/roms/.playora/port-runner.sh "Autosync Enable" "noop" 60 &
(
    sleep 2
    mkdir -p /roms/.playora/logs
    LOG="/roms/.playora/logs/autosync_enable_$(date +%Y%m%d_%H%M%S).log"
    {
        echo "==== $(date) ===="
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
            sudo systemctl enable --now playora-agent.service && echo "service enabled"
        else
            nohup /roms/.playora/playora-agent --config /roms/.playora/agent.toml run \
                > /roms/.playora/logs/run.log 2>&1 &
            echo "running as background process (no systemd)"
        fi
    } >> "$LOG" 2>&1
) &
sleep 1
exit 0
EOF
chmod 0755 "$PORTS_DIR/Playora Autosync Enable.sh"
echo "[install] wrote $PORTS_DIR/Playora Autosync Enable.sh"

cat > "$PORTS_DIR/Playora Autosync Disable.sh" <<'EOF'
#!/bin/sh
/roms/.playora/port-runner.sh "Autosync Disable" "noop" 60 &
(
    sleep 2
    LOG="/roms/.playora/logs/autosync_disable_$(date +%Y%m%d_%H%M%S).log"
    {
        echo "==== $(date) ===="
        sudo systemctl disable --now playora-agent.service 2>/dev/null || true
        pkill -f "playora-agent.*run" 2>/dev/null || true
        echo "service disabled"
    } >> "$LOG" 2>&1
) &
sleep 1
exit 0
EOF
chmod 0755 "$PORTS_DIR/Playora Autosync Disable.sh"
echo "[install] wrote $PORTS_DIR/Playora Autosync Disable.sh"

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

# Disable legacy R36S helper scripts that fight with Playora.
for s in "$SD/tools/R36S-Backup.sh" "$SD/tools/R36S-Search.sh" "$SD/tools/R36S-Install-Collections.sh" "$SD/tools/R36S-Smart.sh" "$SD/tools/R36S-Storage.sh"; do
    [ -f "$s" ] && mv "$s" "$s.disabled" 2>/dev/null && echo "[install] disabled $s"
done

find "$SD" -name "._*" -delete 2>/dev/null || true
find "$SD" -name ".DS_Store" -delete 2>/dev/null || true

# Drop a short README in _inbox so the user knows what to do with it.
cat > "$INBOX_DIR/README.txt" <<'EOF'
Drop ROM archives (.zip .7z .rar .tar.gz ...) or loose ROM files here.

Open EmulationStation → Ports → Playora Extract ROMs.

The agent extracts each archive, detects the system from the file extension
(.gba → gba, .smc → snes, .gen → megadrive, .nes → nes, etc.), and moves
each ROM into /roms/<system>/. Originals are removed once extraction is OK.

Reload the EmulationStation game list afterwards to see the new ROMs.
EOF

sync
echo
echo "Playora installed. Every Port is fire-and-forget + tty1 feedback. ES never freezes."
echo "Watch results live: ${PLAYORA_SERVER_URL:-http://192.168.3.82:8080}/dashboard"
