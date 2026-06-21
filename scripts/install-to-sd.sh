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

# Sweep leftover Playora *.sh and old splash PNGs so install is idempotent.
find "$PORTS_DIR" -maxdepth 1 -type f \( -name "Playora *.sh*" -o -name "Playora *.png" \) -print -delete 2>/dev/null || true

cp "$BIN" "$PLAYORA_DIR/playora-agent"
chmod 0755 "$PLAYORA_DIR/playora-agent"

# Bundle rclone aarch64 (~18MB) for cloud sync. Cached in dist/ to avoid re-download.
RCLONE_CACHE="$ROOT/dist/rclone-aarch64"
mkdir -p "$PLAYORA_DIR/bin"
if [ ! -f "$RCLONE_CACHE" ]; then
    echo "[install] downloading rclone aarch64 (one-time, ~18MB)..."
    TMP_ZIP="$(mktemp -t rclone.XXXXXX.zip)"
    if curl -sSfL --max-time 120 -o "$TMP_ZIP" \
        "https://downloads.rclone.org/rclone-current-linux-arm64.zip"; then
        TMP_DIR="$(mktemp -d -t rclone-extract)"
        unzip -q "$TMP_ZIP" -d "$TMP_DIR"
        find "$TMP_DIR" -name rclone -type f -exec cp {} "$RCLONE_CACHE" \;
        chmod 0755 "$RCLONE_CACHE"
        rm -rf "$TMP_DIR" "$TMP_ZIP"
        echo "[install] cached: $RCLONE_CACHE"
    else
        echo "[install] WARN: rclone download failed — Cloud ports will print install hint"
        rm -f "$TMP_ZIP"
    fi
fi
if [ -f "$RCLONE_CACHE" ]; then
    cp "$RCLONE_CACHE" "$PLAYORA_DIR/bin/rclone"
    chmod 0755 "$PLAYORA_DIR/bin/rclone"
    echo "[install] rclone -> $PLAYORA_DIR/bin/rclone"
fi

# Shared helper: never touches any tty/framebuffer (ES owns /dev/tty1).
# All status goes to log file + dashboard via activity-end.
cat > "$PLAYORA_DIR/port-runner.sh" <<'RUNNER'
#!/bin/sh
# Args: NAME CMD [TIMEOUT_SECONDS]
# Runs in a detached session — no controlling terminal, no fd inherits.
NAME="$1"; shift
CMD="$1"; shift
TIMEOUT="${1:-30}"

SAFE="$(echo "$NAME" | tr ' /' '__')"
LOG="/roms/.playora/logs/${SAFE}_$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
AGENT="/roms/.playora/playora-agent --config /roms/.playora/agent.toml"

# Drop ALL inherited fds — never share stdin/stdout/stderr with ES.
exec </dev/null
exec >>"$LOG"
exec 2>&1

log() {
    echo "[$(date +%H:%M:%S)] $*"
}

# Trap guarantees activity-end fires even on timeout/kill.
END_RC=1
trap '
    RC=$END_RC
    log "exit $RC"
    $AGENT activity-end "$NAME" "$RC" --log "$LOG" >/dev/null 2>&1 || true
    $AGENT sync >/dev/null 2>&1 || true
' EXIT INT TERM

log "===== $NAME ====="
log "starting at $(date)"
log "command: $CMD"
log "timeout: ${TIMEOUT}s"
log "log: $LOG"

$AGENT activity-begin "$NAME" >/dev/null 2>&1 || true
log "> sent start event"

log "> running '$CMD' (max ${TIMEOUT}s)..."
NICE="nice -n 15"
if command -v ionice >/dev/null 2>&1; then
    IONICE="ionice -c 3"
else
    IONICE=""
fi
if [ "$TIMEOUT" = "none" ]; then
    $NICE $IONICE $AGENT $CMD
    END_RC=$?
else
    timeout "$TIMEOUT" $NICE $IONICE $AGENT $CMD
    END_RC=$?
fi

if [ "$END_RC" = "0" ]; then
    log "> ok"
else
    log "> FAILED (exit $END_RC)"
fi
log "> syncing to dashboard..."
exit $END_RC
RUNNER
chmod 0755 "$PLAYORA_DIR/port-runner.sh"

