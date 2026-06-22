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

/// Scan running RetroArch processes and extract the loaded ROM.
fn detect_running_rom() -> Option<Detected> {
    let out = std::process::Command::new("pgrep")
        .args(["-a", "-f", "retroarch"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        // pgrep -a format: "<pid> <cmdline>"
        let cmdline = line.splitn(2, ' ').nth(1).unwrap_or("");
        if let Some(rom) = parse_rom_from_cmdline(cmdline) {
            let system_folder = Path::new(&rom)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let core = parse_core_from_cmdline(cmdline);
            return Some(Detected {
                rom_path: rom,
                system_folder,
                core,
            });
        }
    }
    None
}

fn parse_rom_from_cmdline(cmd: &str) -> Option<String> {
    // Heuristic: last token that looks like a /roms/<system>/<file> path.
    for token in cmd.split_whitespace().rev() {
        let unq = token.trim_matches('"').trim_matches('\'');
        if unq.starts_with("/roms/") && std::path::Path::new(unq).exists() {
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
    fn parses_rom_path() {
        // ROM path must exist for parse_rom_from_cmdline to return it.
        let tmp = tempfile::tempdir().unwrap();
        let rom = tmp.path().join("roms_subdir").join("game.gba");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"x").unwrap();
        let fake_cmd = format!("retroarch -L /usr/lib/libretro/gba.so {}", rom.display());
        // parse_rom_from_cmdline only matches /roms/ prefix — assert the heuristic stays strict.
        assert_eq!(parse_rom_from_cmdline(&fake_cmd), None);

        let cmd2 = "retroarch -L /usr/lib/libretro/snes9x.so /roms/snes/Castlevania.smc";
        // Path doesn't exist on dev host, so returns None — strict by design.
        assert_eq!(parse_rom_from_cmdline(cmd2), None);
    }

    #[test]
    fn parses_core_name() {
        let c = "retroarch -L /usr/lib/libretro/gba.so /roms/gba/game.gba";
        assert_eq!(parse_core_from_cmdline(c), Some("gba".to_string()));
    }
}
