use anyhow::Result;
use std::path::{Path, PathBuf};

const SETTINGS: &[(&str, &str, &str)] = &[
    (
        "video_threaded",
        "false",
        "Avoids the threaded-video deadlock that hangs the framebuffer on exit (R36S / RK3326)",
    ),
    (
        "pause_nonactive",
        "false",
        "Don't pause the core when the window loses focus; ES focus toggling triggers the freeze otherwise",
    ),
    (
        "audio_driver",
        "alsathread",
        "alsathread releases the audio device on exit; default 'pulse' can leave it locked",
    ),
    (
        "video_driver",
        "gl",
        "Stick to GL (KMS+GBM combos sometimes hold the DRM master after quit)",
    ),
    (
        "quit_press_twice",
        "true",
        "Match the dArkOSRE Select+Start×2 combo; if false the first press exits immediately",
    ),
    (
        "input_quit_gamepad_combo",
        "4",
        "4 = L1+R1+Select+Start (RetroArch quit combo). Belt-and-suspenders with quit_press_twice",
    ),
    (
        "video_fullscreen",
        "true",
        "Forced fullscreen — windowed mode can leave dangling X windows on exit",
    ),
    (
        "video_disable_composition",
        "true",
        "Skip compositor handoff (compositor doesn't exist on dArkOSRE, prevents wait-on-handoff hang)",
    ),
    (
        "input_keyboard_gamepad_enable",
        "true",
        "Ensures gptokeyb signals reach RetroArch consistently",
    ),
];

pub fn cmd_fix_exit_game(apply: bool) -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header("Fix Exit-Game Freeze");

    ttyui::section("Locating retroarch.cfg");
    let path = match find_retroarch_cfg() {
        Some(p) => {
            ttyui::row("path", &p.display().to_string(), Status::Ok);
            p
        }
        None => {
            ttyui::row("path", "not found", Status::Fail);
            println!();
            println!("SUMMARY: Fix Exit-Game skipped — retroarch.cfg not on this device.");
            return Ok(());
        }
    };

    let content = std::fs::read_to_string(&path)?;
    let current = parse_settings(&content);

    ttyui::section("Settings audit");
    let mut to_change = Vec::new();
    for (key, want, why) in SETTINGS {
        let cur = current.get(*key).cloned();
        let ok = cur.as_deref() == Some(*want);
        let st = if ok { Status::Ok } else { Status::Warn };
        let display = cur.clone().unwrap_or_else(|| "(unset)".into());
        ttyui::row(key, &format!("{display} → want {want}"), st);
        ttyui::note(why);
        if !ok {
            to_change.push((*key, *want));
        }
    }

    if to_change.is_empty() {
        println!();
        println!("\x1b[1;32m  All settings already optimal — exit-game should work.\x1b[0m");
        println!("SUMMARY: Fix Exit-Game — nothing to change.");
        return Ok(());
    }

    if !apply {
        ttyui::section("How to apply");
        ttyui::note("Re-run with `playora-agent fix-exit-game --apply` (or use the dashboard Update Agent + click Apply on next Doctor run).");
        println!();
        println!(
            "SUMMARY: Fix Exit-Game — {} settings would be changed (dry run).",
            to_change.len()
        );
        return Ok(());
    }

    ttyui::section("Applying patches");
    let backup = path.with_extension("cfg.playora-bak");
    if !backup.exists() {
        std::fs::copy(&path, &backup)?;
        ttyui::row("backup", &backup.display().to_string(), Status::Ok);
    } else {
        ttyui::row("backup", "already exists, kept", Status::Info);
    }
    let new_content = apply_patches(&content, &to_change);
    std::fs::write(&path, &new_content)?;
    ttyui::ok(&format!("wrote {} settings", to_change.len()));

    ttyui::section("Next step");
    ttyui::note("Reboot the console (or relaunch RetroArch) so the new settings take effect.");
    println!();
    println!(
        "SUMMARY: Fix Exit-Game — patched {} settings. Reboot recommended.",
        to_change.len()
    );
    Ok(())
}

pub fn find_retroarch_cfg() -> Option<PathBuf> {
    let candidates = [
        "/home/ark/.config/retroarch/retroarch.cfg",
        "/root/.config/retroarch/retroarch.cfg",
        "/opt/retroarch/.config/retroarch/retroarch.cfg",
        "/home/pi/.config/retroarch/retroarch.cfg",
        "/userdata/system/configs/retroarch/retroarch.cfg",
    ];
    for c in &candidates {
        let p = Path::new(c);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    let out = std::process::Command::new("find")
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
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| PathBuf::from(s.trim()))
        .find(|p| p.is_file())
}

pub fn parse_settings(content: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
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
    let want: std::collections::HashMap<&str, &str> = patches.iter().copied().collect();
    let mut applied: std::collections::HashSet<&str> = std::collections::HashSet::new();
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
}
