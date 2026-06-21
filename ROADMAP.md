# Playora Roadmap

Aligned with the product spec (Phase 1 → Phase 7). Each phase ships as a
minor version. Issues track granular work.

## Status legend

- ✅ done
- 🚧 in progress
- 📋 planned (issue exists)
- 🧭 to be designed

---

## Phase 1 — Technical proof (v0.1) ✅

- ✅ playora-agent + playora-server compile & run
- ✅ SQLite outbox (offline-first)
- ✅ Heartbeat + hardware snapshot + sync batch + dedup
- ✅ test-session + ranking by playtime
- ✅ Dashboard (cards + tables + auto-refresh)
- ✅ LAN-reachable server
- ✅ Apple `container` cross-build aarch64

## Phase 2 — Console real (v0.2) 🚧

- ✅ Launcher wrapper (best-effort, never blocks game)
- ✅ ROM scanner (skip-list, quick hash, indexes)
- ✅ Save tracking (metadata only — no upload by default)
- 📋 Gamelist parser (read existing `gamelist.xml`, enrich game records)
- ✅ TUI on dArkOSRE (Ports menu entries, 3 screens)
- ✅ Doctor (quick checks)
- ✅ Resource guard (skip heavy ops during sessions)
- 📋 ext4 rootfs read for /etc/wpa_supplicant + /storage configs
- 📋 ScreenScraper integration for cover art

## Phase 3 — Community (v0.3)

- 📋 User accounts (server-side username/avatar/level/XP)
- 📋 Friend system (request/accept, friend ranking)
- 📋 Group system (public/private, group ranking, invite codes)
- 📋 5-invites-per-user model
- 📋 Weekly + monthly + per-system + per-game rankings
- 📋 Streak detection (daily play)
- 📋 Antifarm rules (min session length for ranking)
- 📋 Public profile / private profile toggle
- 📋 QR profile for adding friends

## Phase 4 — Club (v0.4)

- 📋 Subscription plans (monthly / quarterly / semi / annual / family)
- 📋 Club badges + monthly R-Coins
- 📋 Premium themes + premium challenges
- 📋 Beta access via Club flag
- 📋 Priority diagnostic (remote)
- 📋 Cloud save (Club only) — server endpoint + agent opt-in
- 📋 Discount engine respecting margin floors

## Phase 5 — Legal catalog (v0.5)

- ✅ Catalog routes + seeded items
- 📋 Catalog admin UI (server-side)
- 📋 SHA-256 verification on download (already partial)
- 📋 Cover art + screenshots
- 📋 Theme installer + config-pack installer (with backups)
- 📋 Metadata pack distribution

## Phase 6 — Play together (v0.6)

- 📋 Game room model (create / join via code / QR)
- 📋 Compatibility matrix (hash + core + emulator)
- 📋 Netplay launcher integration (RetroArch netplay where supported)
- 📋 Match history
- 📋 Anti-cheat rules for competitive ranking

## Phase 7 — Advanced (v1.0)

- 📋 Achievements per game (where ROM hash known)
- 📋 Seasons + leaderboards reset
- 📋 Curator system (approve content, earn R-Coins)
- 📋 Runtime probe (read RAM via RetroArch network) — opt-in, allowlist per game
- 📋 Marketplace for Club perks
- 📋 Mobile companion app

---

## Cross-cutting tracks

### Quality (every release)
- 📋 Test coverage to 90% — requires `Fs`/`Http`/`Time` trait abstractions + mocks
- ✅ Pre-commit hook (fmt + clippy + test)
- ✅ Pre-commit blocks bad commits
- 📋 `cargo deny` (license + dup deps audit)
- 📋 `cargo audit` (RustSec advisory)

### UX (always)
- ✅ Three-screen TUI (main / portmaster / update)
- 📋 Wizard on first boot (init, name console, pick server URL)
- 📋 Big-font theme for low-res 640x480 panel
- 📋 Controller-friendly key bindings (D-pad / A / B mapped)
- 📋 Empty-state messages instead of blank screens

### Distribution
- ✅ Self-update via GitHub release (stable + beta channel)
- 📋 Auto-update opt-in toggle
- 📋 Delta updates (only changed bytes)
- 📋 Signed binaries (minisign)

### Privacy
- ✅ SSID never collected
- ✅ MAC hashed only
- ✅ Runtime probe disabled by default
- 📋 Privacy policy doc + consent flow on first sync

---

## Versioning

Semantic-ish:
- `0.x.y` — pre-1.0, breaking changes allowed in minor
- `1.0` lands when Phase 7 ships
- Patch versions for bug fixes within a minor
- Pre-releases tagged `vX.Y.Z-beta.N`
