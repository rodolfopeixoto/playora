//! Game-session detection by polling the process list.
//!
//! Watches every `/proc/<pid>/cmdline` for a ROM-like token. Catches
//! RetroArch, RetroArch32, PPSSPP, Drastic, mupen64plus standalone, ES
//! launcher shells, and runcommand wrappers indiscriminately. Emits
//! `GameSessionStarted`/`Finished`, plus `GameSessionCrashed` when a
//! session ended unusually fast, `GameSessionOrphaned` when the agent
//! restarts mid-session (reconciled from `current-session.json`), and
//! `SaveChanged` when a save sidecar hash differs at end vs start.

use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const PERSIST_PATH: &str = "/roms/.playora/current-session.json";
/// A "real" session is at least this many seconds. Shorter than this and we
/// treat the exit as a crash (loader bailed, asset missing, ROM rejected).
const CRASH_THRESHOLD_SECS: u64 = 8;
/// Save-sidecar extensions worth hashing.
const SAVE_EXTS: &[&str] = &["srm", "sav", "state", "rtc", "mcr", "fla", "eep"];

#[derive(Default)]
pub struct SessionTracker {
    current: Option<Current>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Current {
    session_id: SessionId,
    started_at: chrono::DateTime<Utc>,
    rom_path: String,
    system_folder: String,
    system: GameSystem,
    game_name: String,
    core: Option<String>,
    pid: Option<u32>,
    save_hash_on_start: Option<String>,
    save_path: Option<String>,
    #[serde(default)]
    max_cpu_percent: Option<f32>,
    #[serde(default)]
    max_memory_mb: Option<u64>,
}

impl SessionTracker {
    pub fn new(cfg: &AgentConfig) -> Self {
        // Reconcile any persisted session from before the agent restarted.
        let mut s = Self::default();
        if let Some(prev) = load_persisted() {
            if !pid_still_owns(prev.pid, &prev.rom_path) {
                emit_orphaned(cfg, &prev);
                emit_finish(cfg, &prev, FinishKind::Orphan);
                let _ = std::fs::remove_file(PERSIST_PATH);
            } else {
                s.current = Some(prev);
            }
        }
        s
    }

    pub fn tick(&mut self, cfg: &AgentConfig) {
        let detected = detect_running_rom();
        match (&self.current, detected) {
            (None, Some(d)) => self.start(cfg, d),
            (Some(cur), Some(d)) if cur.rom_path != d.rom_path => {
                let kind = classify_finish(cur);
                emit_finish(cfg, cur, kind);
                let _ = std::fs::remove_file(PERSIST_PATH);
                self.current = None;
                self.start(cfg, d);
            }
            (Some(_), Some(_)) => {
                // Same session still running — sample CPU/mem peak.
                if let Some(cur) = self.current.as_mut() {
                    sample_into(cur);
                    persist(cur);
                }
            }
            (Some(cur), None) => {
                let kind = classify_finish(cur);
                emit_finish(cfg, cur, kind);
                let _ = std::fs::remove_file(PERSIST_PATH);
                self.current = None;
                let _ = crate::sync::cmd_sync_once(cfg.clone());
            }
            _ => {}
        }
    }

