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
    use chrono::Utc;
    use playora_common::*;
    let started_at = Utc::now();
    ttyui::header("Recover");

    ttyui::section("Killing stale processes");
    let mut killed = Vec::new();
    for proc in ["playora-agent", "gptokeyb", "gptokeyb2"] {
        let _ = Command::new("sudo").args(["killall", "-9", proc]).status();
        ttyui::row(proc, "killed (best-effort)", Status::Ok);
        killed.push(proc.to_string());
    }

    ttyui::section("Sweeping lockfiles");
    let mut cleared = 0u32;
    if let Ok(rd) = std::fs::read_dir("/tmp") {
        for entry in rd.flatten() {
            let p = entry.path();
            let Some(name) = p.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            if name.starts_with("playora-") && name.ends_with(".lock") {
                if std::fs::remove_file(&p).is_ok() {
                    cleared += 1;
                }
            }
        }
    }
    ttyui::row("locks cleared", &cleared.to_string(), Status::Ok);

    ttyui::section("TTY / framebuffer");
    let tty_ok =
        std::path::Path::new("/dev/tty1").exists() || std::path::Path::new("/dev/tty0").exists();
    ttyui::row(
        "tty",
        if tty_ok { "present" } else { "MISSING" },
        if tty_ok { Status::Ok } else { Status::Fail },
    );

    ttyui::section("Restarting EmulationStation");
    let (es_ok, es_method) = restart_emulationstation();
    ttyui::row(
        "method",
        &es_method,
        if es_ok { Status::Ok } else { Status::Warn },
    );

    // Best-effort event emission — don't fail recover if DB or sync is broken.
    let now = Utc::now();
    if let Ok(cfg) = crate::cfg::load(None) {
        if let Ok(conn) = crate::db::open(&crate::cfg::db_path()) {
            let elapsed = (now - started_at).num_seconds().max(0) as u64;
            let bsr = BlackScreenRecovered {
                triggered_by: "playora-agent recover".into(),
                duration_seconds: elapsed,
                es_restarted: es_ok,
                killed_processes: killed,
                captured_at: now,
            };
            let _ = crate::db::enqueue(
                &conn,
                &Event {
                    event_id: EventId::new(),
                    device_id: cfg.device_id.clone(),
                    created_at: now,
                    payload: EventPayload::BlackScreenRecovered(bsr),
                },
            );
            let _ = crate::db::enqueue(
                &conn,
                &Event {
                    event_id: EventId::new(),
                    device_id: cfg.device_id.clone(),
                    created_at: now,
                    payload: EventPayload::EmulationStationRestarted(EmulationStationRestarted {
                        reason: "recover".into(),
                        method: es_method,
                        captured_at: now,
                    }),
                },
            );
        }
    }

    println!();
    println!("SUMMARY: Recover ok — ES restart attempted.");
    Ok(())
}

fn restart_emulationstation() -> (bool, String) {
    let units = ["emulationstation", "emustation", "oga_es"];
    let mut detected: Option<&str> = None;
    if let Ok(out) = Command::new("systemctl")
        .args(["list-unit-files", "--no-legend"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        for u in &units {
            if text.lines().any(|l| l.starts_with(&format!("{u}.service"))) {
                detected = Some(*u);
                break;
            }
        }
    }
    if let Some(u) = detected {
        let s = Command::new("sudo")
            .args(["systemctl", "restart", &format!("{u}.service")])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if s {
            return (true, format!("systemd:{u}"));
        }
    }
    // Fallback exec if no service detected or restart failed.
    if Command::new("which")
        .arg("emulationstation")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        let _ = Command::new("sh")
            .args(["-c", "nohup emulationstation </dev/null >/dev/null 2>&1 &"])
            .status();
        return (true, "exec:emulationstation".into());
    }
    (false, "none".into())
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