# Splash PNG generator: replaces the generic purple ES launch image with
# an informative card so the user sees what's happening during launch.
HAS_MAGICK=0
if command -v magick >/dev/null 2>&1; then
    HAS_MAGICK=1
elif command -v convert >/dev/null 2>&1; then
    HAS_MAGICK=2
fi
write_splash() {
    name="$1"; cmd="$2"; timeout_s="$3"
    out="$PORTS_DIR/Playora ${name}.png"
    case $HAS_MAGICK in
        1) MAGICK="magick" ;;
        2) MAGICK="convert" ;;
        *) return 0 ;;
    esac
    $MAGICK -size 640x480 \
        gradient:'#0a0a14-#1a0a2e' \
        -gravity North -fill '#7c9eff' -pointsize 18 -annotate +0+30 "PLAYORA" \
        -gravity Center -fill '#ffffff' -pointsize 42 -annotate +0-30 "${name}" \
        -gravity Center -fill '#9aa' -pointsize 16 -annotate +0+30 "command: ${cmd}" \
        -gravity Center -fill '#666' -pointsize 13 -annotate +0+60 "timeout: ${timeout_s}s · runs in background" \
        -gravity South -fill '#42a5f5' -pointsize 14 -annotate +0+40 "see hub for live status" \
        -gravity South -fill '#555' -pointsize 11 -annotate +0+18 "192.168.3.82:8080/dashboard" \
        "$out" 2>/dev/null && echo "[install] splash: $(basename "$out")"
}

