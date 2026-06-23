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

# Single-mode foreground port runner — matches the PortMaster / ThemeMaster
# convention. Runs synchronously on /dev/tty1 so ES always sees a normal
# child-script lifecycle, then restarts ES on exit. Never tries to detach.
#
# Args: NAME CMD [TIMEOUT_SECONDS]
cat > "$PLAYORA_DIR/port-runner.sh" <<'RUNNER'
#!/bin/sh
# Single source of truth for every Playora port.
# Hard guarantees:
#   1. Console NEVER stays black: ES is restarted no matter what fails.
#   2. No interactive step blocks indefinitely (timeouts everywhere).
#   3. A safety-net watchdog forces ES restart after MAX_TOTAL seconds
#      regardless of what the script is doing.
#   4. Activity events still flush so the dashboard sees the result.

NAME="$1"; shift
CMD="$1"; shift
TIMEOUT="${1:-30}"

# Total wall-clock budget: command runtime + interactive review.
# Even Restore Backup ("none") gets capped at 2 hours by the watchdog.
case "$TIMEOUT" in
    none) MAX_TOTAL=7200 ;;
    *)    MAX_TOTAL=$(( TIMEOUT + 300 )) ;;
esac

SAFE="$(echo "$NAME" | tr ' /' '__')"
LOG="/roms/.playora/logs/${SAFE}_$(date +%Y%m%d_%H%M%S).log"
mkdir -p /roms/.playora/logs
AGENT="/roms/.playora/playora-agent --config /roms/.playora/agent.toml"
ESUDO="sudo"
[ "$(id -u)" = "0" ] && ESUDO=""

# Detect a usable framebuffer console. Prefer /dev/tty1, fall back to tty0.
TTY=/dev/tty1
[ -c "$TTY" ] || TTY=/dev/tty0
export TERM=linux
$ESUDO chmod 666 "$TTY" /dev/uinput 2>/dev/null || true
printf '\033c' > "$TTY" 2>/dev/null
exec <"$TTY" >"$TTY" 2>&1

# Detect ES service name dynamically (different forks use different names).
ES_SERVICE=""
for s in emulationstation emustation oga_es; do
    systemctl list-unit-files "${s}.service" 2>/dev/null | grep -q "$s" && ES_SERVICE="$s" && break
done

restart_es() {
    stop_gptokeyb
    clear > "$TTY" 2>/dev/null
    if [ -n "$ES_SERVICE" ]; then
        $ESUDO systemctl restart "$ES_SERVICE" 2>/dev/null
    fi
    # Always also try the historical names — defence in depth.
    $ESUDO systemctl restart emulationstation 2>/dev/null
    $ESUDO systemctl restart emustation 2>/dev/null
    # Last resort: spawn ES directly if no service manages it.
    if ! pgrep -x emulationstation >/dev/null 2>&1; then
        command -v emulationstation >/dev/null && \
            ($ESUDO nohup emulationstation </dev/null >/dev/null 2>&1 &)
    fi
}

# WATCHDOG: nothing on this device should keep the screen captive for
# longer than MAX_TOTAL seconds. If we ever do, kill the script's process
# group + restart ES so the user is back in the menu.
SCRIPT_PID=$$
(
    sleep "$MAX_TOTAL"
    echo "[$(date +%H:%M:%S)] WATCHDOG: ${MAX_TOTAL}s elapsed, forcing ES restart." >> "$LOG"
    $ESUDO systemctl restart emulationstation 2>/dev/null \
        || $ESUDO systemctl restart emustation 2>/dev/null \
        || true
    kill -KILL -- -"$SCRIPT_PID" 2>/dev/null
) &
WATCHDOG_PID=$!

# gptokeyb discovery + lifecycle (PortMaster pattern).
GPTOKEYB_BIN=""
for c in \
    /opt/system/Tools/PortMaster/gptokeyb/gptokeyb \
    /opt/tools/PortMaster/gptokeyb/gptokeyb \
    /roms/ports/PortMaster/gptokeyb/gptokeyb \
    /usr/local/bin/gptokeyb \
    /usr/bin/gptokeyb; do
    [ -x "$c" ] && GPTOKEYB_BIN="$c" && break
