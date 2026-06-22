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

# Dual-mode port runner. Two modes:
#   tty: claim /dev/tty1, run command in foreground with colored output the
#        user can read on the R36S screen. ES is paused while a Port script
#        runs, so taking tty1 is safe. We restart ES on exit.
#   bg:  detached background (the legacy pattern) — fire-and-forget.
cat > "$PLAYORA_DIR/port-runner.sh" <<'RUNNER'
#!/bin/sh
# Args: MODE NAME CMD [TIMEOUT_SECONDS]
#   MODE = tty | bg
MODE="$1"; shift
NAME="$1"; shift
CMD="$1"; shift
TIMEOUT="${1:-30}"

SAFE="$(echo "$NAME" | tr ' /' '__')"
LOG="/roms/.playora/logs/${SAFE}_$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
AGENT="/roms/.playora/playora-agent --config /roms/.playora/agent.toml"
ESUDO="sudo"
[ "$EUID" = "0" ] && ESUDO=""

if [ "$MODE" = "tty" ]; then
    # Claim the framebuffer console.
    export TERM=linux
    $ESUDO chmod 666 /dev/tty1 /dev/uinput 2>/dev/null || true
    printf '\033c' > /dev/tty1
    exec </dev/tty1 >/dev/tty1 2>&1
else
    # Detach completely so ES keeps tty1 clean.
    exec </dev/null >>"$LOG" 2>&1
fi

log() {
    if [ "$MODE" = "tty" ]; then
        printf '\033[2m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*"
        echo "[$(date +%H:%M:%S)] $*" >> "$LOG"
    else
        echo "[$(date +%H:%M:%S)] $*"
    fi
}

# Restart EmulationStation at the end so the framebuffer is clean.
restart_es() {
    if [ "$MODE" = "tty" ]; then
        $ESUDO systemctl restart emulationstation 2>/dev/null \
            || $ESUDO systemctl restart emustation 2>/dev/null \
            || true
    fi
}

END_RC=1
trap '
    RC=$END_RC
    log "exit $RC"
    $AGENT activity-end "$NAME" "$RC" --log "$LOG" >/dev/null 2>&1 || true
    $AGENT sync >/dev/null 2>&1 || true
    restart_es
' EXIT INT TERM

if [ "$MODE" = "tty" ]; then
    printf '\033[1;35m╔══════════════════════════════════════════════════════╗\n'
    printf '║  \033[1;37mPLAYORA · %-43s\033[1;35m║\n' "$NAME"
    printf '╚══════════════════════════════════════════════════════╝\033[0m\n\n'
fi

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
    $NICE $IONICE $AGENT $CMD 2>&1 | tee -a "$LOG"
    END_RC=${PIPESTATUS:-$?}
else
    timeout "$TIMEOUT" $NICE $IONICE $AGENT $CMD 2>&1 | tee -a "$LOG"
    END_RC=${PIPESTATUS:-$?}
fi

if [ "$END_RC" = "0" ]; then
    printf '\n\033[1;32m  ✓ DONE  \033[0m exit 0\n'
else
    printf '\n\033[1;31m  ✗ FAIL  \033[0m exit %s\n' "$END_RC"
fi
log "> syncing to dashboard..."

if [ "$MODE" = "tty" ]; then
    printf '\n\033[2m  Returning to EmulationStation in 5s...\033[0m\n'
    sleep 5
fi
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

