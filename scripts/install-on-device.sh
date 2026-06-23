#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/darkos"
WRAP="$HERE/darkos-view-wrap.sh"
GPTK="$HERE/darkos-view.gptk"

for f in "$BIN" "$WRAP" "$GPTK"; do
    [ -f "$f" ] || { echo "[err] missing payload: $f" >&2; exit 1; }
done

detect_os_root() {
    for c in /opt/dArkOSRE /opt/DarkOS /opt/ArkOS /opt/system; do
        [ -d "$c" ] && { echo "$c"; return; }
    done
    if [ -f /etc/os-release ]; then
        ID="$(grep -E '^ID=' /etc/os-release | cut -d= -f2 | tr -d '"')"
        case "$ID" in
            *darkosre*) echo /opt/dArkOSRE; return ;;
            *darkos*)   echo /opt/DarkOS;  return ;;
            *arkos*)    echo /opt/ArkOS;   return ;;
        esac
    fi
    echo /opt/system
}

detect_profile_dir() {
    for c in \
        "$1/gptokeyb/profiles" \
        "$1/configs/gptokeyb" \
        /etc/gptokeyb/profiles \
        /usr/local/share/gptokeyb/profiles; do
        if [ -d "$c" ] || mkdir -p "$c" 2>/dev/null; then
            echo "$c"; return
        fi
    done
    echo /etc/gptokeyb/profiles
}

OS_ROOT="$(detect_os_root)"
PROFILE_DIR="$(detect_profile_dir "$OS_ROOT")"
PORTS_DIR="/roms/ports"

install -m 0755 "$BIN"  /usr/local/bin/darkos
install -m 0755 "$WRAP" /usr/local/bin/darkos-view-wrap.sh
install -m 0644 "$GPTK" "$PROFILE_DIR/darkos-view.gptk"

mkdir -p "$PORTS_DIR"
cat > "$PORTS_DIR/darkOs Menu.sh" <<'EOF'
#!/bin/sh
exec darkos tui
EOF
cat > "$PORTS_DIR/darkOs System Log.sh" <<'EOF'
#!/bin/sh
exec /usr/local/bin/darkos-view-wrap.sh /var/log/messages
EOF
cat > "$PORTS_DIR/darkOs Self-Update.sh" <<'EOF'
#!/bin/sh
exec sudo darkos update --self
EOF
chmod 0755 "$PORTS_DIR/darkOs Menu.sh" "$PORTS_DIR/darkOs System Log.sh" "$PORTS_DIR/darkOs Self-Update.sh"

/usr/local/bin/darkos --version
echo "[ok] darkOs installed (OS root: $OS_ROOT, profile dir: $PROFILE_DIR)"
