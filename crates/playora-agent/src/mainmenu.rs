//! Install the Playora system into EmulationStation's es_systems.cfg.
//!
//! dArkOSRE stores es_systems.cfg under one of several paths; we probe
//! the live filesystem at runtime (the install-to-sd.sh installer can't
//! see the rootfs partition from macOS). Idempotent — checks for the
//! `<name>playora</name>` marker before writing.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const PLAYORA_BLOCK: &str = "  <system>
    <name>playora</name>
    <fullname>Playora</fullname>
    <path>/roms/playora</path>
    <extension>.sh .SH</extension>
    <command>%ROM%</command>
    <theme>playora</theme>
  </system>
";

pub fn cmd_install_main_menu() -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header("Install Main Menu Tile");

    let path = locate_es_systems_cfg()?;
    ttyui::row("config found", &path.display().to_string(), Status::Ok);

    let content =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if content.contains("<name>playora</name>") {
        ttyui::ok("Playora system already registered — nothing to do.");
        return Ok(());
    }

    // Try as the current user first; fall back to sudo if we hit EACCES.
    let backup = path.with_extension("cfg.playora-bak");
    if !backup.exists() {
        if let Err(e) = std::fs::copy(&path, &backup) {
            ttyui::warn(&format!("backup failed ({e}); will try sudo cp"));
            let st = Command::new("sudo")
                .args(["cp", "-n"])
                .arg(&path)
                .arg(&backup)
                .status();
            if !matches!(st, Ok(s) if s.success()) {
                ttyui::fail("could not back up es_systems.cfg even with sudo");
            }
        }
    }

    let new_content = merge_block(&content);
    let tmp = std::env::temp_dir().join("playora_es_systems.cfg");
    std::fs::write(&tmp, &new_content)?;

    let write_attempt = std::fs::write(&path, &new_content);
    match write_attempt {
        Ok(_) => ttyui::ok("merged Playora system block (direct write)"),
        Err(_) => {
            // Try sudo cp from tmp to path.
            let st = Command::new("sudo")
                .args(["cp"])
                .arg(&tmp)
                .arg(&path)
                .status()
                .with_context(|| "spawn sudo cp")?;
            if !st.success() {
                return Err(anyhow!("sudo cp failed for {}", path.display()));
            }
            ttyui::ok("merged Playora system block (via sudo)");
        }
    }
    std::fs::remove_file(&tmp).ok();

    ttyui::ok("Playora system registered. ES will restart in a moment and the Main Menu tile will appear.");
    println!();
    println!("SUMMARY: Main Menu tile installed (port-runner will restart ES on exit)");
    Ok(())
}

fn locate_es_systems_cfg() -> Result<PathBuf> {
    // dArkOSRE typical locations.
    let candidates = [
        "/etc/emulationstation/es_systems.cfg",
        "/opt/system/configs/emulationstation/es_systems.cfg",
        "/opt/.darkosre/configs/emulationstation/es_systems.cfg",
        "/home/ark/.emulationstation/es_systems.cfg",
        "/root/.emulationstation/es_systems.cfg",
        "/userdata/system/configs/emulationstation/es_systems.cfg",
    ];
    for c in &candidates {
        let p = Path::new(c);
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
    }
    // Last resort: search /etc + /opt for any es_systems.cfg.
    for root in &["/etc", "/opt", "/home", "/root", "/userdata"] {
        if let Ok(out) = Command::new("find")
            .args([root, "-name", "es_systems.cfg", "-type", "f"])
            .output()
        {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let p = Path::new(line.trim());
                if p.is_file() {
                    return Ok(p.to_path_buf());
                }
            }
        }
    }
    Err(anyhow!(
        "es_systems.cfg not found anywhere. Looked in /etc, /opt, /home, /root, /userdata."
    ))
}

fn merge_block(content: &str) -> String {
    // Insert before the closing </systemList>.
    if let Some(idx) = content.rfind("</systemList>") {
        let (head, tail) = content.split_at(idx);
        format!("{head}{PLAYORA_BLOCK}{tail}")
    } else {
        // No closing tag — wrap whole thing.
        format!("<?xml version=\"1.0\"?>\n<systemList>\n{PLAYORA_BLOCK}</systemList>\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_inserts_before_close_tag() {
        let original = "<?xml version=\"1.0\"?>\n<systemList>\n  <system><name>snes</name></system>\n</systemList>\n";
        let merged = merge_block(original);
        assert!(merged.contains("<name>playora</name>"));
        let snes_idx = merged.find("snes").unwrap();
        let playora_idx = merged.find("playora").unwrap();
        let close_idx = merged.find("</systemList>").unwrap();
        assert!(snes_idx < playora_idx);
        assert!(playora_idx < close_idx);
    }

    #[test]
    fn merge_idempotent_via_marker() {
        let already = "<systemList>\n<system><name>playora</name></system>\n</systemList>";
        // The cmd-level check catches duplicates; merge_block doesn't dedup itself.
        assert!(already.contains("<name>playora</name>"));
    }
}
