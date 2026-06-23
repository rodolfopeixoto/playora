//! `audit-roms` — non-destructive inventory + integrity report for /roms.
//!
//! Output:
//! - JSON report at `/roms/.playora/reports/audit-YYYYMMDD-HHMMSS.json`
//! - Human summary on TTY
//! - `RomAuditResult` event for the dashboard
//!
//! Detects: zero-byte files, duplicates (by name+size), broken CUE refs,
//! broken M3U refs, unknown extensions (not in SystemSpec), invalid
//! gamelist.xml, missing BIOS for systems that require one, macOS junk.

use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct Report {
    audit_id: String,
    captured_at: chrono::DateTime<Utc>,
    roms_root: String,
    per_system: BTreeMap<String, SystemReport>,
    summary: Totals,
    macos_junk: Vec<String>,
    broken_cue: Vec<String>,
    broken_m3u: Vec<String>,
    invalid_gamelists: Vec<String>,
    bios_missing: Vec<String>,
    unknown_extensions: BTreeMap<String, u32>,
    duplicates: Vec<DuplicateGroup>,
    zero_byte: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SystemReport {
    folder: String,
    rom_count: u32,
    total_bytes: u64,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct Totals {
    roms_total: u32,
    roms_orphan: u32,
    broken_cue: u32,
    broken_m3u: u32,
    zero_byte: u32,
    duplicates: u32,
    macos_junk: u32,
    gamelist_invalid: u32,
    unknown_extensions: u32,
}

#[derive(Debug, Serialize)]
struct DuplicateGroup {
    key: String,
    size: u64,
    paths: Vec<String>,
}

const KNOWN_SKIP_DIRS: &[&str] = &[
    ".playora",
    "_inbox",
    "savestates",
    "themes",
    "BGM",
    "bgmusic",
    "cheats",
    "tools",
    "ports",
    "backup",
    ".update",
    ".r36s-smart",
    ".darkos",
    "System Volume Information",
    ".Spotlight-V100",
    ".fseventsd",
];

/// Systems that effectively *require* user-supplied BIOS to run on RA cores.
const BIOS_REQUIRED: &[(&str, &[&str])] = &[
    ("psx", &["scph5500.bin", "scph5501.bin", "scph5502.bin"]),
    ("psp", &[]), // not strictly required but worth surfacing
    ("dreamcast", &["dc_boot.bin", "dc_flash.bin"]),
    ("saturn", &["sega_101.bin", "mpr-17933.bin"]),
    ("neogeo", &["neogeo.zip"]),
];

pub fn cmd_audit(cfg: AgentConfig, hash_mode: bool) -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header("Audit ROMs");

    let roms_root = cfg
        .rom_paths
        .first()
        .cloned()
        .unwrap_or_else(|| "/roms".into());
    let root = PathBuf::from(&roms_root);
    if !root.is_dir() {
        ttyui::row("roms_root", "missing", Status::Fail);
        return Ok(());
    }

    let mut per_system: BTreeMap<String, SystemReport> = BTreeMap::new();
    let mut by_key: HashMap<String, Vec<(PathBuf, u64)>> = HashMap::new();
    let mut zero_byte: Vec<String> = Vec::new();
    let mut unknown_ext: BTreeMap<String, u32> = BTreeMap::new();
    let mut roms_total = 0u32;

    let known_systems: Vec<&'static playora_common::systems::SystemSpec> =
        playora_common::systems::SYSTEMS.iter().collect();

    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let sys_path = entry.path();
        if !sys_path.is_dir() {
            continue;
        }
        let sys_name = sys_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        if sys_name.starts_with('.') || KNOWN_SKIP_DIRS.contains(&sys_name.as_str()) {
            continue;
        }
        let spec = known_systems
            .iter()
            .find(|s| s.folder.eq_ignore_ascii_case(&sys_name))
            .copied();
        let mut count = 0u32;
        let mut bytes = 0u64;
        for f in walkdir::WalkDir::new(&sys_path)
            .max_depth(3)
            .into_iter()
            .flatten()
        {
            if !f.file_type().is_file() {
                continue;
            }
            let p = f.path().to_path_buf();
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            // Skip save-like sidecars + xml + images
            if matches!(
                ext.as_str(),
                "srm"
                    | "sav"
                    | "state"
                    | "rtc"
                    | "mcr"
                    | "fla"
                    | "sa1"
                    | "sa2"
                    | "eep"
                    | "auto"
                    | "xml"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "webp"
                    | "txt"
                    | "md"
                    | "ds_store"
            ) {
                continue;
            }
            let Ok(md) = std::fs::metadata(&p) else {
                continue;
            };
            let size = md.len();
            if size == 0 {
                zero_byte.push(p.display().to_string());
                continue;
            }
            // Known-extension check
            if let Some(s) = spec {
                if !s.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                    *unknown_ext
                        .entry(format!("{}/{ext}", s.folder))
                        .or_default() += 1;
                }
            }
            // Duplicate key: SHA-256 prefix when --hash, otherwise name+size.
            let key = if hash_mode {
                match hash_with_cache(&p, size) {
                    Some(h) => format!("sha256:{h}"),
                    None => format!(
                        "{}|{size}",
                        p.file_name()
                            .map(|n| n.to_string_lossy().to_lowercase())
                            .unwrap_or_default()
                    ),
                }
            } else {
                format!(
                    "{}|{size}",
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                )
            };
            by_key.entry(key).or_default().push((p, size));
            count += 1;
            bytes += size;
            roms_total += 1;
        }
        per_system.insert(
            sys_name.clone(),
            SystemReport {
                folder: sys_name,
                rom_count: count,
                total_bytes: bytes,
            },
        );
    }

    let duplicates: Vec<DuplicateGroup> = by_key
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, v)| DuplicateGroup {
            key: k,
            size: v.first().map(|(_, s)| *s).unwrap_or(0),
            paths: v.iter().map(|(p, _)| p.display().to_string()).collect(),
        })
        .collect();

    let macos_junk = list_macos_junk(&root);
    let broken_cue = scan_broken_cues(&root);
    let broken_m3u = scan_broken_m3us(&root);
    let invalid_gamelists = scan_gamelists(&root);
    let bios_missing = scan_missing_bios(&root);

    let totals = Totals {
        roms_total,
        roms_orphan: 0,
        broken_cue: broken_cue.len() as u32,
        broken_m3u: broken_m3u.len() as u32,
        zero_byte: zero_byte.len() as u32,
        duplicates: duplicates.iter().map(|d| d.paths.len() as u32 - 1).sum(),
        macos_junk: macos_junk.len() as u32,
        gamelist_invalid: invalid_gamelists.len() as u32,
        unknown_extensions: unknown_ext.values().sum::<u32>(),
    };

    let audit_id = format!("audit_{}", Uuid::new_v4().simple());
    let report = Report {
        audit_id: audit_id.clone(),
        captured_at: Utc::now(),
        roms_root: roms_root.clone(),
        per_system,
        summary: totals,
        macos_junk: macos_junk.clone(),
        broken_cue: broken_cue.clone(),
        broken_m3u: broken_m3u.clone(),
        invalid_gamelists: invalid_gamelists.clone(),
        bios_missing: bios_missing.clone(),
        unknown_extensions: unknown_ext,
        duplicates,
        zero_byte: zero_byte.clone(),
    };

    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let reports_dir = PathBuf::from("/roms/.playora/reports");
    let _ = std::fs::create_dir_all(&reports_dir);
    let report_path = reports_dir.join(format!("audit-{stamp}.json"));
    if let Ok(j) = serde_json::to_string_pretty(&report) {
        let _ = atomic_write(&report_path, j.as_bytes());
    }

    // TTY summary
    ttyui::section("Per-system");
    for (sys, sr) in &report.per_system {
        ttyui::row(
            sys,
            &format!(
                "{} ROMs ({:.1} MiB)",
                sr.rom_count,
                sr.total_bytes as f64 / 1024.0 / 1024.0
            ),
            Status::Info,
        );
    }
    ttyui::section("Findings");
    ttyui::row("ROMs total", &totals.roms_total.to_string(), Status::Info);
    severity_row("zero-byte files", totals.zero_byte);
    severity_row("duplicate ROMs", totals.duplicates);
    severity_row("broken CUE", totals.broken_cue);
    severity_row("broken M3U", totals.broken_m3u);
    severity_row("invalid gamelists", totals.gamelist_invalid);
    severity_row("macOS junk", totals.macos_junk);
    severity_row("unknown extensions", totals.unknown_extensions);
    if !bios_missing.is_empty() {
        ttyui::row("BIOS missing", &bios_missing.join(", "), Status::Warn);
    }
    ttyui::section("Report");
    ttyui::row("path", &report_path.display().to_string(), Status::Ok);

    // Emit event
    if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
        let payload = RomAuditResult {
            audit_id,
            roms_total: totals.roms_total,
            roms_orphan: totals.roms_orphan,
            broken_cue: totals.broken_cue,
            broken_m3u: totals.broken_m3u,
            zero_byte: totals.zero_byte,
            duplicates: totals.duplicates,
            macos_junk: totals.macos_junk,
            gamelist_invalid: totals.gamelist_invalid,
            bios_missing,
            unknown_extensions: totals.unknown_extensions,
            report_path: Some(report_path.display().to_string()),
            captured_at: Utc::now(),
        };
        let _ = crate::db::enqueue(
            &conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: Utc::now(),
                payload: EventPayload::RomAuditResult(payload),
            },
        );
    }

    println!();
    println!("SUMMARY: audit-roms — see {}", report_path.display());
    Ok(())
}

