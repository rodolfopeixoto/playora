//! Game-session detection by polling the process list.
//!
//! Watches for a running `retroarch` process, extracts the ROM path from
//! its command line, and emits GameSessionStarted / GameSessionFinished
//! events when the active ROM changes. Runs inside the autosync loop.

use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::path::Path;
use std::time::Duration;

#[derive(Default)]
pub struct SessionTracker {
    current: Option<Current>,
}

struct Current {
    session_id: SessionId,
    started_at: chrono::DateTime<Utc>,
    rom_path: String,
    system: GameSystem,
    game_name: String,
    core: Option<String>,
}

impl SessionTracker {
    pub fn tick(&mut self, cfg: &AgentConfig) {
        let detected = detect_running_rom();
        match (&self.current, detected) {
            (None, Some(d)) => {
                let session_id = SessionId::new();
                let system = GameSystem::from_folder(&d.system_folder);
                let game_name = Path::new(&d.rom_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("(unknown)")
                    .to_string();
                let ev = Event {
                    event_id: EventId::new(),
                    device_id: cfg.device_id.clone(),
                    created_at: Utc::now(),
                    payload: EventPayload::GameSessionStarted(GameSessionStarted {
                        session_id: session_id.clone(),
                        system,
                        game_name: game_name.clone(),
                        rom_path: d.rom_path.clone(),
                        rom_hash: None,
                        core: d.core.clone(),
                        started_at: Utc::now(),
                    }),
                };
                if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
                    let _ = crate::db::enqueue(&conn, &ev);
                }
                self.current = Some(Current {
                    session_id,
                    started_at: Utc::now(),
                    rom_path: d.rom_path,
                    system,
                    game_name,
                    core: d.core,
                });
                let _ = crate::sync::cmd_sync_once(cfg.clone());
            }
            (Some(cur), Some(d)) if cur.rom_path != d.rom_path => {
                // Transitioned to a different game — finish old, start new.
                emit_finish(cfg, cur);
                self.current = None;
                self.tick(cfg);
            }
            (Some(cur), None) => {
                emit_finish(cfg, cur);
                self.current = None;
                let _ = crate::sync::cmd_sync_once(cfg.clone());
            }
            _ => {}
        }
    }
}

fn emit_finish(cfg: &AgentConfig, cur: &Current) {
    let now = Utc::now();
    let duration = (now - cur.started_at).num_seconds().max(0);
    let ev = Event {
        event_id: EventId::new(),
        device_id: cfg.device_id.clone(),
        created_at: now,
        payload: EventPayload::GameSessionFinished(GameSessionFinished {
            session_id: cur.session_id.clone(),
            ended_at: now,
            duration_seconds: duration as u64,
            exit_code: None,
            save_changed: false,
            max_cpu_percent: None,
            max_memory_mb: None,
        }),
    };
    let _ = (cur.system, &cur.game_name, &cur.rom_path, &cur.core);
    if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
        let _ = crate::db::enqueue(&conn, &ev);
    }
}

#[derive(Debug)]
struct Detected {
    rom_path: String,
    system_folder: String,
    core: Option<String>,
}

/// Scan running emulator processes (retroarch, mupen64plus, ppsspp, mgba,
/// drastic, dosbox, scummvm, …) and extract the loaded ROM from /proc.
fn detect_running_rom() -> Option<Detected> {
    // Read /proc directly — works on every Linux even without busybox pgrep -a.
    let procs = std::fs::read_dir("/proc").ok()?;
    for entry in procs.flatten() {
        let pid = match entry.file_name().to_string_lossy().parse::<u32>() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let cmdline_raw = match std::fs::read(format!("/proc/{pid}/cmdline")) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if cmdline_raw.is_empty() {
            continue;
        }
        // /proc/<pid>/cmdline is NUL-separated.
        let argv: Vec<String> = cmdline_raw
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        if argv.is_empty() {
            continue;
        }
        let exe = Path::new(&argv[0])
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !is_emulator(&exe) {
            continue;
        }
        let joined = argv.join(" ");
        if let Some(rom) = parse_rom_from_cmdline(&joined) {
            let system_folder = Path::new(&rom)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            return Some(Detected {
                rom_path: rom,
                system_folder,
                core: parse_core_from_cmdline(&joined),
            });
        }
    }
    None
}

