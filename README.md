# Playora

> Console-side agent + central hub for the R36S (and clone variants) running
> [dArkOSRE-R36](https://github.com/southoz/dArkOSRE-R36). Records real play
> sessions, hardware data, saves, and feeds a ranking + community layer.
>
> Offline-first SQLite, minimal RAM/CPU footprint, no SSH required on the
> device — everything is reachable from EmulationStation's **Ports** menu.

[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Why

The R36S is an awesome little RK3326 handheld but the ecosystem is fragmented:
no central place for your play time, no ranking, no community, no easy way to
get themes/PortMaster games/saves backed up.

Playora fills that gap with a single static binary (`playora-agent`, ~7.5 MB,
aarch64-linux-gnu) that:

- runs once a minute on the device
- captures hardware, sessions, saves
- syncs to a tiny Axum + SQLite server you can self-host on your laptop /
  Raspberry Pi / VPS
- exposes a TUI menu (PortMaster install, self-update, doctor, ...) right in
  EmulationStation

## Repo layout

```
crates/
├── playora-common/        # wire types, no I/O
├── playora-agent/         # device-side binary (CLI + TUI)
└── playora-server/        # axum + sqlite + dashboard
scripts/
├── build-container.sh         # cross-build aarch64 via Apple `container`
├── restore-sd-from-drive.sh   # one-shot SD restore from Google Drive backup
├── install-to-sd.sh           # drop binary + ports menu directly on mounted SD
├── install-darkosre-menu.sh   # run on the device once SSH is enabled
└── run-local-server.sh
docs/
├── LAN_TESTING.md
├── INSTALL_DARKOSRE.md
├── TESTING_LOCAL.md
├── TESTING_R36S.md
├── HARDWARE_DATA.md
├── FEATURE_FLAGS.md
└── CATALOG_RULES.md
```

## Quick start

### Local dev

```sh
cargo run -p playora-server -- --db ./server.db --bind 0.0.0.0:8080 &
cargo run -p playora-agent  -- --config /tmp/p.toml init --server-url http://127.0.0.1:8080
cargo run -p playora-agent  -- --config /tmp/p.toml test-session --system snes --game "Demo" --duration 5
open http://127.0.0.1:8080/dashboard
```

### Cross-build for R36S

Apple `container` CLI is preferred; falls back to docker.

```sh
sh scripts/build-container.sh   # → dist/playora-agent-aarch64 (~7.5 MB)
```

### Install on the R36S SD (no SSH)

```sh
# 1) (one-time) restore your old ROMs/saves from a Drive backup into the SD
sh scripts/restore-sd-from-drive.sh

# 2) drop the binary + Ports menu entries straight onto the mounted SD
sh scripts/install-to-sd.sh
```

Eject the SD, boot the R36S, open EmulationStation → **Ports**:

| Entry                 | What it does                                                  |
|-----------------------|---------------------------------------------------------------|
| `Playora Hub`         | TUI menu (status, hardware, PortMaster install, update, ...)  |
| `Playora PortMaster`  | same TUI, jumps to PortMaster screen                          |
| `Playora Update`      | self-update from GitHub release (stable / beta channel)       |
| `Playora Saves Backup`| pack saves + upload to your server                            |
| `Playora Doctor`      | diagnostic                                                    |
| `Playora Hardware`    | hardware snapshot JSON                                        |

## Subcommands

```
playora-agent init [--server-url URL] [--device-name NAME]
playora-agent run                                  # heartbeat + sync loop
playora-agent doctor [--interactive]
playora-agent tui [--screen SCREEN]                # 3 screens: main, portmaster, update
playora-agent status
playora-agent hardware {snapshot[--save], test[--mode quick|full], watch}
playora-agent resources {sample, watch}
playora-agent scan
playora-agent heartbeat
playora-agent sync
playora-agent test-session --system SYS --game NAME [--duration N]
playora-agent launcher --system SYS --core CORE --rom PATH -- CMD...
playora-agent saves {pack[--dest], upload}
playora-agent download --url URL --system SYS [--name NAME] [--sha256 HEX]
playora-agent sources
playora-agent systems
playora-agent coolrom {consoles, roms CONSOLE LETTER, download URL_PATH --dest DIR}
playora-agent myrient {index URL, search URL QUERY}
playora-agent portmaster {list[--ready-to-run-only], search QUERY, install NAME, installed}
playora-agent features {fetch, show}
playora-agent self-update [--owner OWNER] [--repo REPO]
playora-agent logs tail [--lines N]
```

## Development

Pre-commit hook enforces:
```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet
```

Enable once:
```sh
git config core.hooksPath .githooks
```

### Tests + coverage

```sh
cargo test                                  # 38 tests
~/.cargo/bin/cargo-tarpaulin tarpaulin --workspace --out Html --output-dir target/coverage
open target/coverage/tarpaulin-report.html
```

Pure-logic crates are at 95-100% (`common`, `sources`, `systems`).
I/O-heavy modules (HTTP, sysfs, sqlite open) are tracked in
[issue #1: coverage refactor](#) and will reach 90% as traits + mock layers land.

## Release flow (gitflow-lite)

- `main` — always shippable; tagged releases (`v0.1.x`)
- `develop` — integration branch
- `feature/<topic>` → PR → `develop`
- when `develop` is green and feature set is locked: PR → `main`, tag
- pre-releases (beta): tag `v0.2.0-beta.1` with `--prerelease` on `gh release`

```sh
sh scripts/build-container.sh
gh release create v0.1.3 dist/playora-agent-aarch64 --notes "..."        # stable
gh release create v0.2.0-beta.1 dist/playora-agent-aarch64 --prerelease  # beta
```

Users on the device pick stable or beta in the **Update Playora** TUI screen.

## Roadmap

See [ROADMAP.md](ROADMAP.md) and the
[milestones tab](https://github.com/rodolfopeixoto/playora/milestones) for the
full feature plan (Phases 1–7).

## License

MIT — see [LICENSE](LICENSE).

## Credits

- [southoz/dArkOSRE-R36](https://github.com/southoz/dArkOSRE-R36) — base firmware
- [PortsMaster/PortMaster-New](https://github.com/PortsMaster/PortMaster-New) — port catalog
- [handhelds.wiki](https://handhelds.wiki/R36S_Clones) — clone identification
- `coolrom.rs` is a Rust port by Rodolfo Peixoto inspired by Victor Oliveira's
  Python prototype (WTFPL, 2018)
