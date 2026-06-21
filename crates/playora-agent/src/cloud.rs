//! Cloud sync via rclone. Wraps `rclone` binary bundled at
//! /roms/.playora/bin/rclone (or PATH). Uses OAuth device flow so the
//! user can authorize on their phone — no keyboard typing on the R36S.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const REMOTE: &str = "gdrive";
const REMOTE_ROOT: &str = "R36S";

fn rclone_bin() -> PathBuf {
    let bundled = Path::new("/roms/.playora/bin/rclone");
    if bundled.is_file() {
        return bundled.to_path_buf();
    }
    PathBuf::from("rclone")
}

fn rclone_config() -> PathBuf {
    PathBuf::from("/roms/.playora/rclone.conf")
}

fn ensure_rclone() -> Result<()> {
    let bin = rclone_bin();
    let status = Command::new(&bin).arg("version").status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(anyhow!(
            "rclone not found. Expected at /roms/.playora/bin/rclone (bundled) or on PATH."
        )),
    }
}

pub fn cmd_setup() -> Result<()> {
    let _lock = crate::lockfile::acquire("cloud-setup")?;
    ensure_rclone()?;
    println!("== Cloud Setup ==");
    println!("Starting Google Drive OAuth device flow.");
    println!();
    println!("In a moment, this log will show:");
    println!("  1. A URL like https://accounts.google.com/o/oauth2/device");
    println!("  2. A code like ABCD-EFGH");
    println!();
    println!("Open the URL on your PHONE, sign in to Google, type the code.");
    println!("Then come back to the dashboard and check this activity's log.");
    println!();

    let mut child = Command::new(rclone_bin())
        .args([
            "config", "create", REMOTE, "drive", "scope", "drive", "--config",
        ])
        .arg(rclone_config())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("spawn rclone config")?;

    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!("rclone config exit {:?}", status.code()));
    }
    println!();
    println!(
        "SUMMARY: Cloud Setup ok — remote '{REMOTE}' configured at {}",
        rclone_config().display()
    );
    Ok(())
}

pub fn cmd_backup() -> Result<()> {
    let _lock = crate::lockfile::acquire("cloud-backup")?;
    ensure_rclone()?;
    println!("== Cloud Backup ==");
    let bin = rclone_bin();
    let cfg = rclone_config();
    let mut total_files = 0u64;
    for (src, dst) in [
        (
            "/roms/savestates",
            format!("{REMOTE}:{REMOTE_ROOT}/savestates"),
        ),
        ("/roms/.playora", format!("{REMOTE}:{REMOTE_ROOT}/playora")),
    ] {
        if !Path::new(src).exists() {
            println!("skip {src} (missing)");
            continue;
        }
        println!("--> {src} -> {dst}");
        let st = Command::new(&bin)
            .args(["sync", "--config"])
            .arg(&cfg)
            .arg(src)
            .arg(&dst)
            .args(["--stats=10s", "--stats-one-line", "--transfers=4"])
            .status()?;
        if !st.success() {
            return Err(anyhow!("rclone sync failed for {src}"));
        }
        total_files += count_files(Path::new(src));
    }
    println!();
    println!("SUMMARY: Cloud Backup ok ({total_files} source files)");
    Ok(())
}

pub fn cmd_restore() -> Result<()> {
    let _lock = crate::lockfile::acquire("cloud-restore")?;
    ensure_rclone()?;
    println!("== Cloud Restore ==");
    let bin = rclone_bin();
    let cfg = rclone_config();
    for (dst, src) in [
        (
            "/roms/savestates",
            format!("{REMOTE}:{REMOTE_ROOT}/savestates"),
        ),
        ("/roms/.playora", format!("{REMOTE}:{REMOTE_ROOT}/playora")),
    ] {
        std::fs::create_dir_all(dst).ok();
        println!("--> {src} -> {dst}");
        let st = Command::new(&bin)
            .args(["copy", "--config"])
            .arg(&cfg)
            .arg(&src)
            .arg(dst)
            .args(["--stats=10s", "--stats-one-line"])
            .status()?;
        if !st.success() {
            return Err(anyhow!("rclone copy failed for {src}"));
        }
    }
    println!();
    println!("SUMMARY: Cloud Restore ok");
    Ok(())
}

pub fn cmd_status() -> Result<()> {
    ensure_rclone()?;
    println!("== Cloud Status ==");
    let bin = rclone_bin();
    let cfg = rclone_config();
    let v = Command::new(&bin).arg("version").output()?;
    print!("{}", String::from_utf8_lossy(&v.stdout));
    println!("config: {}", cfg.display());
    let remotes = Command::new(&bin)
        .args(["listremotes", "--config"])
        .arg(&cfg)
        .output();
    match remotes {
        Ok(o) if o.status.success() => {
            print!("remotes:\n{}", String::from_utf8_lossy(&o.stdout));
        }
        _ => println!("no remotes configured (run cloud setup)"),
    }
    println!();
    println!("SUMMARY: Cloud Status ok");
    Ok(())
}

fn count_files(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .count() as u64
}