    fn start(&mut self, cfg: &AgentConfig, d: Detected) {
        let session_id = SessionId::new();
        let started_at = Utc::now();
        let system = GameSystem::from_folder(&d.system_folder);
        let game_name = Path::new(&d.rom_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)")
            .to_string();
        let (save_path, save_hash_on_start) = find_save(&d.rom_path);
        let cur = Current {
            session_id: session_id.clone(),
            started_at,
            rom_path: d.rom_path.clone(),
            system_folder: d.system_folder.clone(),
            system,
            game_name: game_name.clone(),
            core: d.core.clone(),
            pid: d.pid,
            save_hash_on_start,
            save_path: save_path.as_ref().map(|p| p.display().to_string()),
            max_cpu_percent: None,
            max_memory_mb: None,
        };
        let ev = Event {
            event_id: EventId::new(),
            device_id: cfg.device_id.clone(),
            created_at: started_at,
            payload: EventPayload::GameSessionStarted(GameSessionStarted {
                session_id: session_id.clone(),
                system,
                game_name,
                rom_path: d.rom_path,
                rom_hash: None,
                core: d.core,
                started_at,
            }),
        };
        if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
            let _ = crate::db::enqueue(&conn, &ev);
        }
        persist(&cur);
        self.current = Some(cur);
        let _ = crate::sync::cmd_sync_once(cfg.clone());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinishKind {
    Normal,
    Crash,
    Orphan,
}

/// Sample the emulator pid's CPU and memory via sysinfo; keep the peak.
fn sample_into(cur: &mut Current) {
    let Some(pid_u32) = cur.pid else { return };
    use sysinfo::{Pid, System};
    let mut sys = System::new();
    sys.refresh_all();
    let pid = Pid::from(pid_u32 as usize);
    if let Some(proc_) = sys.process(pid) {
        let cpu = proc_.cpu_usage();
        let mem_mb = proc_.memory() / 1024 / 1024;
        cur.max_cpu_percent = Some(cur.max_cpu_percent.map_or(cpu, |p| p.max(cpu)));
        cur.max_memory_mb = Some(cur.max_memory_mb.map_or(mem_mb, |p| p.max(mem_mb)));
    }
}

fn classify_finish(cur: &Current) -> FinishKind {
    let now = Utc::now();
    let duration = (now - cur.started_at).num_seconds().max(0) as u64;
    if duration < CRASH_THRESHOLD_SECS {
        FinishKind::Crash
    } else {
        FinishKind::Normal
    }
}

fn emit_finish(cfg: &AgentConfig, cur: &Current, kind: FinishKind) {
    let now = Utc::now();
    let duration = (now - cur.started_at).num_seconds().max(0) as u64;

    // Save-changed detection
    let (current_save_hash, save_changed) = match cur.save_path.as_ref() {
        Some(p) => {
            let h = hash_file(Path::new(p));
            let changed = match (&cur.save_hash_on_start, &h) {
                (Some(a), Some(b)) => a != b,
                (None, Some(_)) => true, // save appeared during session
                _ => false,
            };
            (h, changed)
        }
        None => (None, false),
    };

    if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
        let fin = Event {
            event_id: EventId::new(),
            device_id: cfg.device_id.clone(),
            created_at: now,
            payload: EventPayload::GameSessionFinished(GameSessionFinished {
                session_id: cur.session_id.clone(),
                ended_at: now,
                duration_seconds: duration,
                exit_code: None,
                save_changed,
                max_cpu_percent: cur.max_cpu_percent,
                max_memory_mb: cur.max_memory_mb,
            }),
        };
        let _ = crate::db::enqueue(&conn, &fin);

        if kind == FinishKind::Crash {
            let _ = crate::db::enqueue(
                &conn,
                &Event {
                    event_id: EventId::new(),
                    device_id: cfg.device_id.clone(),
                    created_at: now,
                    payload: EventPayload::GameSessionCrashed(GameSessionCrashed {
                        session_id: cur.session_id.clone(),
                        exit_code: None,
                        signal: None,
                        stderr_tail: None,
                        captured_at: now,
                    }),
                },
            );
        }

        if save_changed {
            if let (Some(p), Some(new_h)) = (cur.save_path.as_ref(), current_save_hash) {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let _ = crate::db::enqueue(
                    &conn,
                    &Event {
                        event_id: EventId::new(),
                        device_id: cfg.device_id.clone(),
                        created_at: now,
                        payload: EventPayload::SaveChanged(SaveChanged {
                            system: cur.system_folder.clone(),
                            save_path: p.clone(),
                            old_hash: cur.save_hash_on_start.clone(),
                            new_hash: new_h,
                            file_size: size,
                            captured_at: now,
                        }),
                    },
                );
            }
        }
    }
}

fn emit_orphaned(cfg: &AgentConfig, cur: &Current) {
    let now = Utc::now();
    if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
        let _ = crate::db::enqueue(
            &conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: now,
                payload: EventPayload::GameSessionOrphaned(GameSessionOrphaned {
                    session_id: cur.session_id.clone(),
                    started_at: cur.started_at,
                    reconciled_at: now,
                    reason: "agent restart with no running emulator".into(),
                }),
            },
        );
    }
}

fn persist(cur: &Current) {
    let Ok(s) = serde_json::to_string(cur) else {
        return;
    };
    let _ = std::fs::create_dir_all(
        Path::new(PERSIST_PATH)
            .parent()
            .unwrap_or(Path::new("/tmp")),
    );
    let _ = std::fs::write(PERSIST_PATH, s);
}

