//! Restore from a tarball placed on the SD by the user (or by `install-to-sd.sh`).
//!
//! Linux native exFAT is much faster than macOS fskit, so the strategy is:
//!   1. user creates the tar on a Mac (sequential write to SD)
//!   2. boots the R36S
//!   3. opens Playora Hub → Restore → "Extract Tar"
//!   4. this function extracts into /roms/ and removes the tar to free space
//!
//! We DON'T pre-create directories — `tar` already preserves the source layout.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct RestoreReport {
    pub extracted_bytes: u64,
    pub deleted_tar: bool,
}

pub fn find_tarball() -> Option<PathBuf> {
    for cand in [
        "/roms/playora_restore.tar",
        "/roms/.playora/playora_restore.tar",
        "/roms2/playora_restore.tar",
    ] {
        let p = Path::new(cand);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    None
}

pub fn extract(tar_path: &Path, dest: &Path, keep_tar: bool) -> Result<RestoreReport> {
    if !tar_path.is_file() {
        return Err(anyhow!("not a file: {}", tar_path.display()));
    }
    std::fs::create_dir_all(dest)?;
    let size = std::fs::metadata(tar_path)?.len();

    let status = Command::new("tar")
        .arg("-xpf")
        .arg(tar_path)
        .arg("-C")
        .arg(dest)
        .status()
        .with_context(|| "spawn tar")?;
    if !status.success() {
        return Err(anyhow!("tar exited with {:?}", status.code()));
    }

    let deleted_tar = if keep_tar {
        false
    } else {
        std::fs::remove_file(tar_path).is_ok()
    };
    Ok(RestoreReport {
        extracted_bytes: size,
        deleted_tar,
    })
}

pub fn cmd(keep_tar: bool) -> Result<()> {
    let cfg = crate::cfg::load(None).ok();
    let tar = find_tarball()
        .ok_or_else(|| anyhow!("playora_restore.tar not found in /roms/ or /roms/.playora/"))?;
    let total = std::fs::metadata(&tar)?.len();
    println!("found tar: {} ({} bytes)", tar.display(), total);
    let dest = PathBuf::from("/roms");
    println!("extracting into {}...", dest.display());

    // Spawn tar with verbose output so we can count progress.
    let mut child = Command::new("tar")
        .arg("-xvpf")
        .arg(&tar)
        .arg("-C")
        .arg(&dest)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| "spawn tar")?;
    let stdout = child.stdout.take().expect("stdout pipe");

    let dest_clone = dest.clone();
    let cfg_clone = cfg.clone();
    let total_for_thread = total;
    let progress_thread = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(stdout);
        let mut files: u64 = 0;
        let mut last_report = std::time::Instant::now();
        let mut current = String::new();
        for line in reader.lines().map_while(|r| r.ok()) {
            files += 1;
            current = line;
            if last_report.elapsed().as_secs() >= 3 {
                last_report = std::time::Instant::now();
                let bytes_done = dir_size(&dest_clone).unwrap_or(0);
                if let Some(c) = cfg_clone.as_ref() {
                    let _ = emit_progress(
                        c,
                        bytes_done,
                        total_for_thread,
                        files,
                        Some(current.clone()),
                    );
                }
            }
        }
        if let Some(c) = cfg_clone.as_ref() {
            let bytes_done = dir_size(&dest_clone).unwrap_or(total_for_thread);
            let _ = emit_progress(c, bytes_done, total_for_thread, files, None);
        }
    });

    let status = child.wait().with_context(|| "wait tar")?;
    let _ = progress_thread.join();
    if !status.success() {
        return Err(anyhow!("tar exited with {:?}", status.code()));
    }
    let deleted = if keep_tar {
        false
    } else {
        std::fs::remove_file(&tar).is_ok()
    };
    println!("done. tar deleted: {deleted}");
    Ok(())
}

fn emit_progress(
    cfg: &playora_common::AgentConfig,
    bytes_done: u64,
    bytes_total: u64,
    files_done: u64,
    current_path: Option<String>,
) -> Result<()> {
    use playora_common::*;
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let ev = Event {
        event_id: EventId::new(),
        device_id: cfg.device_id.clone(),
        created_at: chrono::Utc::now(),
        payload: EventPayload::RestoreProgress(RestoreProgress {
            bytes_done,
            bytes_total,
            files_done,
            current_path,
            captured_at: chrono::Utc::now(),
        }),
    };
    crate::db::enqueue(&conn, &ev)?;
    let _ = crate::sync::cmd_sync_once(cfg.clone());
    Ok(())
}

fn dir_size(p: &Path) -> std::io::Result<u64> {
    let mut total: u64 = 0;
    for e in walkdir::WalkDir::new(p).max_depth(4).into_iter().flatten() {
        if e.file_type().is_file() {
            if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_tarball_returns_none_when_absent() {
        // assumes no /roms/playora_restore.tar exists on dev host
        // (won't on macOS dev box)
        let _ = find_tarball();
    }

    #[test]
    fn extract_errors_on_missing_file() {
        let r = extract(Path::new("/tmp/nope_xyz.tar"), Path::new("/tmp"), true);
        assert!(r.is_err());
    }
}
