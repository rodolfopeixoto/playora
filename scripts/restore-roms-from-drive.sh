#!/bin/sh
# Restore ROMs + saves + themes from the full SD backup on Google Drive.
# Strategy:
#   1) Download r36s-backup-*.img.gz from gdrive:r36s-backups/ (~39 GB compressed)
#   2) Extract image
#   3) Mount the EASYROMS partition (offset 9130999808 bytes, exFAT) read-only
#   4) rsync ROMs + savestates + tools + .r36s-smart into a destination dir
#
# Destination is configurable:
#   DEST=/Volumes/EASYROMS sh scripts/restore-roms-from-drive.sh  (if SD mounted)
#   DEST=~/r36s-restore sh scripts/restore-roms-from-drive.sh    (to local dir)
set -eu

DEST="${DEST:?DEST env var required, e.g. /Volumes/EASYROMS or ~/r36s-restore}"
WORK="${WORK:-$HOME/R36S_DARKOS_TEST/99_restore}"
mkdir -p "$WORK" "$DEST"

REMOTE="gdrive:r36s-backups/"
NAME="$(rclone lsf "$REMOTE" | grep '\.img\.gz$' | head -1)"
[ -n "$NAME" ] || { echo "no .img.gz in $REMOTE"; exit 1; }

GZ="$WORK/$NAME"
IMG="$WORK/${NAME%.gz}"

if [ ! -f "$GZ" ]; then
    echo "[restore] downloading $NAME (~39 GB)"
    rclone copy --progress "$REMOTE$NAME" "$WORK/"
fi

if [ ! -f "$IMG" ]; then
    echo "[restore] decompressing"
    if command -v pigz >/dev/null 2>&1; then pigz -dc "$GZ" > "$IMG"
    else gunzip -c "$GZ" > "$IMG"; fi
fi

EASYROMS_OFFSET=9130999808
case "$(uname -s)" in
    Darwin)
        echo "[restore] attaching disk image (macOS)"
        DEV="$(hdiutil attach -imagekey diskimage-class=CRawDiskImage -nomount "$IMG" | head -1 | awk '{print $1}')"
        echo "[restore] attached at $DEV"
        EASY_DEV="${DEV}s3"
        MNT="$WORK/mnt-easyroms"
        mkdir -p "$MNT"
        echo "[restore] mounting EASYROMS read-only at $MNT"
        sudo mount -t exfat -o ro,nobrowse "$EASY_DEV" "$MNT" 2>/dev/null || \
            sudo mount_exfat -o ro "$EASY_DEV" "$MNT"
        ;;
    Linux)
        LOOP="$(sudo losetup --find --show -o $EASYROMS_OFFSET "$IMG")"
        MNT="$WORK/mnt-easyroms"
        mkdir -p "$MNT"
        sudo mount -o ro "$LOOP" "$MNT"
        ;;
    *) echo "unsupported OS"; exit 1 ;;
esac

echo "[restore] rsyncing into $DEST"
rsync -a --info=progress2 \
    "$MNT/savestates" \
    "$MNT/tools" \
    "$MNT/.r36s-smart" 2>/dev/null \
    "$MNT/themes" \
    "$MNT/bgmusic" \
    "$MNT/BGM" \
    "$MNT/cheats" \
    "$DEST/" || true

# ROM dirs — copy each system dir
for sys in nes snes gb gbc gba megadrive genesis n64 psx psp dreamcast saturn arcade mame neogeo pcengine mastersystem gamegear; do
    if [ -d "$MNT/$sys" ]; then
        echo "[restore] $sys"
        rsync -a --info=progress2 "$MNT/$sys" "$DEST/"
    fi
done

echo "[restore] unmounting"
case "$(uname -s)" in
    Darwin) sudo umount "$MNT" 2>/dev/null || true; hdiutil detach "$DEV" 2>/dev/null || true ;;
    Linux)  sudo umount "$MNT"; sudo losetup -d "$LOOP" ;;
esac

echo "[restore] done"
du -sh "$DEST"
