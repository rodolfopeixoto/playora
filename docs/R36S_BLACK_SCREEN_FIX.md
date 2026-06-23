# R36S Black-Screen Fix

How Playora diagnoses and patches the Select+Start exit-game freeze on
R36S / dArkOSRE clones, and what *not* to assume.

## Symptom

User presses Select+Start to exit a game in RetroArch. RetroArch exits but
EmulationStation never redraws — the framebuffer stays black until power
cycle.

## Root causes (ranked, all observed in the wild)

1. **`video_threaded = true`** — threaded video deadlocks on quit on
   RK3326 (Mali T-820). Most common.
2. **`pause_nonactive = true`** — ES focus toggle pauses the core; freeze
   happens inside the pause path.
3. **`audio_driver = pulse`** — pulse leaves the device locked after exit.
   alsathread releases cleanly.
4. **gptokeyb still alive after RetroArch dies** — captures input so ES
   sees no events even after it draws.
5. **Hotkey combo mismatch** — user expects Select+Start but
   `input_quit_gamepad_combo` is unset/different in the dArkOSRE shipped
   cfg.
6. **Override masking** — `~/.config/retroarch/config/<core>/<game>.cfg`
   silently overrides the global setting.
7. **RetroArch32 cfg ignored** — fix applied only to `retroarch.cfg`, not
   `retroarch32/retroarch.cfg` that handles SNES/NES/etc.
8. **Standalone emulators** (PPSSPP, Drastic, mupen64plus standalone) do
   *not* honor RetroArch quit combos — different exit paths, NOT covered
   by this fix.
9. **SD remounted read-only** — patches silently fail; doctor flags this
   under `roms_writable`.
10. **DRM master not released** (`video_driver=glcore` on some KMS
    combos). `gl` is safer on RK3326.

## Tools

### `playora-agent doctor --deep`

Reports the state of each of the above. Output:

- TTY summary on screen (R36S framebuffer)
- JSON report at `/roms/.playora/reports/doctor-YYYYMMDD-HHMMSS.json`
- Log tail at `/roms/.playora/logs/doctor-latest.log`
- Event `DoctorReport` + `SystemIssueDetected` per warning/failure

### `playora-agent fix-exit-game` (dry-run by default)

- `playora-agent fix-exit-game` — audits, shows diff, writes nothing.
- `playora-agent fix-exit-game --apply` — writes changes, with a
  timestamped backup (`retroarch.cfg.playora-bak.YYYYMMDD-HHMMSS`)
  per cfg.
- `playora-agent fix-exit-game --restore` — rolls back to the most recent
  backup.

Audits both `retroarch.cfg` and `retroarch32.cfg` when present. Lists any
per-core/per-game override that may mask the global fix. Each setting
carries a justification line written to
`/roms/.playora/logs/fix-exit-*.log`.

### `playora-agent recover`

When a freeze does happen and the user still has SSH (or runs the
"Playora Recover" port through the port-runner):

- Kills `playora-agent`, `gptokeyb`, `gptokeyb2`
- Sweeps any `/tmp/playora-*.lock` files
- Verifies `/dev/tty1`/`tty0` exist
- Restarts EmulationStation (systemd unit if detected,
  `exec emulationstation` fallback otherwise)
- Emits `BlackScreenRecovered` + `EmulationStationRestarted` events

## What this fix does NOT promise

- Standalone emulators (PSP/N64/NDS/Dreamcast standalone, Ports) follow
  their own exit paths. `quit_press_twice` and combo do not affect them.
- True hardware failure (bad SD, dying flash, broken display ribbon) is
  not patched by configuration.
- DTB/panel mismatches that cause boot-time issues are out of scope —
  the doctor reports panel info but does not change DTB.

## Manual verification

```bash
# Local dry run, write nothing:
playora-agent fix-exit-game

# Apply (backs up first):
playora-agent fix-exit-game --apply

# Rollback if needed:
playora-agent fix-exit-game --restore

# Full diagnose:
playora-agent doctor --deep
```

On the R36S itself: launch from EmulationStation → Ports → "Playora
Check Exit-Game" (dry run), then "Playora Fix Exit-Game" to apply.
