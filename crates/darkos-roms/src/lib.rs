//! ROM management: scan, index, download with disk-space check.

use darkos_core::{Error, Result};
use darkos_db::{Db, RomRecord};
use darkos_storage::disk_usage;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Scan all subdirectories of `roms_dir` (one per system) and index files.
/// Skips hidden dirs, .update, .r36s-smart, savestates, themes, tools, BGM, bgmusic.
pub fn scan_into_db(roms_dir: &Path, db: &Db, hash: bool) -> Result<usize> {
    let skip: &[&str] = &[
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
        ".darkos",
        "System Volume Information",
        ".Spotlight-V100",
        ".fseventsd",
    ];
    let mut count = 0;
    for entry in std::fs::read_dir(roms_dir)? {
        let entry = entry?;
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') || skip.iter().any(|s| *s == name) {
            continue;
        }
        let system = name.into_owned();
        for sub in walkdir::WalkDir::new(&p).max_depth(2).into_iter().flatten() {
            if !sub.file_type().is_file() {
                continue;
            }
            let path = sub.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            // skip save/state/meta files at index time
            if matches!(
                ext.as_str(),
                "srm" | "sav" | "state" | "rtc" | "mcr" | "fla" | "xml" | "txt" | "auto"
            ) {
                continue;
            }
            let md = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size = md.len();
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let sha = if hash { Some(file_sha256(path)?) } else { None };
            let rec = RomRecord {
                system: system.clone(),
                path: path.display().to_string(),
                name: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string(),
                sha256: sha,
                size_bytes: size,
                mtime,
            };
            db.upsert_rom(&rec)?;
            count += 1;
        }
    }
    Ok(count)
}

fn file_sha256(p: &Path) -> Result<String> {
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(p)?;
    let mut buf = [0u8; 1 << 14];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Download a ROM to <dest_dir>/<filename> after checking enough free space.
/// Refuses to overwrite existing file unless `overwrite` is true.
pub fn download_rom(
    url: &str,
    dest_dir: &Path,
    filename: &str,
    expected_size_hint: Option<u64>,
    overwrite: bool,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let dest = dest_dir.join(filename);
    if dest.exists() && !overwrite {
        return Err(Error::Other(format!("file exists: {}", dest.display())));
    }
    let du = disk_usage(dest_dir)?;
    if let Some(hint) = expected_size_hint {
        // require 1.5x hint as safety margin
        let need = hint.saturating_mul(3) / 2;
        if du.free_bytes < need {
            return Err(Error::Other(format!(
                "not enough free space: have {} need ~{}",
                bytesize::ByteSize(du.free_bytes),
                bytesize::ByteSize(need)
            )));
        }
    }

    let resp = ureq::get(url)
        .call()
        .map_err(|e| Error::Net(e.to_string()))?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&dest)?;
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])?;
    }
    Ok(dest)
}