# Generator: per-port wrapper. mode=tty means the script claims /dev/tty1
# and shows colored output to the user; mode=bg detaches silently.
write_port() {
    name="$1"; cmd="$2"; timeout_s="${3:-30}"; mode="${4:-tty}"
    file="$PORTS_DIR/Playora ${name}.sh"
    if [ "$mode" = "tty" ]; then
        # Foreground: port-runner takes the tty + restarts ES on exit.
        cat > "$file" <<EOF
#!/bin/sh
exec /roms/.playora/port-runner.sh tty "${name}" "${cmd}" "${timeout_s}"
EOF
    else
        # Background: fire-and-forget so ES returns immediately.
        cat > "$file" <<EOF
#!/bin/sh
SETSID=\$(command -v setsid 2>/dev/null)
if [ -n "\$SETSID" ]; then
    \$SETSID nohup /roms/.playora/port-runner.sh bg "${name}" "${cmd}" "${timeout_s}" </dev/null >/dev/null 2>&1 &
else
    nohup /roms/.playora/port-runner.sh bg "${name}" "${cmd}" "${timeout_s}" </dev/null >/dev/null 2>&1 &
fi
sleep 1
exit 0
EOF
    fi
    chmod 0755 "$file"
    echo "[install] wrote ${file} (mode=${mode})"
    write_splash "${name}" "${cmd}" "${timeout_s}"
}

# name | command | timeout-seconds (or "none" for no timeout)
# tty mode → user sees colored output on the R36S screen
# bg  mode → fire-and-forget background job, dashboard tracks
write_port "Quick Sync"      "quick-sync"               45    bg
write_port "Doctor"          "doctor"                   30    tty
write_port "Hardware"        "hardware snapshot --pretty --save" 30 tty
write_port "Scan ROMs"       "scan"                     300   tty
write_port "Extract ROMs"    "extract-roms"             600   tty
write_port "Compress ROMs"   "compress-roms"            1800  tty
write_port "Restore Backup"  "restore-tar"              none  tty
write_port "Cleanup"         "cleanup"                  120   tty
write_port "Cloud Setup"     "cloud setup"              600   tty
write_port "Cloud Backup"    "cloud backup"             1200  bg
write_port "Cloud Restore"   "cloud restore"            1200  bg
write_port "Cloud Status"    "cloud status"             10    tty
write_port "Kodi Setup"      "kodi setup"               60    tty
write_port "Update"          "self-update"              180   tty
write_port "File Browser"    "serve"                    none  bg
write_port "Install Main Menu" "install-main-menu"      30    tty

# Autosync triple: Status / Enable / Disable
write_port "Autosync Status" "status"                   10    tty

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

# Per-command human-readable description shown in the Ports + Playora menus.
desc_for() {
    case "$1" in
        "Doctor") echo "Health check: storage, server, tools, RetroArch, autosync — full report on screen.";;
        "Hardware") echo "Show CPU/RAM/kernel/panel/disk/WiFi on screen and sync the snapshot to the dashboard.";;
        "Quick Sync") echo "Background: diagnostic + hardware snapshot + sync. Quick way to push state to the hub.";;
        "Scan ROMs") echo "Index every ROM in /roms. Incremental: re-runs skip files that haven't changed.";;
        "Extract ROMs") echo "Extract every archive in /roms/_inbox and route ROMs into /roms/<system>/ by file extension.";;
        "Compress ROMs") echo "Convert PSX/Saturn/Dreamcast/PSP/Wii images to CHD/CSO/RVZ. Smaller + RetroArch-native.";;
        "Restore Backup") echo "Extract /roms/playora_restore.tar idempotently — skips files already present.";;
        "Cleanup") echo "Apply pending deletions: /roms/.playora/delete_queue.txt + the dashboard delete queue.";;
        "Cloud Setup") echo "Pair Google Drive via QR. Scan the QR with your phone to open the dashboard setup page.";;
        "Cloud Backup") echo "Background: sync /roms/savestates and /roms/.playora to gdrive:R36S. Long-running.";;
        "Cloud Restore") echo "Background: pull savestates + config from gdrive:R36S back to the SD.";;
        "Cloud Status") echo "Print rclone version + configured remotes.";;
        "Kodi Setup") echo "Enable curated Kodi addons (YouTube, Jellyfin, IPTV Simple, IAGL) via JSON-RPC.";;
        "Update") echo "Self-update the agent from the GitHub release.";;
        "Autosync Status") echo "Show the autosync service status, pending events, and last sync time.";;
        "Autosync Enable") echo "Install + start the autosync systemd service so events sync continuously.";;
        "Autosync Disable") echo "Stop + disable the autosync service.";;
        "Recover") echo "Emergency: kill agent processes, clear locks, restart EmulationStation.";;
        "File Browser") echo "Start the on-device file server on :7878. Open the link on the dashboard Device page to browse /roms, upload (ZIPs auto-extract), download.";;
        "Install Main Menu") echo "Register the Playora tile in the EmulationStation Main Menu (edits es_systems.cfg with sudo, backs up first).";;
        *) echo "Playora command: $1.";;
    esac
}

