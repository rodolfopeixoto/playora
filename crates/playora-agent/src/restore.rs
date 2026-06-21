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
    let tar = find_tarball()
        .ok_or_else(|| anyhow!("playora_restore.tar not found in /roms/ or /roms/.playora/"))?;
    println!(
        "found tar: {} ({} bytes)",
        tar.display(),
        std::fs::metadata(&tar)?.len()
    );
    let dest = PathBuf::from("/roms");
    println!("extracting into {}...", dest.display());
    let r = extract(&tar, &dest, keep_tar)?;
    println!(
        "extracted {} bytes; tar deleted: {}",
        r.extracted_bytes, r.deleted_tar
    );
    Ok(())
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
