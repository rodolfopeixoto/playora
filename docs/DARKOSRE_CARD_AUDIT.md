# dArkOSRE Card Audit

How to audit a Playora-installed dArkOSRE SD card end-to-end before
trusting it for production use.

## Goals

- Confirm the card boots cleanly without freeze.
- Confirm `/roms` is writable, not running on a dying SD.
- Confirm ROMs survived TAR transfer from a previous (possibly fake
  ArkOS) backup.
- Confirm exit-game, recover, and port flow all return the user to
  EmulationStation, never to a black screen.
- Confirm Playora events reach the dashboard when online.

## Sequence

### 1. Boot check (manual)

- Console reaches EmulationStation main screen.
- No serial console errors during boot (if SSH is available, watch
  `dmesg`).

### 2. Run `playora-agent doctor --deep`

From Ports → "Playora Doctor Deep".

Expected: score `Ok` or `Warn` (never `Fail`). Any `Fail` indicates one
of:

- `/roms` read-only
- TTY missing (no `/dev/tty1` or `tty0`)
- `dmesg` shows mmc/ext4/I/O errors → back up immediately, SD likely
  failing
- RetroArch binary absent
- retroarch.cfg files not found

Each finding lands as a `SystemIssueDetected` event in the dashboard
device feed with severity, evidence, and suggested fix.

### 3. Black-screen test

- Launch any RetroArch core game.
- Press the exit combo. Expect ES to return cleanly.
- If it freezes, run `playora-agent recover` from SSH (or invoke the
  "Playora Recover" port via a controller hotkey if reachable). Recover
  emits `BlackScreenRecovered`.
- Apply `playora-agent fix-exit-game --apply`, reboot, retest.

### 4. ROM transfer audit (planned: `audit-roms`)

Until the dedicated `audit-roms` subcommand ships, doctor already flags:

- macOS junk files (`.DS_Store`, `._*`, `__MACOSX/`)
- Invalid `gamelist.xml`
- Broken CUE references (CUE points to a missing .bin)
- Broken M3U references
- Missing `/roms/bios` directory

Re-running doctor after `clean-roms --apply` (planned) should zero out
the macOS junk count.

### 5. Round-trip sync

- Trigger one sync: Ports → "Playora Quick Sync".
- Confirm device dashboard shows recent `DoctorReport`, `HardwareSnapshot`,
  and any `SystemIssueDetected` events.

## Acceptance criteria

- Doctor score is `Ok` or all `Warn` items have a known explanation.
- No `Fail` items.
- Exit-game does not freeze.
- Recover restores ES if a freeze is forced.
- `/roms` free space ≥ 1 GiB.
- Dashboard receives events within 60s of running ports.