fn severity_row(name: &str, count: u32) {
    use crate::ttyui::{self, Status};
    let s = if count == 0 {
        Status::Ok
    } else if count < 20 {
        Status::Warn
    } else {
        Status::Fail
    };
    crate::ttyui::row(name, &count.to_string(), s);
    let _ = ttyui::row;
    let _ = s;
}

fn list_macos_junk(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .flatten()
    {
        let p = f.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == ".DS_Store" || name == "thumbs.db" || name.starts_with("._") {
            out.push(p.display().to_string());
        } else if p.is_dir() && name == "__MACOSX" {
            out.push(p.display().to_string());
        }
    }
    out
}

fn scan_broken_cues(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .flatten()
    {
        if !f.file_type().is_file() {
            continue;
        }
        let p = f.path();
        if !p
            .extension()
            .map(|e| e.eq_ignore_ascii_case("cue"))
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(p) else {
            continue;
        };
        let parent = p.parent().unwrap_or(Path::new("."));
        for l in content.lines() {
            let t = l.trim();
            if let Some(rest) = t.strip_prefix("FILE ") {
                if let Some(start) = rest.find('"') {
                    if let Some(end) = rest[start + 1..].find('"') {
                        let fname = &rest[start + 1..start + 1 + end];
                        if !parent.join(fname).exists() {
                            out.push(format!("{} -> missing {fname}", p.display()));
                        }
                    }
                }
            }
        }
    }
    out
}