write_gamelist() {
    local out_dir="$1"
    local gl="${out_dir}/gamelist.xml"
    echo '<?xml version="1.0"?>' > "$gl"
    echo '<gameList>' >> "$gl"
    for sh in "${out_dir}"/Playora\ *.sh; do
        [ -f "$sh" ] || continue
        base="$(basename "$sh")"
        name_only="$(basename "$sh" .sh)"
        short="${name_only#Playora }"
        png="./${name_only}.png"
        desc="$(desc_for "$short")"
        cat >> "$gl" <<XML
  <game>
    <path>./${base}</path>
    <name>Playora · ${short}</name>
    <desc>${desc}</desc>
    <image>${png}</image>
    <thumbnail>${png}</thumbnail>
  </game>
XML
    done
    echo '</gameList>' >> "$gl"
    echo "[install] wrote $gl"
}

write_gamelist "$PORTS_DIR"

# Main Menu integration: mirror the Playora ports into /roms/playora/ so a
# top-level "Playora" tile appears next to NES, SNES, etc. Same scripts —
# no duplication of logic, only the directory listing differs.
PLAYORA_SYS_DIR="$SD/playora"
mkdir -p "$PLAYORA_SYS_DIR"
# Sweep + copy current ports into the system folder.
find "$PLAYORA_SYS_DIR" -maxdepth 1 -type f \( -name "Playora *.sh" -o -name "Playora *.png" \) -delete 2>/dev/null || true
for sh in "$PORTS_DIR"/Playora\ *.sh; do
    [ -f "$sh" ] || continue
    cp "$sh" "$PLAYORA_SYS_DIR/"
    base="$(basename "$sh")"
    name_only="$(basename "$sh" .sh)"
    png="$PORTS_DIR/${name_only}.png"
    [ -f "$png" ] && cp "$png" "$PLAYORA_SYS_DIR/"
done
write_gamelist "$PLAYORA_SYS_DIR"
echo "[install] mirrored ports into $PLAYORA_SYS_DIR"

# Try to register the Playora system in es_systems.cfg. dArkOSRE typical
# locations — bail silently if we can't find one (the Ports menu still works).
for ES_CFG in \
    "$SD/configs/emulationstation/es_systems.cfg" \
    "$SD/system/configs/emulationstation/es_systems.cfg" \
    "$SD/.system/configs/emulationstation/es_systems.cfg" \
    "$SD/.r36s-smart/es_systems.cfg" \
    "$SD/configs/es_systems.cfg"; do
    if [ -f "$ES_CFG" ]; then
        if grep -q '<name>playora</name>' "$ES_CFG"; then
            echo "[install] playora system already present in $ES_CFG"
            break
        fi
        cp "$ES_CFG" "${ES_CFG}.playora-bak"
        # Insert our block before the closing </systemList>.
        awk '
            /<\/systemList>/ && !done {
                print "  <system>"
                print "    <name>playora</name>"
                print "    <fullname>Playora</fullname>"
                print "    <path>/roms/playora</path>"
                print "    <extension>.sh .SH</extension>"
                print "    <command>%ROM%</command>"
                print "    <theme>playora</theme>"
                print "  </system>"
                done = 1
            }
            { print }
        ' "${ES_CFG}.playora-bak" > "$ES_CFG"
        echo "[install] added playora system to $ES_CFG"
        break
    fi
done

sync
echo
echo "Playora installed. Every Port is fire-and-forget + tty1 feedback. ES never freezes."
echo "Watch results live: ${PLAYORA_SERVER_URL:-http://192.168.3.82:8080}/dashboard"
