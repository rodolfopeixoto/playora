#!/bin/sh
# One-shot restore on macOS: pull SD backup from Google Drive, mount, copy ROMs/saves
# straight into the EASYROMS partition of the SD that's currently in the Mac.
#
# Requires: rclone (with `gdrive:` configured), pigz (optional), hdiutil (macOS).
# The SD must be plugged in. Script auto-detects /Volumes/EASYROMS.
#
# Steps:
#   1. Find the SD (must have a partition labeled EASYROMS).
#   2. Download the latest r36s-backup-*.img.gz from gdrive:r36s-backups/.
#   3. Decompress to a .img.
#   4. Attach the image (read-only) via hdiutil → exposes 3rd partition (EASYROMS, exFAT).
#   5. rsync ROMs + saves + tools + themes from image's EASYROMS to SD's EASYROMS.
#   6. Detach image. Eject SD.
#
# Idempotent: skips download/decompress if cached files exist with correct size.
set -eu

WORK="${WORK:-$HOME/R36S_DARKOS_TEST/99_restore}"
mkdir -p "$WORK"

SD_VOLUME="${SD_VOLUME:-/Volumes/EASYROMS}"
if [ ! -d "$SD_VOLUME" ]; then
    echo "[restore] $SD_VOLUME not mounted. Insert the dArkOSRE SD card and try again."
    diskutil list external | grep -E "EASYROMS|BOOT" || true
    exit 1
fi

# --- 1) Locate latest backup on Drive
REMOTE="gdrive:r36s-backups/"
NAME=$(rclone lsf "$REMOTE" | grep '\.img\.gz$' | head -1)
[ -n "$NAME" ] || { echo "[restore] no .img.gz on $REMOTE"; exit 1; }
REMOTE_SIZE=$(rclone size "$REMOTE$NAME" --json 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['bytes'])")
GZ="$WORK/$NAME"
IMG="$WORK/${NAME%.gz}"

# --- 2) Download
if [ -f "$GZ" ] && [ "$(stat -f '%z' "$GZ")" = "$REMOTE_SIZE" ]; then
    echo "[restore] $GZ already cached (matches size)"
else
    echo "[restore] downloading $NAME (~$((REMOTE_SIZE/1024/1024/1024)) GB)"
    rclone copy --progress "$REMOTE$NAME" "$WORK/"
fi

# --- 3) Decompress
if [ ! -f "$IMG" ] || [ "$(stat -f '%z' "$IMG")" -lt 1000000000 ]; then
    echo "[restore] decompressing"
    if command -v pigz >/dev/null 2>&1; then pigz -dc "$GZ" > "$IMG"
    else gunzip -c "$GZ" > "$IMG"; fi
fi
echo "[restore] image size: $(stat -f '%z' "$IMG") bytes"

# --- 4) Attach image
echo "[restore] attaching image (read-only)"
DEV_LINE=$(hdiutil attach -imagekey diskimage-class=CRawDiskImage -nomount "$IMG" | head -1)
DEV=$(echo "$DEV_LINE" | awk '{print $1}')
echo "[restore] attached at $DEV"
EASY_DEV="${DEV}s3"

MNT_SRC="$WORK/mnt-src-easyroms"
mkdir -p "$MNT_SRC"
echo "[restore] mounting source EASYROMS at $MNT_SRC"
diskutil mountDisk readOnly "$DEV" >/dev/null 2>&1 || true
# After mountDisk, look up by label
SRC_AUTOMOUNT=$(diskutil info "$EASY_DEV" 2>/dev/null | awk -F: '/Mount Point/{print $2}' | xargs)
if [ -n "$SRC_AUTOMOUNT" ] && [ -d "$SRC_AUTOMOUNT" ]; then
    MNT_SRC="$SRC_AUTOMOUNT"
    echo "[restore] auto-mounted at $MNT_SRC"
else
    # Manual mount via sudo as exfat
    sudo /sbin/mount -t exfat -o ro,nobrowse "$EASY_DEV" "$MNT_SRC" 2>/dev/null || \
    sudo /sbin/mount_exfat -o ro "$EASY_DEV" "$MNT_SRC"
fi

# --- 5) rsync into SD
echo "[restore] copying ROMs + saves into $SD_VOLUME (this may take 20-60 min on USB)"
TOPLEVEL_KEEPS="savestates tools .r36s-smart themes bgmusic BGM cheats"
for d in $TOPLEVEL_KEEPS; do
    if [ -d "$MNT_SRC/$d" ]; then
        echo "  rsync $d/"
        rsync -a --info=progress2 "$MNT_SRC/$d" "$SD_VOLUME/"
    fi
done

# All system folders
SYSTEMS="3do alg amiga amigacd32 amstradcpc apple2 arcade arduboy astrocde atari2600 atari5200 atari7800 atari800 atarijaguar atarilynx atarist atomiswave c128 c16 c64 channelf coco3 coleco cps1 cps2 cps3 daphne dos dreamcast easyrpg famicom fds gameandwatch gamegear gb gba gbc genesis gx4000 intellivision j2me love2d lowresnx mame mame2003 mastersystem megadrive megaduck msx msx2 mv n64 n64dd naomi nds neogeo neogeocd nes ngp ngpc odyssey2 onscripter openbor palm pc98 pcengine pcenginecd pcfx pico-8 ports psx psp saturn scummvm sega32x segacd sg-1000 snes snes-hacks snesmsu1 solarus sufami supergrafx supervision thomson ti99 tic80 turbografx turbografxcd tvc uzebox vectrex vic20 videopac virtualboy vmac vmu wasm4 wolf wonderswan wonderswancolor x1 x68000 zx81 zxspectrum"
for sys in $SYSTEMS; do
    if [ -d "$MNT_SRC/$sys" ]; then
        echo "  rsync $sys/"
        rsync -a "$MNT_SRC/$sys" "$SD_VOLUME/" 2>&1 | tail -1
    fi
done

# --- 6) Detach + eject
echo "[restore] detaching image"
sudo /sbin/umount "$MNT_SRC" 2>/dev/null || true
hdiutil detach "$DEV" 2>/dev/null || true

sync; sync
echo "[restore] free space on SD after copy:"
df -h "$SD_VOLUME" | tail -1
echo "[restore] done. Ejecting SD"
diskutil eject "$SD_VOLUME" 2>&1 | head -3 || true

echo
echo "================================================================"
echo "  SD restored. Insert it back into the R36S and power on."
echo "  Saves + ROMs + themes + tools recovered from $NAME"
echo "================================================================"