# Generator: each port is a thin wrapper that backgrounds port-runner.sh
# and exits within 1 second so EmulationStation never freezes.
write_port() {
    name="$1"; cmd="$2"; timeout_s="${3:-30}"
    file="$PORTS_DIR/Playora ${name}.sh"
    # setsid fully detaches child from the controlling tty so ES keeps clean
    # ownership of /dev/tty1. nohup ignores HUP. All fds redirected to /dev/null
    # so child never inherits ES's stdout/stderr.
    cat > "$file" <<EOF
#!/bin/sh
SETSID=\$(command -v setsid 2>/dev/null)
if [ -n "\$SETSID" ]; then
    \$SETSID nohup /roms/.playora/port-runner.sh "${name}" "${cmd}" "${timeout_s}" </dev/null >/dev/null 2>&1 &
else
    nohup /roms/.playora/port-runner.sh "${name}" "${cmd}" "${timeout_s}" </dev/null >/dev/null 2>&1 &
fi
sleep 1
exit 0
EOF
    chmod 0755 "$file"
    echo "[install] wrote ${file}"
    write_splash "${name}" "${cmd}" "${timeout_s}"
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
write_port "Compress ROMs"   "compress-roms"            1800
write_port "Cloud Setup"     "cloud setup"              600
write_port "Cloud Backup"    "cloud backup"             1200
write_port "Cloud Restore"   "cloud restore"            1200
write_port "Cloud Status"    "cloud status"             10
write_port "Cleanup"         "cleanup"                  120

# Autosync triple: Status / Enable / Disable
write_port "Autosync Status" "status"                   10

cat > "$PORTS_DIR/Playora Autosync Enable.sh" <<'EOF'
#!/bin/sh
SETSID=$(command -v setsid 2>/dev/null)
detach() {
    if [ -n "$SETSID" ]; then
        $SETSID nohup "$@" </dev/null >/dev/null 2>&1 &
    else
        nohup "$@" </dev/null >/dev/null 2>&1 &
    fi
}
detach /roms/.playora/port-runner.sh "Autosync Enable" "noop" 60
detach sh -c '
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
'
sleep 1
exit 0
EOF
chmod 0755 "$PORTS_DIR/Playora Autosync Enable.sh"
echo "[install] wrote $PORTS_DIR/Playora Autosync Enable.sh"
write_splash "Autosync Enable" "systemd enable + start" "60"

cat > "$PORTS_DIR/Playora Autosync Disable.sh" <<'EOF'
#!/bin/sh
SETSID=$(command -v setsid 2>/dev/null)
detach() {
    if [ -n "$SETSID" ]; then
        $SETSID nohup "$@" </dev/null >/dev/null 2>&1 &
    else
        nohup "$@" </dev/null >/dev/null 2>&1 &
    fi
}
detach /roms/.playora/port-runner.sh "Autosync Disable" "noop" 60
detach sh -c '
    sleep 2
    LOG="/roms/.playora/logs/autosync_disable_$(date +%Y%m%d_%H%M%S).log"
    {
        echo "==== $(date) ===="
        sudo systemctl disable --now playora-agent.service 2>/dev/null || true
        pkill -f "playora-agent.*run" 2>/dev/null || true
        echo "service disabled"
    } >> "$LOG" 2>&1
'
sleep 1
exit 0
EOF
chmod 0755 "$PORTS_DIR/Playora Autosync Disable.sh"
echo "[install] wrote $PORTS_DIR/Playora Autosync Disable.sh"
write_splash "Autosync Disable" "systemd disable + stop" "60"

cat > "$PORTS_DIR/Playora Recover.sh" <<'EOF'
#!/bin/sh
SETSID=$(command -v setsid 2>/dev/null)
[ -n "$SETSID" ] && PREFIX="$SETSID nohup" || PREFIX="nohup"
$PREFIX sh -c '
    LOG="/roms/.playora/logs/recover_$(date +%Y%m%d_%H%M%S).log"
    mkdir -p /roms/.playora/logs
    {
        echo "==== $(date) ===="
        sudo killall -9 playora-agent 2>/dev/null
        sudo killall -9 gptokeyb 2>/dev/null
        rm -f /tmp/playora-*.lock 2>/dev/null
        sudo systemctl restart emulationstation 2>/dev/null \
            || sudo systemctl start emulationstation 2>/dev/null \
            || (cd /; nohup emulationstation >/dev/null 2>&1 &)
        echo "recover done"
    } > "$LOG" 2>&1
' </dev/null >/dev/null 2>&1 &
sleep 1
exit 0
EOF
chmod 0755 "$PORTS_DIR/Playora Recover.sh"
echo "[install] wrote $PORTS_DIR/Playora Recover.sh"
write_splash "Recover" "kill agent + restart ES" "30"

QUEUE="$PLAYORA_DIR/delete_queue.txt"
if [ ! -f "$QUEUE" ]; then
    cat > "$QUEUE" <<'EOF'
# Playora — Delete Queue
# One absolute path per line. Lines starting with # are ignored.
# After editing, run "Playora Cleanup" from Ports (or wait for the autosync
# service to process them every ~60s). Paths must be under /roms/.
#
# Examples:
# /roms/snes/Old Game (Bad Dump).smc
# /roms/psx/.duplicate
EOF
fi

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

# Daily scheduled jobs (UTC hour 0-23). Comment out to disable.
# Cloud Backup at 06:00 UTC (≈ 03:00 BRT) — drop overnight, autosync prevents suspend.
cloud_backup_daily_hour_utc = 6
# Scan ROMs at 05:00 UTC — finishes before backup.
scan_daily_hour_utc = 5
# Auto-extract anything in /roms/_inbox at 04:00 UTC.
extract_roms_daily_hour_utc = 4
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

# Write gamelist.xml so EmulationStation uses our PNG splash + description per Port.
GAMELIST="$PORTS_DIR/gamelist.xml"
echo '<?xml version="1.0"?>' > "$GAMELIST"
echo '<gameList>' >> "$GAMELIST"
for sh in "$PORTS_DIR"/Playora\ *.sh; do
    [ -f "$sh" ] || continue
    base="$(basename "$sh")"
    name_only="$(basename "$sh" .sh)"
    short="${name_only#Playora }"
    png="./${name_only}.png"
    cat >> "$GAMELIST" <<XML
  <game>
    <path>./${base}</path>
    <name>Playora · ${short}</name>
    <desc>Background task. Runs the playora-agent ${short} command. Watch live status + log tail on the dashboard at ${PLAYORA_SERVER_URL:-http://192.168.3.82:8080}/dashboard — every click shows up there within seconds. EmulationStation returns immediately; never wait on a black screen.</desc>
    <image>${png}</image>
    <thumbnail>${png}</thumbnail>
  </game>
XML
done
echo '</gameList>' >> "$GAMELIST"
echo "[install] wrote $GAMELIST"

sync
echo
echo "Playora installed. Every Port is fire-and-forget + tty1 feedback. ES never freezes."
echo "Watch results live: ${PLAYORA_SERVER_URL:-http://192.168.3.82:8080}/dashboard"
