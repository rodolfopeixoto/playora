# Playora

Offline-first telemetry, hardware data, sessions, ranking, feature flags and
legal catalog for the **R36S clone** running **dArkOSRE-R36**.

3 crates:
- **playora-common** — wire types
- **playora-agent** — runs on the device
- **playora-server** — Axum + SQLite + dashboard (`/dashboard`)

## Quick local run

```sh
cargo run -p playora-server -- --db ./server.db --bind 0.0.0.0:8080 &
cargo run -p playora-agent  -- init --server-url http://127.0.0.1:8080
cargo run -p playora-agent  -- doctor
cargo run -p playora-agent  -- heartbeat
cargo run -p playora-agent  -- test-session --system snes --game "Fake SNES Test" --duration 5
cargo run -p playora-agent  -- sync
open http://127.0.0.1:8080/dashboard
```

## On the R36S
See [docs/INSTALL_DARKOSRE.md](docs/INSTALL_DARKOSRE.md) — copies a single binary
into `/usr/local/bin/playora-agent` and creates Ports menu entries.

## Docs
- `docs/TESTING_LOCAL.md`
- `docs/TESTING_R36S.md`
- `docs/INSTALL_DARKOSRE.md`
- `docs/HARDWARE_DATA.md`
- `docs/FEATURE_FLAGS.md`
- `docs/CATALOG_RULES.md`
