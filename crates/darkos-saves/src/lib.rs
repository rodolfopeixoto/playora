//! Save state + battery backup management.

use darkos_core::{Error, Result};
use std::path::{Path, PathBuf};

const SAVE_EXTS: &[&str] = &[
    "srm", "sav", "state", "state1", "state2", "state3", "state4", "state5", "state6", "state7",
    "state8", "state9", "rtc", "mcr", "fla", "sa1", "sa2", "eep",
];

const SAVE_DIRS: &[&str] = &["savestates", "saves"];

/// Find all save files under the roms tree.
pub fn list_saves(roms_dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for entry in walkdir::WalkDir::new(roms_dir)
        .max_depth(5)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        // include known save-dir contents
        if let Some(parent) = p.parent() {
            let pname = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if SAVE_DIRS.contains(&pname) {
                out.push(p.to_path_buf());
                continue;
            }
        }
        if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
            if SAVE_EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
                out.push(p.to_path_buf());
            }
        }
    }
    out
}

/// Copy all saves under roms_dir into a snapshot dir tagged with timestamp.
pub fn snapshot(roms_dir: &Path, snapshot_root: &Path) -> Result<PathBuf> {
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let dest = snapshot_root.join(format!("saves_{ts}"));
    std::fs::create_dir_all(&dest)?;
    let saves = list_saves(roms_dir);
    for src in &saves {
        // preserve relative path from roms_dir
        let rel = src.strip_prefix(roms_dir).unwrap_or(src);
        let to = dest.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, &to).map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("{}: {e}", src.display()),
            ))
        })?;
    }
    Ok(dest)
}
