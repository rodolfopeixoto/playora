//! Cloud sync via rclone with a QR-code paired setup flow.
//!
//! The R36S has no browser, so OAuth happens on the user's phone:
//!   1. Agent runs `rclone config create gdrive drive scope drive
//!      config_is_local false` non-interactively. rclone prints the
//!      one-liner `rclone authorize "drive" "<blob>"` and waits on stdin
//!      for the resulting token.
//!   2. Agent posts the blob + a dashboard setup URL via an activity
//!      summary. Generates an ASCII QR of the dashboard URL.
//!   3. User scans the QR with their phone, follows the dashboard's
//!      step-by-step (install rclone on their PC, paste blob, run the
//!      command, paste the token JSON back into the dashboard form).
//!   4. Server stores the token; agent polls every 5s and feeds it to
//!      rclone's stdin. rclone writes the config; we're done.

use anyhow::{anyhow, Context, Result};
use playora_common::AgentConfig;
use qrcode::QrCode;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const REMOTE: &str = "gdrive";
const REMOTE_ROOT: &str = "R36S";
const SETUP_TIMEOUT_SECS: u64 = 600;
const POLL_INTERVAL_SECS: u64 = 5;

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
    let cfg = crate::cfg::load(None)?;
    ensure_rclone()?;

    println!("== Cloud Setup (QR flow) ==");
    println!();

    // Setup URL on dashboard, device-specific.
    let setup_url = format!(
        "{}/dashboard/cloud-setup/{}",
        cfg.server_url.trim_end_matches('/'),
        cfg.device_id.0
    );
    println!("Dashboard setup page: {setup_url}");
    println!();

    // Render QR: use qrencode CLI (more compact) if present, fall back
    // to our Rust crate. Print to stdout so port-runner.sh (tty mode)
    // sends it to /dev/tty1 for the user to scan with their phone.
    println!("Scan this with your phone camera:");
    println!();
    let qr_text = crate::ttyui::qr_ansi(&setup_url);
    println!("{qr_text}");
    println!();
    println!("(Or open: {setup_url})");
    println!();

    // Save a PNG and try fbv as a bigger backup display.
    let qr = QrCode::new(setup_url.as_bytes()).context("qr encode")?;
    let png_path = Path::new("/roms/.playora/auth_qr.png");
    if let Err(e) = save_qr_png(&qr, png_path) {
        eprintln!("warn: could not write QR PNG: {e}");
    }

    // Spawn rclone in piped mode. It prints the authorize blob and waits
    // for the resulting token on stdin.
    let bin = rclone_bin();
    let cfg_path = rclone_config();
    let mut child = Command::new(&bin)
        .args([
            "config",
            "create",
            REMOTE,
            "drive",
            "scope=drive",
            "config_is_local=false",
            "--config",
        ])
        .arg(&cfg_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn rclone config")?;

    let stdout = child.stdout.take().context("no stdout")?;
    let mut authorize_blob: Option<String> = None;
    let reader = BufReader::new(stdout);

    // Read stdout to find the authorize blob.
    for line in reader.lines().map_while(Result::ok) {
        println!("rclone: {line}");
        if let Some(start) = line.find("rclone authorize") {
            let snippet = &line[start..];
            authorize_blob = Some(snippet.trim().to_string());
            println!();
            println!("AUTHORIZE_CMD: {snippet}");
            break;
        }
    }

    let blob = authorize_blob.ok_or_else(|| {
        anyhow!("could not capture 'rclone authorize' command from rclone output")
    })?;

    // Post the command + setup URL via activity summary so the dashboard
    // can render the QR + step-by-step + the paste form.
    let _ = crate::activity::progress(
        &cfg,
        "Cloud Setup",
        &format!("AUTH_QR_URL:{setup_url} | AUTH_CMD:{blob}"),
    );

    println!();
    println!("Waiting for token from dashboard (timeout: {SETUP_TIMEOUT_SECS}s)...");

    // Poll server every POLL_INTERVAL_SECS for the user-supplied token.
    let start = Instant::now();
    let token = loop {
        if start.elapsed().as_secs() > SETUP_TIMEOUT_SECS {
            let _ = child.kill();
            return Err(anyhow!("timed out waiting for token"));
        }
        match fetch_token(&cfg) {
            Ok(Some(t)) => break t,
            Ok(None) => {}
            Err(e) => eprintln!("poll error: {e}"),
        }
        std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));
    };

    println!();
    println!(
        "Token received from dashboard ({} bytes). Sending to rclone...",
        token.len()
    );

    // Feed token to rclone's stdin and close it.
    if let Some(stdin) = child.stdin.as_mut() {
        writeln!(stdin, "{token}").ok();
    }
    drop(child.stdin.take());

    let status = child.wait().context("wait rclone")?;
    if !status.success() {
        return Err(anyhow!("rclone config exit {:?}", status.code()));
    }

    println!();
    println!("\x1b[1;32m  ✓ AUTHORIZED — Cloud is ready.\x1b[0m");
    println!();
    println!(
        "SUMMARY: Cloud Setup ok — remote '{REMOTE}' configured at {}",
        cfg_path.display()
    );
    Ok(())
}

