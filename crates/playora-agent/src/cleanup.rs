//! Cleanup: delete ROMs listed in `/roms/.playora/delete_queue.txt`
//! (one absolute path per line, blank/comment lines ignored), or pull
//! pending delete requests from the dashboard server and apply them.

use anyhow::{Context, Result};
use playora_common::AgentConfig;
use std::path::{Path, PathBuf};

const QUEUE_PATH: &str = "/roms/.playora/delete_queue.txt";

pub fn cmd_cleanup(cfg: AgentConfig, apply_server_queue: bool) -> Result<()> {
    let _lock = crate::lockfile::acquire("cleanup")?;
    let mut deleted = 0u32;
    let mut errors = 0u32;
    let mut skipped = 0u32;

    // 1) Local queue file
    let paths = read_queue(Path::new(QUEUE_PATH));
    println!("local queue: {} entries", paths.len());
    for p in &paths {
        match delete_one(p) {
            Ok(true) => deleted += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                eprintln!("  fail {p}: {e}");
                errors += 1;
            }
        }
    }
    // Empty queue file on success.
    if !paths.is_empty() {
        std::fs::write(QUEUE_PATH, "").ok();
    }

    // 2) Server queue
    if apply_server_queue {
        let server_paths = match pull_server_queue(&cfg) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("server queue fetch fail: {e}");
                vec![]
            }
        };
        println!("server queue: {} entries", server_paths.len());
        for p in &server_paths {
            let result = delete_one(p);
            let (ok, err_msg) = match &result {
                Ok(true) => (true, String::new()),
                Ok(false) => (true, "not found".into()),
                Err(e) => (false, e.to_string()),
            };
            ack_server(&cfg, p, ok, &err_msg).ok();
            match result {
                Ok(true) => deleted += 1,
                Ok(false) => skipped += 1,
                Err(_) => errors += 1,
            }
        }
    }

    println!();
    println!("== Cleanup detail ==");
    println!("deleted: {deleted}");
    println!("skipped (missing): {skipped}");
    println!("errors: {errors}");
    println!();
    println!("SUMMARY: {deleted} ROMs deleted, {skipped} missing, {errors} errors");
    Ok(())
}

fn read_queue(path: &Path) -> Vec<String> {
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    s.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

fn delete_one(path: &str) -> Result<bool> {
    let p = PathBuf::from(path);
    if !p.exists() {
        println!("  skip {path} (missing)");
        return Ok(false);
    }
    // Refuse to delete outside /roms/ for safety.
    if !path.starts_with("/roms/") {
        anyhow::bail!("refusing to delete outside /roms/: {path}");
    }
    if p.is_dir() {
        std::fs::remove_dir_all(&p)?;
    } else {
        std::fs::remove_file(&p)?;
    }
    println!("  deleted {path}");
    Ok(true)
}

fn pull_server_queue(cfg: &AgentConfig) -> Result<Vec<String>> {
    let url = format!(
        "{}/api/v1/devices/{}/delete-pending",
        cfg.server_url.trim_end_matches('/'),
        cfg.device_id.0
    );
    let v: serde_json::Value = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .get(&url)
        .send()
        .context("GET delete-pending")?
        .json()?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .filter_map(|x| {
            x.get("rom_path")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .collect())
}

fn ack_server(cfg: &AgentConfig, rom_path: &str, ok: bool, err: &str) -> Result<()> {
    let url = format!(
        "{}/api/v1/devices/{}/delete-ack",
        cfg.server_url.trim_end_matches('/'),
        cfg.device_id.0
    );
    let body = serde_json::json!({
        "rom_path": rom_path,
        "status": if ok { "ok" } else { "fail" },
        "error": err,
    });
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .post(&url)
        .json(&body)
        .send()?;
    Ok(())
}
