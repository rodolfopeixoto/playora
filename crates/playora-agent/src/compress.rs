//! CHD compression for PS1 / arcade-style optical-media ROMs.
//!
//! Uses `chdman` (from MAME tools) to convert .cue/.bin or .iso into .chd.
//! Smaller files, native RetroArch support, faster boot. Operates in place
//! per system folder.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const PSX_FOLDERS: &[&str] = &["psx", "ps1", "playstation"];

pub fn cmd_compress_roms(roms_root: &str, dry_run: bool) -> Result<()> {
    let _lock = crate::lockfile::acquire("compress-roms")?;
    let cfg = crate::cfg::load(None).ok();

    if which("chdman").is_none() {
        println!("chdman not found in PATH.");
        println!("Install on dArkOSRE: sudo apt-get install mame-tools");
        println!("Or copy chdman binary into /roms/.playora/bin/ and re-run.");
        return Err(anyhow!("chdman missing"));
    }

    let root = Path::new(roms_root);
    if !root.is_dir() {
        return Err(anyhow!("roms root not found: {roms_root}"));
    }

    let mut found = 0u32;
    let mut converted = 0u32;
    let mut saved_bytes: i64 = 0;
    let mut errors = 0u32;

    for system in PSX_FOLDERS {
        let sys_dir = root.join(system);
        if !sys_dir.is_dir() {
            continue;
        }
        println!("== scanning {} ==", sys_dir.display());
        let entries: Vec<_> = walkdir::WalkDir::new(&sys_dir)
            .max_depth(3)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .collect();
        for entry in entries {
            let p = entry.path();
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            // Only act on canonical disc images.
            if !matches!(ext.as_str(), "cue" | "iso") {
                continue;
            }
            // Skip if a .chd next to it already exists.
            let chd = p.with_extension("chd");
            if chd.exists() {
                continue;
            }
            found += 1;
            let in_size = sources_total_size(p);
            println!("  -> {} ({} MB)", p.display(), in_size / 1024 / 1024);
            if dry_run {
                continue;
            }
            if let Some(c) = &cfg {
                let _ = crate::activity::progress(
                    c,
                    "Compress ROMs",
                    &format!("converting {} -> chd", p.display()),
                );
            }
            let out_tmp = chd.with_extension("chd.part");
            let status = Command::new("chdman")
                .args(["createcd", "-i"])
                .arg(p)
                .arg("-o")
                .arg(&out_tmp)
                .status();
            match status {
                Ok(s) if s.success() => {
                    std::fs::rename(&out_tmp, &chd)?;
                    let out_size = std::fs::metadata(&chd).map(|m| m.len() as i64).unwrap_or(0);
                    saved_bytes += in_size as i64 - out_size;
                    // Delete source(s) only if conversion succeeded.
                    delete_sources(p);
                    converted += 1;
                    println!(
                        "     ok ({:.1} MB → {:.1} MB)",
                        in_size as f64 / 1024.0 / 1024.0,
                        out_size as f64 / 1024.0 / 1024.0
                    );
                }
                Ok(s) => {
                    eprintln!("     chdman exit {:?}", s.code());
                    std::fs::remove_file(&out_tmp).ok();
                    errors += 1;
                }
                Err(e) => {
                    eprintln!("     chdman spawn fail: {e}");
                    errors += 1;
                }
            }
        }
    }

    println!();
    println!("== Compress ROMs detail ==");
    println!("candidates: {found}");
    println!("converted:  {converted}");
    println!("errors:     {errors}");
    println!(
        "space saved: {:.1} MB",
        saved_bytes as f64 / 1024.0 / 1024.0
    );
    println!();
    println!(
        "SUMMARY: {converted} ROMs compressed to CHD, saved {:.1} MB, {errors} errors",
        saved_bytes as f64 / 1024.0 / 1024.0
    );
    if errors > 0 {
        return Err(anyhow!("{errors} errors"));
    }
    Ok(())
}

fn sources_total_size(cue_or_iso: &Path) -> u64 {
    let mut total = std::fs::metadata(cue_or_iso).map(|m| m.len()).unwrap_or(0);
    if cue_or_iso.extension().and_then(|s| s.to_str()) == Some("cue") {
        for bin in parse_cue_bins(cue_or_iso) {
            let p = cue_or_iso.parent().unwrap_or(Path::new(".")).join(bin);
            total += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

fn delete_sources(cue_or_iso: &Path) {
    if cue_or_iso.extension().and_then(|s| s.to_str()) == Some("cue") {
        for bin in parse_cue_bins(cue_or_iso) {
            let p = cue_or_iso.parent().unwrap_or(Path::new(".")).join(bin);
            std::fs::remove_file(&p).ok();
        }
    }
    std::fs::remove_file(cue_or_iso).ok();
}

fn parse_cue_bins(cue: &Path) -> Vec<String> {
    let s = match std::fs::read_to_string(cue) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    s.lines()
        .filter_map(|l| {
            let t = l.trim();
            let lower = t.to_lowercase();
            if !lower.starts_with("file ") {
                return None;
            }
            let q = t.find('"')?;
            let rest = &t[q + 1..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

fn which(tool: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let p = Path::new(dir).join(tool);
        if p.is_file() {
            return Some(p);
        }
    }
    // Also check our bundled bin dir.
    let bundled = Path::new("/roms/.playora/bin").join(tool);
    if bundled.is_file() {
        return Some(bundled);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cue_bin_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let cue = tmp.path().join("game.cue");
        std::fs::write(
            &cue,
            "FILE \"game.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        let bins = parse_cue_bins(&cue);
        assert_eq!(bins, vec!["game.bin"]);
    }
}
