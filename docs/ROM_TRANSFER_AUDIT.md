# ROM Transfer Audit

ROMs that arrived from a previous (possibly fake) ArkOS backup via TAR
through a Mac will carry several known classes of breakage. This document
lists what to look for and how Playora helps detect / fix each.

## Known breakage classes

| Class | How to detect | Auto-fix |
|---|---|---|
| `.DS_Store`, `._*`, `__MACOSX/` (macOS metadata) | `doctor --deep` `macos_junk` count | `clean-roms --apply` (sprint 2) |
| Wrong system folder name (`pcengine` vs `tg16`, `genesis` vs `megadrive`) | Manual + scanner output | `repair-rom-layout --apply` (sprint 2) |
| Broken CUE (CUE points to missing BIN) | `doctor --deep` `cue_integrity` | Manual |
| Broken M3U | `doctor --deep` `m3u_integrity` | Manual |
| Missing BIOS folder | `doctor --deep` `bios_present` | Manual (BIOS is user-supplied) |
| Zero-byte / truncated ROMs | `audit-roms` (sprint 2) | Manual delete |
| Duplicates by content | `audit-roms` (sprint 2) | Manual delete |
| Invalid `gamelist.xml` | `doctor --deep` `gamelists` | ES rescan |
| CRLF in scripts | doctor (sprint 2) | `clean-roms --apply` (sprint 2) |
| Permissions wrong on `.sh` | doctor (sprint 2) | `clean-roms --apply` (sprint 2) |
| Hidden ROMs (path with special chars) | scanner skip count | Manual |
| Saves/savestates in wrong folder | session tracker (sprint 2) | Manual move |

## Today's signal (sprint 1)

`playora-agent doctor --deep` already flags:

- `macos_junk` — count of macOS metadata files under `/roms`
- `cue_integrity` — list of CUEs pointing to missing BIN files
- `m3u_integrity` — list of M3Us pointing to missing entries
- `gamelists` — count of invalid `gamelist.xml`
- `bios_present` — whether `/roms/bios` exists and has any files

Each is a separate `CheckResult` in the JSON report and a
`SystemIssueDetected` event when severity is `warn` or `fail`.

## Sprint 2 — shipped commands

- `playora-agent audit-roms` — full inventory, duplicates by name+size,
  zero-byte detection, unknown-extension report, BIOS requirements by
  system. Emits `RomAuditResult`.
- `playora-agent clean-roms` (dry-run) / `--apply` — removes
  certified-safe junk only (`.DS_Store`, `._*`, `__MACOSX/`, `thumbs.db`)
  and optionally fixes CRLF + `+x` on `ports/*.sh`.
- `playora-agent repair-rom-layout` (dry-run) / `--apply` — moves ROMs
  from `_inbox` or wrong system folders to the correct one by extension.
  Never overwrites; renames `.dup-N` on collision.
- `playora-agent scan` already incremental (skip-by `path + size`).

All planned commands will support `--dry-run` (default) and require
explicit `--apply` to write.

## Manual triage checklist

1. `playora-agent doctor --deep` — read the report JSON.
2. If `macos_junk > 0`: run cleanup (planned `clean-roms`).
3. If `cue_integrity` lists broken cues: open each CUE, fix the filename
   reference or re-copy the matching BIN.
4. If `m3u_integrity` lists broken m3us: fix path entries (relative,
   no Windows backslashes).
5. If `gamelists` lists invalid xml: delete the file and let ES rescan,
   or run `playora-agent scan` after fixing.
6. If `bios_present` is missing: create `/roms/bios` and drop the
   required BIOS files there.
