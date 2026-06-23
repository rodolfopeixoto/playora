//! `clean-roms` — removes certified-safe junk: macOS metadata
//! (`.DS_Store`, `._*`, `__MACOSX/`), `thumbs.db`. Optionally fixes CRLF
//! line endings in `.sh` scripts under `/roms/ports/` and ensures `+x`.
//!
//! Never touches ROMs, BIOS, saves, or gamelist files. Defaults to dry-run.

use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::path::{Path, PathBuf};

pub fn cmd_clean(cfg: AgentConfig, apply: bool) -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header(if apply {
        "Clean ROMs — Apply"
    } else {
        "Clean ROMs — Dry Run"
    });

    let roms_root = cfg
        .rom_paths
        .first()
        .cloned()
        .unwrap_or_else(|| "/roms".into());
    let root = PathBuf::from(&roms_root);
    if !root.is_dir() {
        ttyui::row("roms_root", "missing", Status::Fail);
        return Ok(());
    }

    let mut to_remove: Vec<PathBuf> = Vec::new();
    let mut crlf_scripts: Vec<PathBuf> = Vec::new();
    let mut nonexec_scripts: Vec<PathBuf> = Vec::new();

    for f in walkdir::WalkDir::new(&root)
        .max_depth(8)
        .into_iter()
        .flatten()
    {
        let p = f.path().to_path_buf();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Junk
        if name == ".DS_Store" || name == "thumbs.db" || name.starts_with("._") {
            to_remove.push(p);
            continue;
        }
        if p.is_dir() && name == "__MACOSX" {
            to_remove.push(p);
            continue;
        }
        // Script hygiene under /roms/ports only
        if p.is_file()
            && p.extension().map(|e| e == "sh").unwrap_or(false)
            && p.starts_with(root.join("ports"))
        {
            let Ok(content) = std::fs::read(&p) else {
                continue;
            };
            if content.contains(&b'\r') {
                crlf_scripts.push(p.clone());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(md) = std::fs::metadata(&p) {
                    let mode = md.permissions().mode();
                    if mode & 0o111 == 0 {
                        nonexec_scripts.push(p);
                    }
                }
            }
        }
    }

    ttyui::section("Findings");
    ttyui::row(
        "junk files/dirs",
        &to_remove.len().to_string(),
        if to_remove.is_empty() {
            Status::Ok
        } else {
            Status::Warn
        },
    );
    ttyui::row(
        "CRLF scripts",
        &crlf_scripts.len().to_string(),
        if crlf_scripts.is_empty() {
            Status::Ok
        } else {
            Status::Warn
        },
    );
    ttyui::row(
        "non-exec scripts",
        &nonexec_scripts.len().to_string(),
        if nonexec_scripts.is_empty() {
            Status::Ok
        } else {
            Status::Warn
        },
    );

    for p in to_remove
        .iter()
        .chain(crlf_scripts.iter())
        .chain(nonexec_scripts.iter())
        .take(20)
    {
        ttyui::note(&format!("  {}", p.display()));
    }

    if !apply {
        println!();
        println!("SUMMARY: clean-roms dry-run — re-run with --apply to remove/fix.");
        return Ok(());
    }

    // Backup junk list before deletion so user can verify.
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let log_dir = Path::new("/roms/.playora/logs");
    let _ = std::fs::create_dir_all(log_dir);
    let log = log_dir.join(format!("clean-{stamp}.log"));
    let mut body = String::from("# clean-roms log\n");

    let mut removed = 0u32;
    for p in &to_remove {
        let r = if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        };
        if r.is_ok() {
            removed += 1;
            body.push_str(&format!("removed: {}\n", p.display()));
        } else {
            body.push_str(&format!("kept (err): {}\n", p.display()));
        }
    }
    let mut fixed_crlf = 0u32;
    for p in &crlf_scripts {
        if let Ok(content) = std::fs::read(p) {
            let cleaned: Vec<u8> = content.into_iter().filter(|b| *b != b'\r').collect();
            if std::fs::write(p, &cleaned).is_ok() {
                fixed_crlf += 1;
                body.push_str(&format!("crlf-fixed: {}\n", p.display()));
            }
        }
    }
    let mut chmod_x = 0u32;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in &nonexec_scripts {
            if let Ok(md) = std::fs::metadata(p) {
                let mut perm = md.permissions();
                let mode = perm.mode();
                perm.set_mode(mode | 0o111);
                if std::fs::set_permissions(p, perm).is_ok() {
                    chmod_x += 1;
                    body.push_str(&format!("chmod +x: {}\n", p.display()));
                }
            }
        }
    }
    let _ = std::fs::write(&log, body);

    crate::ttyui::ok(&format!(
        "removed {removed} junk · fixed {fixed_crlf} CRLF · +x {chmod_x}"
    ));
    crate::ttyui::row(
        "log",
        &log.display().to_string(),
        crate::ttyui::Status::Info,
    );

    // Best-effort emit a ScriptFinished so dashboard sees it
    if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
        let _ = crate::db::enqueue(
            &conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: Utc::now(),
                payload: EventPayload::ScriptFinished(ScriptFinished {
                    script: "clean-roms".into(),
                    exit_code: 0,
                    duration_seconds: 0,
                    stdout_tail: Some(format!(
                        "removed {removed} junk, fixed {fixed_crlf} CRLF, +x {chmod_x}"
                    )),
                    ended_at: Utc::now(),
                }),
            },
        );
    }

    println!();
    println!("SUMMARY: clean-roms applied — see {}", log.display());
    Ok(())
}
