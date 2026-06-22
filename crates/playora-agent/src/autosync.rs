use anyhow::Result;
use std::process::Command;

const UNIT_PATH: &str = "/etc/systemd/system/playora-agent.service";
const UNIT_BODY: &str = r#"[Unit]
Description=Playora agent
After=network-online.target

[Service]
ExecStart=/roms/.playora/playora-agent --config /roms/.playora/agent.toml run
Restart=on-failure
RestartSec=10
StandardOutput=append:/roms/.playora/logs/run.log
StandardError=append:/roms/.playora/logs/run.log

[Install]
WantedBy=multi-user.target
"#;

pub fn cmd_enable() -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header("Autosync Enable");

    ttyui::section("Detecting init system");
    if Command::new("systemctl").arg("--version").status().is_err() {
        ttyui::row("systemd", "not found", Status::Warn);
        return spawn_background_fallback();
    }
    ttyui::row("systemd", "present", Status::Ok);

    ttyui::section("Writing unit file");
    let write = Command::new("sudo")
        .args(["tee", UNIT_PATH])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn();
    match write {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(UNIT_BODY.as_bytes()).ok();
            }
            let s = child.wait()?;
            if s.success() {
                ttyui::row("unit", UNIT_PATH, Status::Ok);
            } else {
                ttyui::row("unit", "sudo tee failed", Status::Fail);
                return Ok(());
            }
        }
        Err(e) => {
            ttyui::row("unit", &format!("spawn fail: {e}"), Status::Fail);
            return Ok(());
        }
    }

    ttyui::section("Activating service");
    let _ = Command::new("sudo")
        .args(["systemctl", "daemon-reload"])
        .status();
    let st = Command::new("sudo")
        .args(["systemctl", "enable", "--now", "playora-agent.service"])
        .status();
    match st {
        Ok(s) if s.success() => {
            ttyui::ok("playora-agent.service enabled + started");
            print_status();
        }
        Ok(s) => ttyui::row(
            "enable --now",
            &format!("exit {:?}", s.code()),
            Status::Fail,
        ),
        Err(e) => ttyui::row("enable --now", &format!("error: {e}"), Status::Fail),
    }

    println!();
    println!("SUMMARY: Autosync Enable — service ready. File browser + game tracker now running.");
    Ok(())
}

pub fn cmd_recover() -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header("Recover");
    ttyui::section("Killing stale processes");
    let _ = Command::new("sudo")
        .args(["killall", "-9", "playora-agent"])
        .status();
    ttyui::row("playora-agent", "killed", Status::Ok);
    let _ = Command::new("sudo")
        .args(["killall", "-9", "gptokeyb"])
        .status();
    ttyui::row("gptokeyb", "killed", Status::Ok);
    let _ = std::fs::remove_file("/tmp/playora-scan.lock");
    let _ = std::fs::remove_file("/tmp/playora-extract-roms.lock");
    let _ = std::fs::remove_file("/tmp/playora-compress-roms.lock");
    let _ = std::fs::remove_file("/tmp/playora-cleanup.lock");
    let _ = std::fs::remove_file("/tmp/playora-cloud-setup.lock");
    let _ = std::fs::remove_file("/tmp/playora-cloud-backup.lock");
    let _ = std::fs::remove_file("/tmp/playora-cloud-restore.lock");
    let _ = std::fs::remove_file("/tmp/playora-cloud-catalog.lock");
    let _ = std::fs::remove_file("/tmp/playora-fetch-covers.lock");
    let _ = std::fs::remove_file("/tmp/playora-restore-tar.lock");
    let _ = std::fs::remove_file("/tmp/playora-cloud-download.lock");
    ttyui::row("locks", "cleared", Status::Ok);
    println!();
    println!("SUMMARY: Recover ok — ES restart on exit.");
    Ok(())
}

pub fn cmd_disable() -> Result<()> {
    use crate::ttyui::{self, Status};
    ttyui::header("Autosync Disable");

    let st = Command::new("sudo")
        .args(["systemctl", "disable", "--now", "playora-agent.service"])
        .status();
    match st {
        Ok(s) if s.success() => ttyui::ok("playora-agent.service stopped + disabled"),
        Ok(s) => ttyui::row(
            "systemctl disable",
            &format!("exit {:?}", s.code()),
            Status::Warn,
        ),
        Err(_) => ttyui::row("systemctl", "not present", Status::Warn),
    }
    let _ = Command::new("pkill")
        .args(["-f", "playora-agent.*run"])
        .status();
    ttyui::ok("background agent processes killed");
    println!();
    println!("SUMMARY: Autosync Disable ok.");
    Ok(())
}

fn print_status() {
    use crate::ttyui::{self, Status};
    if let Ok(out) = Command::new("systemctl")
        .args(["is-active", "playora-agent.service"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let st = if s == "active" {
            Status::Ok
        } else {
            Status::Warn
        };
        ttyui::row("service status", &s, st);
    }
}

fn spawn_background_fallback() -> Result<()> {
    use crate::ttyui::{self, Status};
    Command::new("nohup")
        .args([
            "/roms/.playora/playora-agent",
            "--config",
            "/roms/.playora/agent.toml",
            "run",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    ttyui::row("background", "spawned via nohup", Status::Ok);
    println!();
    println!("SUMMARY: Autosync Enable (no systemd) — agent running in background.");
    Ok(())
}
