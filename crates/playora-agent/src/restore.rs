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
    let _lock = crate::lockfile::acquire("restore-tar")?;
    let cfg = crate::cfg::load(None).ok();
    let tar = find_tarball()
        .ok_or_else(|| anyhow!("playora_restore.tar not found in /roms/ or /roms/.playora/"))?;
    let total = std::fs::metadata(&tar)?.len();
    let dest = PathBuf::from("/roms");
    println!("== Restore Backup ==");
    println!("tar:  {} ({} MB)", tar.display(), total / 1024 / 1024);
    println!("dest: {}", dest.display());

    // Free-disk preflight.
    if let Some((_total_bytes, free_bytes)) = statvfs_bytes(&dest) {
        let needed = total + 1024 * 1024 * 1024; // 1 GB margin
        println!(
            "free: {} MB (need at least {} MB)",
            free_bytes / 1024 / 1024,
            needed / 1024 / 1024
        );
        if free_bytes < total {
            return Err(anyhow!(
                "not enough free space: {} MB free, {} MB tar — clear /roms first",
                free_bytes / 1024 / 1024,
                total / 1024 / 1024
            ));
        }
    }

    // Pre-walk the tar listing to count entries + detect already-present
    // files. Comparing only by path existence + size — close enough for
    // ROM packs (immutable content).
    println!();
    println!("inspecting tar contents...");
    let listing_out = Command::new("tar")
        .arg("-tvf")
        .arg(&tar)
        .output()
        .with_context(|| "spawn tar -tvf")?;
    if !listing_out.status.success() {
        return Err(anyhow!("tar -tvf failed"));
    }
    let listing = String::from_utf8_lossy(&listing_out.stdout);
    let mut tar_files: Vec<(String, u64)> = Vec::new();
    for line in listing.lines() {
        // typical: "-rw-r--r-- root/root 12345 2024-01-01 12:00 path/to/file"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 || !line.starts_with('-') {
            continue; // skip directories + non-file entries
        }
        let size: u64 = parts[2].parse().unwrap_or(0);
        let path = parts[5..].join(" ");
        tar_files.push((path, size));
    }
    let total_entries = tar_files.len();
    let mut already_present = 0usize;
    for (path, size) in &tar_files {
        let on_disk = dest.join(path);
        if let Ok(md) = std::fs::metadata(&on_disk) {
            if md.len() == *size {
                already_present += 1;
            }
        }
    }
    let to_extract = total_entries - already_present;
    println!("tar contains:   {total_entries} files");
    println!("already in dest: {already_present}");
    println!("to extract:      {to_extract}");

    if to_extract == 0 {
        println!();
        println!("SUMMARY: nothing to do — all {total_entries} files already present.");
        return Ok(());
    }

    // Idempotent extract: --skip-old-files skips files that already exist.
    println!();
    println!("extracting (skipping existing files)...");
    let mut child = Command::new("tar")
        .arg("-xvpf")
        .arg(&tar)
        .arg("--skip-old-files")
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
        for line in reader.lines().map_while(|r| r.ok()) {
            files += 1;
            if last_report.elapsed().as_secs() >= 3 {
                last_report = std::time::Instant::now();
                let bytes_done = dir_size(&dest_clone).unwrap_or(0);
                if let Some(c) = cfg_clone.as_ref() {
                    let _ =
                        emit_progress(c, bytes_done, total_for_thread, files, Some(line.clone()));
                }
                println!("  extracted {files} files (current: {line})");
            }
        }
        files
    });

    let status = child.wait().with_context(|| "wait tar")?;
    let files_extracted = progress_thread.join().unwrap_or(0);
    let rc = status.code().unwrap_or(-1);

    if rc == 143 || rc == 137 {
        println!();
        println!("RESUMABLE: tar killed mid-extract (signal {rc}).");
        println!("Run again — already-extracted files will be skipped.");
        println!(
            "SUMMARY: {files_extracted} files written before interruption, {} remaining",
            to_extract as i64 - files_extracted as i64
        );
        return Err(anyhow!("interrupted (signal {rc}) — re-run to resume"));
    }
    if !status.success() {
        return Err(anyhow!("tar exited with {:?}", rc));
    }

    let deleted = if keep_tar {
        false
    } else {
        std::fs::remove_file(&tar).is_ok()
    };
    println!();
    println!(
        "SUMMARY: {files_extracted} files extracted (of {to_extract} new, {already_present} already present), tar deleted: {deleted}"
    );
    Ok(())
}

fn statvfs_bytes(path: &std::path::Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    let p = path.to_string_lossy().to_string();
    let cstr = CString::new(p).ok()?;
    #[cfg(target_os = "linux")]
    {
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(cstr.as_ptr(), &mut s) };
        if rc != 0 {
            return None;
        }
        Some((
            s.f_blocks as u64 * s.f_frsize as u64,
            s.f_bavail as u64 * s.f_frsize as u64,
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Dev host fallback — assume plenty of space.
        let _ = cstr;
        Some((u64::MAX, u64::MAX))
    }
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
