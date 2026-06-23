//! fix-exit-game v2 — audits and patches retroarch.cfg (and retroarch32.cfg)
//! settings that are known to cause the Select+Start exit-game freeze on R36S
//! and dArkOSRE clones. Default mode is dry-run; `--apply` writes changes,
//! always creating a timestamped backup first. `--restore` rolls back to the
//! most recent backup.
//!
//! Every patched key carries a justification (`why`) so users and the
//! dashboard see *why* a change is recommended rather than blind overwrites.

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const SETTINGS: &[(&str, &str, &str)] = &[
    (
        "video_threaded",
        "false",
        "Threaded video deadlocks the framebuffer on exit on RK3326 / R36S.",
    ),
    (
        "pause_nonactive",
        "false",
        "ES focus toggling pauses the core; the freeze happens during that pause.",
    ),
    (
        "audio_driver",
        "alsathread",
        "alsathread releases the audio device on exit; 'pulse' keeps it locked.",
    ),
    (
        "video_driver",
        "gl",
        "GL releases the DRM master cleanly; some KMS combos do not.",
    ),
    (
        "quit_press_twice",
        "true",
        "Matches the dArkOSRE Select+Start×2 confirm pattern.",
    ),
    (
        "input_quit_gamepad_combo",
        "4",
        "Combo 4 = L1+R1+Select+Start (RetroArch convention).",
    ),
    (
        "video_fullscreen",
        "true",
        "Windowed mode can leave a dangling X window after quit.",
    ),
    (
        "video_disable_composition",
        "true",
        "Compositor doesn't exist on dArkOSRE; prevents wait-on-handoff hang.",
    ),
    (
        "input_keyboard_gamepad_enable",
        "true",
        "Ensures gptokeyb signals reach RetroArch consistently.",
    ),
];

pub fn cmd_fix_exit_game(apply: bool) -> Result<()> {
    // Back-compat entry: `apply` true → --apply path, false → dry-run.
    cmd_fix_exit_game_v2(apply, false)
}

pub fn cmd_fix_exit_game_v2(apply: bool, restore: bool) -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header(if restore {
        "Fix Exit-Game — Restore"
    } else if apply {
        "Fix Exit-Game — Apply"
    } else {
        "Fix Exit-Game — Dry Run"
    });

    let cfgs = find_all_retroarch_cfgs();
    if cfgs.is_empty() {
        ttyui::row("retroarch.cfg", "no files found", Status::Fail);
        println!("\nSUMMARY: nothing to do — no retroarch.cfg located.");
        return Ok(());
    }

    for cfg_path in &cfgs {
        ttyui::section(&format!("Target: {}", cfg_path.display()));

        if restore {
            match restore_latest_backup(cfg_path) {
                Ok(b) => ttyui::ok(&format!("restored from {}", b.display())),
                Err(e) => ttyui::fail(&format!("restore failed: {e}")),
            }
            continue;
        }

        let content = match std::fs::read_to_string(cfg_path) {
            Ok(c) => c,
            Err(e) => {
                ttyui::fail(&format!("read failed: {e}"));
                continue;
            }
        };
        let current = parse_settings(&content);
        let overrides = list_overrides_for(cfg_path);
        if !overrides.is_empty() {
            ttyui::row(
                "overrides",
                &format!(
                    "{} per-core/per-game cfg(s) — may mask global fix",
                    overrides.len()
                ),
                Status::Warn,
            );
            for o in overrides.iter().take(5) {
                ttyui::note(&format!("  {}", o.display()));
            }
        }

        let mut to_change = Vec::new();
        for (key, want, why) in SETTINGS {
            let cur = current.get(*key).cloned();
            let ok = cur.as_deref() == Some(*want);
            let display = cur.clone().unwrap_or_else(|| "(unset)".into());
            ttyui::row(
                key,
                &format!("{display} → {want}"),
                if ok { Status::Ok } else { Status::Warn },
            );
            ttyui::note(why);
            if !ok {
                to_change.push((*key, *want, *why));
            }
        }

        if to_change.is_empty() {
            ttyui::ok("all settings already optimal");
            continue;
        }

        if !apply {
            ttyui::note(&format!("dry-run: {} key(s) would change", to_change.len()));
            ttyui::note("Re-run with `--apply` to write changes (backup is automatic).");
            continue;
        }

        let backup = make_backup(cfg_path).context("make backup")?;
        ttyui::row("backup", &backup.display().to_string(), Status::Ok);
        let pairs: Vec<(&str, &str)> = to_change.iter().map(|(k, v, _)| (*k, *v)).collect();
        let new_content = apply_patches(&content, &pairs);
        if let Err(e) = atomic_write(cfg_path, new_content.as_bytes()) {
            ttyui::fail(&format!("write failed: {e}"));
            continue;
        }
        write_change_log(cfg_path, &backup, &to_change);
        ttyui::ok(&format!("wrote {} setting(s)", to_change.len()));
    }

    println!();
    if restore {
        println!("SUMMARY: Fix Exit-Game restore complete.");
    } else if apply {
        println!("SUMMARY: Fix Exit-Game applied — relaunch RetroArch or reboot.");
    } else {
        println!("SUMMARY: Fix Exit-Game dry-run — re-run with --apply to write changes.");
    }
    Ok(())
}

