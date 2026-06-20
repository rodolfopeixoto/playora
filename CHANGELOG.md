# Changelog

## 0.1.0 — 2026-06-20 (MVP)

Initial preview. Functional end-to-end on macOS host; targets R36S clone with dArkOSRE-R36.

### Added
- `playora-common`: wire types, `GameSystem`, `DeviceProfile`, `Event`/`SyncBatch`, systems registry, ROM sources catalog.
- `playora-agent`: 18 subcommands — `init`, `run`, `doctor`, `status`, `tui`, `hardware {snapshot,test,watch}`, `resources {sample,watch}`, `scan`, `sync`, `heartbeat`, `test-session`, `launcher`, `catalog {list,search,download}`, `features {fetch,show}`, `logs tail`, `download`, `sources`, `systems`.
- `playora-server`: Axum + SQLite, dashboard at `/dashboard`, 20+ REST routes.
- Offline-first SQLite outbox with idempotent enqueue.
- ROM-source catalog (Myrient, Internet Archive, Vimm's Lair, user URL).
- Per-system metadata (emulator + RetroArch core + extensions) for 20 systems.
- Apple `container` build script (`scripts/build-container.sh`) with docker fallback.
- 17 tests (12 unit + 5 integration including full E2E client→server cycle).

### Defaults
- `runtime_probe` = disabled (privacy by default).
- `netplay` = planned (was: locked).
- `rom_download` = enabled.

### Not yet
- ROM source crawling/index (manual URL only).
- RetroArch Network Control probe (stub).
- TUI screens 4–8 (only main menu).
- OTA firmware install on-device (use macOS scripts in `~/R36S_DARKOS_TEST/`).