done
KEYS_GPTK="/roms/.playora/keys.gptk"
GPTOKEYB_PID=""
start_gptokeyb() {
    [ -n "$GPTOKEYB_BIN" ] && [ -f "$KEYS_GPTK" ] || return
    $ESUDO killall -9 gptokeyb gptokeyb2 2>/dev/null
    SDL_GAMECONTROLLERCONFIG_FILE="" "$GPTOKEYB_BIN" -1 "playora" \
        -c "$KEYS_GPTK" </dev/null >/dev/null 2>&1 &
    GPTOKEYB_PID=$!
}
stop_gptokeyb() {
    [ -n "$GPTOKEYB_PID" ] && kill "$GPTOKEYB_PID" 2>/dev/null
    $ESUDO killall -9 gptokeyb gptokeyb2 2>/dev/null
}

cleanup() {
    rc=${END_RC:-1}
    kill "$WATCHDOG_PID" 2>/dev/null
    stop_gptokeyb
    $AGENT activity-end "$NAME" "$rc" --log "$LOG" >/dev/null 2>&1
    $AGENT sync >/dev/null 2>&1
    restart_es
}
trap cleanup EXIT INT TERM HUP

start_gptokeyb

printf '\033[1;35m╔══════════════════════════════════════════════════════╗\n'
printf '║  \033[1;37mPLAYORA · %-43s\033[1;35m║\n' "$NAME"
printf '╚══════════════════════════════════════════════════════╝\033[0m\n\n'

echo "[$(date +%H:%M:%S)] command: $CMD" | tee -a "$LOG"
echo "[$(date +%H:%M:%S)] timeout:  ${TIMEOUT}s" | tee -a "$LOG"
echo "[$(date +%H:%M:%S)] watchdog: ${MAX_TOTAL}s total budget" | tee -a "$LOG"
echo "[$(date +%H:%M:%S)] tty:      $TTY" | tee -a "$LOG"
echo "[$(date +%H:%M:%S)] es svc:   ${ES_SERVICE:-(auto)}" | tee -a "$LOG"
echo "[$(date +%H:%M:%S)] gptokeyb: ${GPTOKEYB_BIN:-(missing, dialog wont accept gamepad)}" | tee -a "$LOG"

# Verify the agent binary at all — fail loudly rather than silently.
if [ ! -x /roms/.playora/playora-agent ]; then
    printf '\n\033[1;31m  ✗ /roms/.playora/playora-agent missing or not executable.\033[0m\n'
    END_RC=127
    sleep 5
    exit "$END_RC"
fi

$AGENT activity-begin "$NAME" >/dev/null 2>&1 || true

NICE="nice -n 15"
command -v ionice >/dev/null 2>&1 && IONICE="ionice -c 3" || IONICE=""

if [ "$TIMEOUT" = "none" ]; then
    $NICE $IONICE $AGENT $CMD 2>&1 | tee -a "$LOG"
    END_RC=${PIPESTATUS:-$?}
else
    timeout --kill-after=10 "$TIMEOUT" $NICE $IONICE $AGENT $CMD 2>&1 | tee -a "$LOG"
    END_RC=${PIPESTATUS:-$?}
fi

if [ "$END_RC" = "0" ]; then
    printf '\n\033[1;32m  ✓ DONE  \033[0m exit 0\n' | tee -a "$LOG"
else
    printf '\n\033[1;31m  ✗ FAIL  \033[0m exit %s\n' "$END_RC" | tee -a "$LOG"
fi

# Interactive review. Everything bounded. Worst case: 30s without input
# → auto-restart ES. Never blocks indefinitely.
HAVE_DIALOG=0
command -v dialog >/dev/null 2>&1 && HAVE_DIALOG=1

if [ "$HAVE_DIALOG" = "1" ] && [ -n "$GPTOKEYB_BIN" ]; then
    # Full menu — bounded to 60s of inactivity.
    choice=$(timeout 60 dialog --no-mouse --keep-tite \
        --timeout 60 \
        --backtitle "Playora · $NAME (exit $END_RC)" \
        --title " Done — D-Pad + A " \
        --menu "Choose what to do next:" 14 60 4 \
            restart "↻  Restart EmulationStation (default)" \
            view    "📜  View full log (scrollable)" \
            stay    "💤  Drop to shell (advanced)" \
        2>&1 >"$TTY") || choice="restart"
    case "$choice" in
        view)
            timeout 120 dialog --no-mouse --keep-tite \
                --backtitle "Playora · $NAME" \
                --title "Log — $(basename "$LOG")" \
                --textbox "$LOG" 0 0 2>"$TTY" || true
            ;;
        stay)
            clear > "$TTY"
            printf 'Shell — type exit to restart ES.\n' > "$TTY"
            timeout 600 $ESUDO /bin/sh <"$TTY" >"$TTY" 2>&1 || true
            ;;
    esac
    exit "$END_RC"