/// Legacy single-file finder kept for callers (e.g. tests.rs).
pub fn find_retroarch_cfg() -> Option<PathBuf> {
    find_all_retroarch_cfgs().into_iter().next()
}

fn find_all_retroarch_cfgs() -> Vec<PathBuf> {
    let mut all = Vec::new();
    let candidates = [
        "/home/ark/.config/retroarch/retroarch.cfg",
        "/home/ark/.config/retroarch32/retroarch.cfg",
        "/root/.config/retroarch/retroarch.cfg",
        "/opt/retroarch/.config/retroarch/retroarch.cfg",
        "/home/pi/.config/retroarch/retroarch.cfg",
        "/userdata/system/configs/retroarch/retroarch.cfg",
    ];
    for c in &candidates {
        let p = Path::new(c);
        if p.is_file() {
            all.push(p.to_path_buf());
        }
    }
    if all.is_empty() {
        let Ok(out) = Command::new("find")
            .args([
                "/home",
                "/root",
                "/opt",
                "/userdata",
                "-maxdepth",
                "6",
                "-name",
                "retroarch.cfg",
                "-type",
                "f",
                "-not",
                "-path",
                "*/cores/*",
            ])
            .output()
        else {
            return all;
        };
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let p = PathBuf::from(line.trim());
            if p.is_file() {
                all.push(p);
            }
        }
    }
    all
}

fn list_overrides_for(cfg_path: &Path) -> Vec<PathBuf> {
    let base = cfg_path
        .parent()
        .map(|p| p.join("config"))
        .unwrap_or_default();
    if !base.is_dir() {
        return vec![];
    }
    let mut out = Vec::new();
    let Ok(found) = Command::new("find")
        .args([
            base.to_str().unwrap_or(""),
            "-maxdepth",
            "6",
            "-type",
            "f",
            "-name",
            "*.cfg",
        ])
        .output()
    else {
        return out;
    };
    for line in String::from_utf8_lossy(&found.stdout).lines() {
        out.push(PathBuf::from(line.trim()));
    }
    out
}

fn make_backup(cfg_path: &Path) -> Result<PathBuf> {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let mut name = cfg_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "retroarch.cfg".into());
    name.push_str(&format!(".playora-bak.{stamp}"));
    let backup = cfg_path.with_file_name(name);
    std::fs::copy(cfg_path, &backup)?;
    Ok(backup)
}

fn restore_latest_backup(cfg_path: &Path) -> Result<PathBuf> {
    let dir = cfg_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent dir"))?;
    let stem = cfg_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "retroarch.cfg".into());
    let prefix = format!("{stem}.playora-bak.");
    let mut backups: Vec<PathBuf> = dir
        .read_dir()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .map(|n| n.starts_with(&prefix))
                .unwrap_or(false)
        })
        .collect();
    backups.sort();
    let latest = backups
        .pop()
        .ok_or_else(|| anyhow::anyhow!("no backup found in {}", dir.display()))?;
    std::fs::copy(&latest, cfg_path)?;
    Ok(latest)
}

