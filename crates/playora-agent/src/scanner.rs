use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

const SKIP_DIRS: &[&str] = &[
    "savestates",
    "themes",
    "BGM",
    "bgmusic",
    "cheats",
    "tools",
    "backup",
    "ports_scripts",
    ".update",
    ".r36s-smart",
    ".playora",
    ".darkos",
    "_inbox",
    "System Volume Information",
    ".Spotlight-V100",
    ".fseventsd",
];

const SAVE_LIKE: &[&str] = &[
    "srm", "sav", "state", "rtc", "mcr", "fla", "sa1", "sa2", "eep", "xml", "txt", "auto",
];

pub fn cmd_scan(cfg: AgentConfig) -> Result<()> {
    let _lock = crate::lockfile::acquire("scan")?;
    let mut conn = crate::db::open(&crate::cfg::db_path())?;
    let mut count = 0u64;
    let mut skipped = 0u64;
    let mut per_system: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();

    let tx = conn.transaction()?;
    let mut last_progress = std::time::Instant::now();

    for root in &cfg.rom_paths {
        let root = Path::new(root);
        if !root.is_dir() {
            continue;
        }
        for sys_entry in std::fs::read_dir(root)? {
            let sys_entry = sys_entry?;
            let sys_path = sys_entry.path();
            if !sys_path.is_dir() {
                continue;
            }
            let sys_name = sys_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if sys_name.starts_with('.') || SKIP_DIRS.contains(&sys_name.as_str()) {
                continue;
            }
            let system = GameSystem::from_folder(&sys_name);
            for entry in walkdir::WalkDir::new(&sys_path)
                .max_depth(2)
                .into_iter()
                .flatten()
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let p = entry.path();
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if SAVE_LIKE.contains(&ext.as_str()) {
                    continue;
                }
                let md = match std::fs::metadata(p) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let size = md.len();
                let rom_path = p.display().to_string();

                // Skip if already scanned with same path+size (avoid expensive hash).
                let existing_size: Option<i64> = tx
                    .query_row(
                        "SELECT file_size FROM games WHERE rom_path=?1",
                        rusqlite::params![rom_path],
                        |r| r.get(0),
                    )
                    .ok();
                if existing_size == Some(size as i64) {
                    skipped += 1;
                    *per_system.entry(sys_name.clone()).or_default() += 1;
                    continue;
                }

                let rom_hash = match quick_hash(p) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                let meta = GameMetadata {
                    system,
                    name: p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string(),
                    rom_path: rom_path.clone(),
                    rom_hash: Some(rom_hash),
                    file_size: size,
                    extension: ext,
                    image_path: None,
                    metadata: serde_json::Value::Null,
                };
                tx.execute(
                    "INSERT INTO games(system,name,rom_path,rom_hash,file_size,extension,metadata_json,last_scanned_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                     ON CONFLICT(rom_path) DO UPDATE SET
                       system=excluded.system,name=excluded.name,rom_hash=excluded.rom_hash,
                       file_size=excluded.file_size,extension=excluded.extension,
                       metadata_json=excluded.metadata_json,last_scanned_at=excluded.last_scanned_at",
                    rusqlite::params![
                        format!("{:?}", meta.system).to_lowercase(),
                        meta.name, meta.rom_path, meta.rom_hash,
                        meta.file_size as i64, meta.extension,
                        serde_json::to_string(&meta.metadata)?, Utc::now().to_rfc3339()
                    ],
                )?;
                let ev = Event {
                    event_id: EventId::new(),
                    device_id: cfg.device_id.clone(),
                    created_at: Utc::now(),
                    payload: EventPayload::RomScanned(RomScanned {
                        metadata: meta,
                        scanned_at: Utc::now(),
                    }),
                };
                tx.execute(
                    "INSERT OR IGNORE INTO events_outbox(event_id, event_type, payload_json, status, retry_count, created_at)
                     VALUES (?1, 'rom_scanned', ?2, 'pending', 0, ?3)",
                    rusqlite::params![ev.event_id.0, serde_json::to_string(&ev)?, ev.created_at.to_rfc3339()],
                )?;
                count += 1;
                *per_system.entry(sys_name.clone()).or_default() += 1;

                // Time-based progress (every 2s) — DB-cheap.
                if last_progress.elapsed().as_secs() >= 2 {
                    last_progress = std::time::Instant::now();
                    println!("  progress: {count} new, {skipped} unchanged ({sys_name})");
                }
            }
        }
    }
    tx.commit()?;

    // Final progress emit outside transaction (small extra write).
    let _ = crate::activity::progress(
        &cfg,
        "Scan ROMs",
        &format!("done: {count} new + {skipped} unchanged"),
    );

    println!();
    println!(
        "SUMMARY: {count} new ROMs, {skipped} unchanged, {} systems",
        per_system.len()
    );
    for (sys, n) in &per_system {
        println!("  {sys:20} {n}");
    }
    Ok(())
}

// Hash first + last 64 KiB only (fast). Real cryptographic verification done on download path.
fn quick_hash(p: &Path) -> Result<String> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(p)?;
    let len = f.metadata()?.len();
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let n = f.read(&mut buf)?;
    h.update(&buf[..n]);
    if len > 128 * 1024 {
        f.seek(SeekFrom::End(-((buf.len() as i64).min(len as i64))))?;
        let n = f.read(&mut buf)?;
        h.update(&buf[..n]);
    }
    h.update(len.to_le_bytes());
    Ok(hex::encode(h.finalize()))
}