fi

# Fallback: print log tail + bounded countdown, no deps.
printf '\n----- last 20 lines of the log -----\n'
tail -n 20 "$LOG" 2>/dev/null
printf '\n--------------------------------------\n\n'
printf '\033[1;37m  Returning to EmulationStation in '
for i in 10 9 8 7 6 5 4 3 2 1; do
    printf '\b\b\b%2d ' "$i"
    sleep 1
done
printf '\b\b\bnow.\033[0m\n'
exit "$END_RC"
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

# Generator: every Playora port is a single foreground exec into port-runner.
# Matches PortMaster/ThemeMaster convention so ES never has to redraw a
# half-detached child.
write_port() {
    name="$1"; cmd="$2"; timeout_s="${3:-30}"; mode="${4:-tty}"
    _=$mode
    file="$PORTS_DIR/Playora ${name}.sh"
    cat > "$file" <<EOF
#!/bin/sh
exec /roms/.playora/port-runner.sh "${name}" "${cmd}" "${timeout_s}"
EOF
    chmod 0755 "$file"
    echo "[install] wrote ${file}"
    write_splash "${name}" "${cmd}" "${timeout_s}"
}

# name | command | timeout-seconds (or "none" for no timeout)
# tty mode → user sees colored output on the R36S screen
# bg  mode → fire-and-forget background job, dashboard tracks
write_port "Doctor"          "doctor"                            30    tty
write_port "Fix Exit-Game"   "fix-exit-game --apply"             60    tty
write_port "Check Exit-Game" "fix-exit-game"                     30    tty
write_port "Hardware"        "hardware snapshot --pretty --save" 30    tty
write_port "Scan ROMs"       "scan"                              300   tty
write_port "Extract ROMs"    "extract-roms"                      600   tty
write_port "Compress ROMs"   "compress-roms"                     1800  tty
write_port "Restore Backup"  "restore-tar"                       none  tty
write_port "Cleanup"         "cleanup"                           120   tty
write_port "Cloud Setup"     "cloud setup"                       600   tty
write_port "Cloud Backup"    "cloud backup"                      1200  tty
write_port "Cloud Restore"   "cloud restore"                     1200  tty
write_port "Cloud Catalog"   "cloud catalog"                     300   tty
write_port "Fetch Covers"    "fetch-covers"                      300   tty
write_port "Kodi Setup"      "kodi setup"                        60    tty
write_port "Update"          "self-update"                       180   tty

# Autosync Enable / Disable use the same foreground port-runner pattern.
# The systemd unit work lives in the agent's autosync-enable subcommand.
write_port "Autosync Enable"  "autosync-enable"  60   tty
write_port "Autosync Disable" "autosync-disable" 30   tty

write_port "Recover" "recover" 30 tty

