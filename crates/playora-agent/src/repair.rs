//! `repair-rom-layout` — moves ROMs from `_inbox` or wrong system folder
//! into the correct `/roms/<system>/` based on file extension. Never
//! overwrites existing files (renames with `.dup-N` on collision). Default
//! dry-run; `--apply` writes.

use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Plan {
    from: PathBuf,
    to: PathBuf,
    reason: String,
}

pub fn cmd_repair(cfg: AgentConfig, apply: bool) -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header(if apply {
        "Repair ROM Layout — Apply"
    } else {
        "Repair ROM Layout — Dry Run"
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

    // Build extension -> folder map from SystemSpec table.
    let mut ext_to_folder: BTreeMap<String, &'static str> = BTreeMap::new();
    for s in playora_common::systems::SYSTEMS {
        for e in s.extensions {
            // Skip ambiguous archive extensions — they need user intent
            if matches!(*e, "zip" | "7z" | "rar") {
                continue;
            }
            // Avoid overwriting; first SystemSpec wins.
            ext_to_folder
                .entry(e.to_ascii_lowercase())
                .or_insert(s.folder);
        }
    }

    let mut plan: Vec<Plan> = Vec::new();

    // Pass 1: /roms/_inbox -> system folder by extension
    let inbox = root.join("_inbox");
    if inbox.is_dir() {
        for f in walkdir::WalkDir::new(&inbox)
            .max_depth(3)
            .into_iter()
            .flatten()
        {
            if !f.file_type().is_file() {
                continue;
            }
            let p = f.path();
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if let Some(folder) = ext_to_folder.get(&ext) {
                let dest_dir = root.join(folder);
                let dest = dest_dir.join(p.file_name().unwrap_or_default());
                plan.push(Plan {
                    from: p.to_path_buf(),
                    to: dest,
                    reason: format!("inbox -> {folder} (by .{ext})"),
                });
            }
        }
    }

    // Pass 2: ROM in wrong system folder. Only consider extensions that
    // uniquely map to one folder; skip ambiguous ones.
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let sys_dir = entry.path();
        if !sys_dir.is_dir() {
            continue;
        }
        let sys_name = sys_dir
            .file_name()
            .map(|f| f.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if sys_name.starts_with('.')
            || matches!(
                sys_name.as_str(),
                "_inbox" | "bios" | "savestates" | "themes" | "ports" | "tools"
            )
        {
            continue;
        }
        for f in walkdir::WalkDir::new(&sys_dir)
            .max_depth(2)
            .into_iter()
            .flatten()
        {
            if !f.file_type().is_file() {
                continue;
            }
            let p = f.path();
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let Some(want_folder) = ext_to_folder.get(&ext) else {
                continue;
            };
            if want_folder.eq_ignore_ascii_case(&sys_name) {
                continue;
            }
            let dest = root
                .join(want_folder)
                .join(p.file_name().unwrap_or_default());
            plan.push(Plan {
                from: p.to_path_buf(),
                to: dest,
                reason: format!("{sys_name} -> {want_folder} (by .{ext})"),
            });
        }
    }

    ttyui::section("Plan");
    if plan.is_empty() {
        ttyui::ok("no layout fixes needed");
        return Ok(());
    }
    for p in plan.iter().take(40) {
        ttyui::row(
            &p.reason,
            &format!("{} -> {}", p.from.display(), p.to.display()),
            Status::Info,
        );
    }
    if plan.len() > 40 {
        ttyui::note(&format!("(+{} more)", plan.len() - 40));
    }

    if !apply {
        println!();
        println!(
            "SUMMARY: repair-rom-layout dry-run — {} move(s) planned. Re-run with --apply.",
            plan.len()
        );
        return Ok(());
    }

    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let log_dir = Path::new("/roms/.playora/logs");
    let _ = std::fs::create_dir_all(log_dir);
    let log = log_dir.join(format!("repair-{stamp}.log"));
    let mut body = String::from("# repair-rom-layout log\n");

    let mut moved = 0u32;
    let mut failed = 0u32;
    for step in &plan {
        if let Some(parent) = step.to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let final_to = collision_safe(&step.to);
        match std::fs::rename(&step.from, &final_to) {
            Ok(_) => {
                moved += 1;
                body.push_str(&format!(
                    "moved: {} -> {} ({})\n",
                    step.from.display(),
                    final_to.display(),
                    step.reason
                ));
            }
            Err(e) => {
                failed += 1;
                body.push_str(&format!(
                    "failed: {} -> {} ({e})\n",
                    step.from.display(),
                    final_to.display()
                ));
            }
        }
    }
    let _ = std::fs::write(&log, body);

    ttyui::ok(&format!("moved {moved} · failed {failed}"));
    ttyui::row("log", &log.display().to_string(), Status::Info);

    if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
        let _ = crate::db::enqueue(
            &conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: Utc::now(),
                payload: EventPayload::ScriptFinished(ScriptFinished {
                    script: "repair-rom-layout".into(),
                    exit_code: if failed == 0 { 0 } else { 1 },
                    duration_seconds: 0,
                    stdout_tail: Some(format!("moved {moved}, failed {failed}")),
                    ended_at: Utc::now(),
                }),
            },
        );
    }

    println!();
    println!(
        "SUMMARY: repair-rom-layout — moved {moved}, failed {failed}. See {}",
        log.display()
    );
    Ok(())
}

fn collision_safe(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let parent = dest.parent().unwrap_or(Path::new("."));
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = dest
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..1000 {
        let candidate = parent.join(format!("{stem}.dup-{i}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dest.to_path_buf()
}