const EMU_NEEDLES: &[&str] = &[
    "retroarch",
    "mupen64plus",
    "ppsspp",
    "mgba",
    "drastic",
    "dosbox",
    "scummvm",
    "dolphin-emu",
    "yuzu",
    "citra",
    "pcsx2",
    "pcsx_rearmed",
    "snes9x",
    "fceux",
    "vbam",
    "gpsp",
    "stella",
    "duckstation",
    "redream",
    "flycast",
];

fn is_emulator(name: &str) -> bool {
    EMU_NEEDLES.iter().any(|n| name.contains(n))
}

const ROM_EXTS: &[&str] = &[
    ".nes", ".sfc", ".smc", ".gba", ".gb", ".gbc", ".n64", ".z64", ".v64", ".md", ".gen", ".smd",
    ".bin", ".iso", ".chd", ".cso", ".cue", ".gdi", ".pbp", ".elf", ".nds", ".3ds", ".cdi", ".m3u",
    ".zip", ".7z",
];

fn parse_rom_from_cmdline(cmd: &str) -> Option<String> {
    // Pass 1: token that contains "/roms/" (most reliable).
    for token in cmd.split_whitespace().rev() {
        let unq = token.trim_matches('"').trim_matches('\'');
        if unq.contains("/roms/") {
            return Some(unq.to_string());
        }
    }
    // Pass 2: any token whose lowercase ends in a known ROM extension.
    for token in cmd.split_whitespace().rev() {
        let unq = token.trim_matches('"').trim_matches('\'');
        let lower = unq.to_lowercase();
        if ROM_EXTS.iter().any(|e| lower.ends_with(e)) {
            return Some(unq.to_string());
        }
    }
    None
}

fn parse_core_from_cmdline(cmd: &str) -> Option<String> {
    let mut found_l = false;
    for tok in cmd.split_whitespace() {
        if found_l {
            return Some(
                Path::new(tok)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(tok)
                    .to_string(),
            );
        }
        if tok == "-L" || tok == "--libretro" {
            found_l = true;
        }
    }
    None
}

#[allow(dead_code)]
pub fn poll_loop(cfg: AgentConfig, interval_s: u64) -> Result<()> {
    let mut tracker = SessionTracker::default();
    loop {
        tracker.tick(&cfg);
        std::thread::sleep(Duration::from_secs(interval_s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rom_path_via_roms_prefix() {
        let cmd = "retroarch -L /usr/lib/libretro/snes9x.so /roms/snes/Castlevania.smc";
        assert_eq!(
            parse_rom_from_cmdline(cmd),
            Some("/roms/snes/Castlevania.smc".into())
        );
    }

    #[test]
    fn parses_rom_path_via_extension_fallback() {
        let cmd = "mupen64plus --corelib /opt/cores/mupen.so /mnt/data/games/Mario.z64";
        assert_eq!(
            parse_rom_from_cmdline(cmd),
            Some("/mnt/data/games/Mario.z64".into())
        );
    }

    #[test]
    fn emulator_name_match() {
        assert!(is_emulator("retroarch"));
        assert!(is_emulator("retroarch-aarch64"));
        assert!(is_emulator("ppsspp-sdl"));
        assert!(!is_emulator("bash"));
    }

    #[test]
    fn parses_core_name() {
        let c = "retroarch -L /usr/lib/libretro/gba.so /roms/gba/game.gba";
        assert_eq!(parse_core_from_cmdline(c), Some("gba".to_string()));
    }
}
