#!/bin/sh
# Removes binary + systemd unit. Keeps config + DB unless --purge is given.
set -eu
PURGE=0
while [ $# -gt 0 ]; do
    case "$1" in
        --purge) PURGE=1 ;;
    esac
    shift
done
[ -f /etc/systemd/system/playora-agent.service ] && {
    sudo systemctl disable --now playora-agent.service || true
    sudo rm -f /etc/systemd/system/playora-agent.service
    sudo systemctl daemon-reload
}
sudo rm -f /usr/local/bin/playora-agent
[ "$PURGE" = "1" ] && rm -rf /roms/playora "$HOME/.playora"
echo "uninstalled"
