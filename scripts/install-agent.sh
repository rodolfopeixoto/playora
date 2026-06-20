#!/bin/sh
# install-agent.sh — installs playora-agent binary + config on a Linux handheld
# Usage: sh scripts/install-agent.sh [--dry-run] [--server URL] [--bin PATH]
set -eu
DRY=0
SERVER="http://127.0.0.1:8080"
BIN_SRC="dist/playora-agent-aarch64"
while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) DRY=1 ;;
        --server) SERVER="$2"; shift ;;
        --bin) BIN_SRC="$2"; shift ;;
        *) echo "unknown arg: $1"; exit 1 ;;
    esac
    shift
done

ARCH="$(uname -m)"
echo "[install] arch=$ARCH"

if [ ! -f "$BIN_SRC" ]; then
    echo "[install] missing binary: $BIN_SRC (build first: sh scripts/build-arm64.sh)"; exit 1
fi

# pick dest dirs
if [ -d /roms ]; then
    BASE="/roms/playora"
else
    BASE="$HOME/.playora"
fi

INSTALL_BIN="/usr/local/bin/playora-agent"
CFG="$BASE/agent.toml"
DB="$BASE/playora.db"

run() { if [ "$DRY" = "1" ]; then echo "DRY: $*"; else eval "$*"; fi; }

run "mkdir -p '$BASE'"
run "install -m 0755 '$BIN_SRC' '$INSTALL_BIN'"

if [ ! -f "$CFG" ]; then
    run "playora-agent init --server-url '$SERVER'"
else
    echo "[install] existing config preserved at $CFG"
fi

# systemd (optional)
if [ -d /etc/systemd/system ] && [ "$(id -u)" = "0" ]; then
    SVC=/etc/systemd/system/playora-agent.service
    run "cat > $SVC <<EOF
[Unit]
Description=Playora agent
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/playora-agent run
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF"
    run "systemctl daemon-reload"
    run "systemctl enable playora-agent.service"
    echo "[install] systemd service enabled. Start with: systemctl start playora-agent"
else
    echo "[install] no systemd (or not root). Run agent manually:"
    echo "    playora-agent run &"
fi

echo "[install] done. config=$CFG db=$DB"
