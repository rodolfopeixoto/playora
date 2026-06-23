# Netplay Limits & Compatibility Matrix

Playora netplay is **experimental, opt-in, and locked behind a feature
flag**. It is not a universal "online multiplayer" toggle — RetroArch
netplay (and the standalone emulator equivalents) work well only when
*all of these are true*:

1. The core supports deterministic rollback (most do not).
2. Every peer runs the exact same core version.
3. Every peer has the exact same ROM content (matching SHA-256 / CRC32).
4. Latency between peers is low (< 80 ms RTT for fighting games).
5. No save-state diff exists between peers.

When any of these fail, netplay either desyncs after seconds or refuses
to start. Playora's job is to **predict failure before it happens** and
show a clear "not supported" message — never silently start a session
that will desync.

## Compatibility matrix (Playora's opinion)

| System | RA Core | Netplay viability on R36S |
|---|---|---|
| NES | nestopia / fceumm | Good — deterministic, low CPU |
| SNES | snes9x / snes9x_2010 | Good |
| Mega Drive / Genesis | genesis_plus_gx | Good |
| Game Boy / GBC | gambatte | Good |
| GBA | mgba | Workable; some games desync on RTC |
| TG16 / PC Engine | mednafen_pce / mednafen_pce_fast | Workable |
| Master System / Game Gear | genesis_plus_gx | Good |
| Arcade (small FBNeo set) | fbneo | Good for SF / KOF subset |
| PS1 | pcsx_rearmed / mednafen_psx_hw | Fragile — disc swaps + BIOS need to match |
| N64 | mupen64plus_next | **Not recommended** — non-deterministic on RK3326 |
| PSP | ppsspp (standalone) | **Not supported** — PPSSPP netplay is standalone-only |
| NDS | desmume / melonds | **Not supported on R36S** — too heavy |
| Dreamcast | flycast | **Not supported** — non-deterministic on this hardware |
| Saturn | mednafen_saturn / yabasanshiro | **Not supported** — desyncs |

`Good` = will likely work end-to-end with matching ROMs.
`Workable` = works for most games; one or two known-bad titles.
`Not recommended` / `Not supported` = Playora refuses to start netplay in
this combination and surfaces a clear reason on the dashboard.

## What Playora exposes today (sprint 4)

- `NetplayRoomCreated` event with `room_id`, `host_code`, `system`,
  `core`, `content_hash`.
- `NetplayRoomJoined` event with `latency_ms` when measurable.
- Feature flag `netplay` defaults to `Locked`. Dashboard must flip it
  explicitly per-device.
- Server route stubs only — no live matchmaking yet.

## Not in scope yet

- Matchmaking server (rooms are host-code only).
- NAT traversal / hole-punching.
- Spectator mode.
- Voice / chat.
- Rollback rating (game-by-game grade — needs telemetry over time).

## How to disable

Feature flags ship as `Locked` for netplay. To enable per device:

```
curl -X PUT http://<server>/api/v1/devices/<id>/features \
  -H 'content-type: application/json' \
  -d '[{"key":"netplay","status":"enabled","reason":"trusted"}]'
```

Re-lock with `"status":"locked"`.