fn load_persisted() -> Option<Current> {
    let s = std::fs::read_to_string(PERSIST_PATH).ok()?;
    serde_json::from_str(&s).ok()
}

fn pid_still_owns(pid: Option<u32>, rom_path: &str) -> bool {
    let Some(pid) = pid else { return false };
    let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    String::from_utf8_lossy(&raw).contains(rom_path)
}

fn find_save(rom_path: &str) -> (Option<PathBuf>, Option<String>) {
    let p = Path::new(rom_path);
    let stem = p.file_stem().and_then(|s| s.to_str());
    let parent = p.parent();
    let Some(stem) = stem else {
        return (None, None);
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(par) = parent {
        for ext in SAVE_EXTS {
            candidates.push(par.join(format!("{stem}.{ext}")));
        }
    }
    candidates.push(PathBuf::from(format!("/roms/savestates/{stem}.state")));
    candidates.push(PathBuf::from(format!("/roms/saves/{stem}.srm")));
    for c in candidates {
        if c.is_file() {
            let h = hash_file(&c);
            return (Some(c), h);
        }
    }
    (None, None)
}

fn hash_file(p: &Path) -> Option<String> {
    let bytes = std::fs::read(p).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(hex::encode(h.finalize()))
}

#[derive(Debug)]
struct Detected {
    rom_path: String,
    system_folder: String,
    core: Option<String>,
    pid: Option<u32>,
}

/// PID-agnostic: scan EVERY /proc/<pid>/cmdline for a ROM-like token.
/// Picks the youngest matching process so launcher wrappers lose to the
/// real emulator they spawned.
fn detect_running_rom() -> Option<Detected> {
    let procs = std::fs::read_dir("/proc").ok()?;
    let mut best: Option<(u64, Detected)> = None;
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
        let joined: String = cmdline_raw
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        if joined.contains("playora-agent") {
            continue;
        }
        let Some(rom) = parse_rom_from_cmdline(&joined) else {
            continue;
        };
        let start_time = read_start_time(pid).unwrap_or(0);
        let system_folder = Path::new(&rom)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let detected = Detected {
            rom_path: rom,
            system_folder,
            core: parse_core_from_cmdline(&joined),
            pid: Some(pid),
        };
        match best.as_ref() {
            Some((t, _)) if *t >= start_time => {}
            _ => best = Some((start_time, detected)),
        }
    }
    best.map(|(_, d)| d)
}

fn read_start_time(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let close = s.rfind(')')?;
    let rest = &s[close + 1..];
    let parts: Vec<&str> = rest.split_whitespace().collect();
    parts.get(19)?.parse().ok()
}

const ROM_EXTS: &[&str] = &[
    ".nes", ".sfc", ".smc", ".gba", ".gb", ".gbc", ".n64", ".z64", ".v64", ".md", ".gen", ".smd",
    ".bin", ".iso", ".chd", ".cso", ".cue", ".gdi", ".pbp", ".elf", ".nds", ".3ds", ".cdi", ".m3u",
    ".zip", ".7z",
];

fn parse_rom_from_cmdline(cmd: &str) -> Option<String> {
    for token in cmd.split_whitespace().rev() {
        let unq = token.trim_matches('"').trim_matches('\'');
        if unq.contains("/roms/") {
            return Some(unq.to_string());
        }
    }
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
    let mut tracker = SessionTracker::new(&cfg);
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
    fn detects_wrapper_shell_cmdline() {
        let cmd = "bash /opt/system/Launchers/snes.sh /roms/snes/Castlevania.smc";
        assert_eq!(
            parse_rom_from_cmdline(cmd),
            Some("/roms/snes/Castlevania.smc".into())
        );
    }

    #[test]
    fn detects_ppsspp_standalone() {
        let cmd = "PPSSPPSDL /roms/psp/GTA.iso";
        assert_eq!(
            parse_rom_from_cmdline(cmd),
            Some("/roms/psp/GTA.iso".into())
        );
    }

    #[test]
    fn detects_drastic_standalone() {
        let cmd = "drastic /roms/nds/Pokemon.nds";
        assert_eq!(
            parse_rom_from_cmdline(cmd),
            Some("/roms/nds/Pokemon.nds".into())
        );
    }

    #[test]
    fn parses_core_name() {
        let c = "retroarch -L /usr/lib/libretro/gba.so /roms/gba/game.gba";
        assert_eq!(parse_core_from_cmdline(c), Some("gba".to_string()));
    }
}