fn scan_broken_m3us(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .flatten()
    {
        if !f.file_type().is_file() {
            continue;
        }
        let p = f.path();
        if !p
            .extension()
            .map(|e| e.eq_ignore_ascii_case("m3u"))
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(p) else {
            continue;
        };
        let parent = p.parent().unwrap_or(Path::new("."));
        for l in content.lines() {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if !parent.join(t).exists() {
                out.push(format!("{} -> missing {t}", p.display()));
            }
        }
    }
    out
}

fn scan_gamelists(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in walkdir::WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .flatten()
    {
        if !f.file_type().is_file() {
            continue;
        }
        let p = f.path();
        if p.file_name().and_then(|n| n.to_str()) != Some("gamelist.xml") {
            continue;
        }
        match std::fs::read_to_string(p) {
            Ok(c) => {
                let t = c.trim_start();
                if !t.starts_with("<?xml") || !c.contains("<gameList") || !c.contains("</gameList>")
                {
                    out.push(p.display().to_string());
                }
            }
            Err(_) => out.push(p.display().to_string()),
        }
    }
    out
}

fn scan_missing_bios(root: &Path) -> Vec<String> {
    let bios_dir = root.join("bios");
    let mut present: BTreeSet<String> = BTreeSet::new();
    if bios_dir.is_dir() {
        for e in walkdir::WalkDir::new(&bios_dir).into_iter().flatten() {
            if e.file_type().is_file() {
                if let Some(n) = e.path().file_name().and_then(|n| n.to_str()) {
                    present.insert(n.to_lowercase());
                }
            }
        }
    }
    let mut missing = Vec::new();
    for (system, files) in BIOS_REQUIRED {
        let system_dir = root.join(system);
        let has_roms = system_dir.is_dir()
            && std::fs::read_dir(&system_dir)
                .map(|mut it| it.next().is_some())
                .unwrap_or(false);
        if !has_roms {
            continue;
        }
        for f in *files {
            if !present.contains(&f.to_lowercase()) {
                missing.push(format!("{system}: {f}"));
            }
        }
    }
    missing
}

/// Full SHA-256 with an on-disk cache keyed by (path, size, mtime). Cache
/// table is created by migration v3. Re-runs are near-instant; first run
/// streams the file through Sha256 in 1 MiB chunks regardless of size.
fn hash_with_cache(path: &Path, size: u64) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let rom_path = path.display().to_string();
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let conn_opt = crate::db::open(&crate::cfg::db_path()).ok();
    if let Some(conn) = conn_opt.as_ref() {
        let cached: Option<(i64, i64, String)> = conn
            .query_row(
                "SELECT file_size, mtime, sha256 FROM file_hashes WHERE path=?1",
                rusqlite::params![rom_path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        if let Some((csize, cmtime, csha)) = cached {
            if csize as u64 == size && cmtime == mtime && !csha.is_empty() {
                return Some(csha);
            }
        }
    }

    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let sha = hex::encode(h.finalize());

    if let Some(conn) = conn_opt.as_ref() {
        let _ = conn.execute(
            "INSERT INTO file_hashes(path, file_size, mtime, sha256, computed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
               file_size=excluded.file_size,
               mtime=excluded.mtime,
               sha256=excluded.sha256,
               computed_at=excluded.computed_at",
            rusqlite::params![
                rom_path,
                size as i64,
                mtime,
                sha,
                chrono::Utc::now().to_rfc3339()
            ],
        );
    }
    Some(sha)
}

fn atomic_write(p: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = p.with_extension("tmp.playora");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, p)
}