fn fetch_token(cfg: &AgentConfig) -> Result<Option<String>> {
    let url = format!(
        "{}/api/v1/devices/{}/cloud-auth-token",
        cfg.server_url.trim_end_matches('/'),
        cfg.device_id.0
    );
    let resp = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?
        .get(&url)
        .send()?;
    if resp.status().as_u16() == 204 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json()?;
    Ok(v.get("token")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string()))
}

fn save_qr_png(qr: &QrCode, path: &Path) -> Result<()> {
    // Minimal PBM (Portable BitMap) writer — no image crate dep needed.
    let modules = qr.to_colors();
    let width = qr.width();
    let height = modules.len() / width;
    let mut out = String::new();
    out.push_str("P1\n");
    out.push_str(&format!("{width} {height}\n"));
    for (i, c) in modules.iter().enumerate() {
        let bit = if matches!(c, qrcode::Color::Dark) {
            1
        } else {
            0
        };
        out.push_str(&format!("{bit}"));
        if i % width == width - 1 {
            out.push('\n');
        } else {
            out.push(' ');
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Write as .pbm — even if extension is .png, that's a graceful fallback
    // for systems without an image encoder. The dashboard renders QR from
    // the URL directly via api.qrserver.com so this is informational only.
    let pbm_path = path.with_extension("pbm");
    std::fs::write(&pbm_path, out)?;
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

pub fn cmd_catalog() -> Result<()> {
    let _lock = crate::lockfile::acquire("cloud-catalog")?;
    let cfg = crate::cfg::load(None)?;
    ensure_rclone()?;
    println!("== Cloud Catalog refresh ==");
    let bin = rclone_bin();
    let rc_cfg = rclone_config();
    let remote = format!("{REMOTE}:{REMOTE_ROOT}/roms");
    println!("listing {remote}...");
    let out = Command::new(&bin)
        .args(["lsjson", "--recursive", "--files-only", "--config"])
        .arg(&rc_cfg)
        .arg(&remote)
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("rclone lsjson failed: {err}"));
    }
    let body = String::from_utf8_lossy(&out.stdout).to_string();
    let count = body.matches("\"Path\"").count();
    println!("found {count} files");
    let url = format!(
        "{}/api/v1/devices/{}/cloud-catalog",
        cfg.server_url.trim_end_matches('/'),
        cfg.device_id.0
    );
    let resp = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("server HTTP {}", resp.status()));
    }
    println!();
    println!("SUMMARY: Cloud Catalog ok ({count} entries posted)");
    Ok(())
}

pub fn cmd_download(rel_path: &str) -> Result<()> {
    let _lock = crate::lockfile::acquire("cloud-download")?;
    ensure_rclone()?;
    println!("== Cloud Download ==");
    let bin = rclone_bin();
    let rc_cfg = rclone_config();
    let src = format!("{REMOTE}:{REMOTE_ROOT}/roms/{rel_path}");
    let dst_dir = Path::new("/roms").join(
        Path::new(rel_path)
            .parent()
            .unwrap_or_else(|| Path::new("")),
    );
    std::fs::create_dir_all(&dst_dir).ok();
    println!("--> {src}");
    println!("    {}/", dst_dir.display());
    let st = Command::new(&bin)
        .args(["copy", "--config"])
        .arg(&rc_cfg)
        .arg(&src)
        .arg(&dst_dir)
        .args(["--stats=5s", "--stats-one-line", "--transfers=2"])
        .status()?;
    if !st.success() {
        return Err(anyhow!("rclone copy failed"));
    }
    println!();
    println!("SUMMARY: Cloud Download ok — {rel_path}");
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
