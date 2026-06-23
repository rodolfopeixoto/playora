#!/bin/sh
# Playora port-runner — single source of truth for every Playora port.
#
# Hard guarantees:
#   1. Console NEVER stays black: ES is restarted no matter what fails.
#   2. No interactive step blocks indefinitely (timeouts everywhere).
#   3. A watchdog forces ES restart after MAX_TOTAL seconds regardless.
#   4. Activity events still flush so the dashboard sees the result.
#
# Args: NAME CMD [TIMEOUT_SECONDS|none]
#
# Installed to /roms/.playora/port-runner.sh by scripts/install-to-sd.sh.

NAME="$1"; shift
CMD="$1"; shift
TIMEOUT="${1:-30}"

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

TTY=/dev/tty1
[ -c "$TTY" ] || TTY=/dev/tty0
export TERM=linux
$ESUDO chmod 666 "$TTY" /dev/uinput 2>/dev/null || true
printf '\033c' > "$TTY" 2>/dev/null
exec <"$TTY" >"$TTY" 2>&1

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
    $ESUDO systemctl restart emulationstation 2>/dev/null
    $ESUDO systemctl restart emustation 2>/dev/null
    if ! pgrep -x emulationstation >/dev/null 2>&1; then
        command -v emulationstation >/dev/null && \
            ($ESUDO nohup emulationstation </dev/null >/dev/null 2>&1 &)
    fi
}

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

HAVE_DIALOG=0
command -v dialog >/dev/null 2>&1 && HAVE_DIALOG=1

if [ "$HAVE_DIALOG" = "1" ] && [ -n "$GPTOKEYB_BIN" ]; then
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