# Default gptokeyb mapping so dialog menus respond to the gamepad.
# Mirrors PortMaster's defaults: A=Enter, B=Esc, D-Pad=arrows, Start=Enter,
# Select=Tab. Override per-port by dropping a custom keys.gptk in
# /roms/.playora/ (port-runner picks up the same file).
KEYS_GPTK="$PLAYORA_DIR/keys.gptk"
cat > "$KEYS_GPTK" <<'EOF'
back = esc
guide = enter
a = enter
b = esc
x = backspace
y = space
start = enter
select = tab
l1 = pageup
r1 = pagedown
l2 = home
r2 = end
left_analog_up = up
left_analog_down = down
left_analog_left = left
left_analog_right = right
dpup = up
dpdown = down
dpleft = left
dpright = right
EOF

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
    SERVER_URL="${PLAYORA_SERVER_URL:-auto}"
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
# Cover lookup right after scan finishes — fills missing thumbnails.
fetch_covers_daily_hour_utc = 7
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
        "Fix Exit-Game") echo "Patch retroarch.cfg to fix the Select+Start exit-freeze on R36S: video_threaded, audio_driver=alsathread, pause_nonactive, quit combo. Backs up first. Reboot to apply.";;
        "Check Exit-Game") echo "Show which retroarch.cfg settings would be changed by Fix Exit-Game. Dry run, no writes.";;
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
        "Cloud Catalog") echo "Refresh the cloud ROM catalog from gdrive (lsjson + post). Lets the dashboard show every ROM you own across systems with one-click Download.";;
        "Fetch Covers") echo "Look up missing covers + metadata for every scanned ROM via TheGamesDB. Rate-limited to 50 per run. Re-run as needed.";;
        "Kodi Setup") echo "Enable curated Kodi addons (YouTube, Jellyfin, IPTV Simple, IAGL) via JSON-RPC.";;
        "Update") echo "Self-update the agent from the GitHub release. After it finishes, the new ports + features appear on next ES reload.";;
        "Autosync Enable") echo "Install + start the autosync systemd service so events sync continuously and the file browser + game-session tracker run in the background.";;
        "Autosync Disable") echo "Stop + disable the autosync service.";;
        "Recover") echo "Emergency: kill agent processes, clear locks, restart EmulationStation.";;
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
    # Hide non-Playora ports (PortMaster, Counter-Strike, etc) from this menu
    # so the user sees only the Playora entries on the Ports tab.
    for sh in "${out_dir}"/*.sh; do
        [ -f "$sh" ] || continue
        base="$(basename "$sh")"
        case "$base" in
            Playora\ *) continue ;;
        esac
        cat >> "$gl" <<XML
  <game>
    <path>./${base}</path>
    <hidden>true</hidden>
  </game>
XML
    done
    echo '</gameList>' >> "$gl"
    echo "[install] wrote $gl"
}

write_gamelist "$PORTS_DIR"

# PortMaster installs other .sh entries (Counter-Strike, etc) right in
# /roms/ports/. write_gamelist marks them <hidden>true</hidden>, but some
# ES builds also need .skyscraper-ignore or just bail on hidden entries.
# Belt-and-suspenders: drop a .playora-hidden manifest + nudge ES to
# re-read the gamelist on next boot via .gamelist-needs-refresh marker.
{
    for sh in "$PORTS_DIR"/*.sh; do
        [ -f "$sh" ] || continue
        base="$(basename "$sh")"
        case "$base" in
            Playora\ *) continue ;;
            *) echo "$base" ;;
        esac
    done
} > "$PORTS_DIR/.playora-hidden"
touch "$PORTS_DIR/.gamelist-needs-refresh"

# Clean up any leftover /roms/playora/ mirror + es_systems.cfg edits from v0.2.
# Playora lives in /roms/ports/ like PortMaster / ThemeMaster.
if [ -d "$SD/playora" ]; then
    rm -rf "$SD/playora"
    echo "[install] removed legacy /roms/playora/ mirror"
fi
for ES_CFG in \
    "$SD/configs/emulationstation/es_systems.cfg" \
    "$SD/system/configs/emulationstation/es_systems.cfg" \
    "$SD/.system/configs/emulationstation/es_systems.cfg" \
    "$SD/.r36s-smart/es_systems.cfg" \
    "$SD/configs/es_systems.cfg"; do
    if [ -f "$ES_CFG" ] && grep -q '<name>playora</name>' "$ES_CFG"; then
        # Reverse the v0.2 merge — drop our system block.
        cp "$ES_CFG" "${ES_CFG}.playora-bak-uninstall"
        awk '
            /<system>/ { buf = $0; in_sys = 1; next }
            in_sys && /<name>playora<\/name>/ { drop = 1 }
            in_sys && /<\/system>/ {
                if (!drop) print buf;
                buf = ""; in_sys = 0; drop = 0; next
            }
            in_sys { buf = buf "\n" $0; next }
            { print }
        ' "${ES_CFG}.playora-bak-uninstall" > "$ES_CFG"
        echo "[install] removed legacy playora system block from $ES_CFG"
    fi
done

sync
echo
echo "Playora installed. Every Port is fire-and-forget + tty1 feedback. ES never freezes."
echo "Watch results live: ${PLAYORA_SERVER_URL:-http://192.168.3.82:8080}/dashboard"