fn write_change_log(cfg_path: &Path, backup: &Path, changes: &[(&str, &str, &str)]) {
    let log_dir = Path::new("/roms/.playora/logs");
    let _ = std::fs::create_dir_all(log_dir);
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let log = log_dir.join(format!("fix-exit-{stamp}.log"));
    let mut body = String::new();
    body.push_str(&format!("# fix-exit-game patch log {stamp}\n"));
    body.push_str(&format!("target: {}\n", cfg_path.display()));
    body.push_str(&format!("backup: {}\n", backup.display()));
    for (k, v, why) in changes {
        body.push_str(&format!("\n[{k}] -> {v}\n  why: {why}\n"));
    }
    let _ = std::fs::write(log, body);
}

pub fn parse_settings(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(eq) = t.find('=') {
            let key = t[..eq].trim().to_string();
            let val = t[eq + 1..].trim().trim_matches('"').to_string();
            map.insert(key, val);
        }
    }
    map
}

pub fn apply_patches(content: &str, patches: &[(&str, &str)]) -> String {
    let want: HashMap<&str, &str> = patches.iter().copied().collect();
    let mut applied: HashSet<&str> = HashSet::new();
    let mut out = String::new();
    for line in content.lines() {
        let t = line.trim_start();
        let mut matched = false;
        for (key, val) in patches.iter() {
            if t.starts_with(key) {
                let next = t[key.len()..].trim_start();
                if next.starts_with('=') {
                    out.push_str(&format!("{key} = \"{val}\"\n"));
                    applied.insert(key);
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            out.push_str(line);
            out.push('\n');
        }
    }
    for (key, _) in want.iter() {
        if !applied.contains(key) {
            out.push_str(&format!("{key} = \"{}\"\n", want[key]));
        }
    }
    out
}

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("cfg.playora-tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_new_keys() {
        let cfg = "video_smooth = \"true\"\n";
        let out = apply_patches(
            cfg,
            &[("video_threaded", "false"), ("pause_nonactive", "false")],
        );
        assert!(out.contains("video_smooth = \"true\""));
        assert!(out.contains("video_threaded = \"false\""));
        assert!(out.contains("pause_nonactive = \"false\""));
    }

    #[test]
    fn replaces_existing_value() {
        let cfg = "video_threaded = \"true\"\nfoo = \"bar\"\n";
        let out = apply_patches(cfg, &[("video_threaded", "false")]);
        assert!(out.contains("video_threaded = \"false\""));
        assert!(!out.contains("video_threaded = \"true\""));
        assert!(out.contains("foo = \"bar\""));
    }

    #[test]
    fn parses_quoted_values() {
        let cfg = "video_threaded = \"true\"\npause_nonactive=false\n";
        let m = parse_settings(cfg);
        assert_eq!(m.get("video_threaded").map(String::as_str), Some("true"));
        assert_eq!(m.get("pause_nonactive").map(String::as_str), Some("false"));
    }

    #[test]
    fn patch_is_idempotent() {
        let cfg = "video_threaded = \"false\"\n";
        let out = apply_patches(cfg, &[("video_threaded", "false")]);
        // After patch, value remains and key still present exactly once.
        assert_eq!(out.matches("video_threaded").count(), 1);
        assert!(out.contains("video_threaded = \"false\""));
    }

    #[test]
    fn backup_then_restore_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path().join("retroarch.cfg");
        std::fs::write(&cfg, b"video_threaded = \"true\"\n").unwrap();
        let backup = make_backup(&cfg).unwrap();
        std::fs::write(&cfg, b"video_threaded = \"false\"\n").unwrap();
        let restored = restore_latest_backup(&cfg).unwrap();
        assert_eq!(restored, backup);
        let back = std::fs::read_to_string(&cfg).unwrap();
        assert!(back.contains("\"true\""));
    }
}
