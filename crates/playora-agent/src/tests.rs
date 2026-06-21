use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::time::Duration;

pub fn cmd_doctor(cfg: AgentConfig, apply_fixes: bool) -> Result<()> {
    use crate::ttyui::{self, Status};

    ttyui::header("Doctor — Health Check");

    ttyui::section("Identity");
    ttyui::row("device_id", &cfg.device_id.0, Status::Info);
    ttyui::row("device_name", &cfg.device_name, Status::Info);
    ttyui::row("agent_version", env!("CARGO_PKG_VERSION"), Status::Info);

    ttyui::section("Storage");
    let roms_writeable = is_writeable("/roms");
    ttyui::row(
        "/roms writeable",
        if roms_writeable { "yes" } else { "READ-ONLY" },
        if roms_writeable {
            Status::Ok
        } else {
            Status::Fail
        },
    );
    let snap = crate::hw::snapshot();
    let free_mb = snap
        .disks
        .iter()
        .find(|d| d.mount == "/roms")
        .map(|d| d.free_bytes / 1024 / 1024)
        .unwrap_or(0);
    let st = if free_mb >= 1024 {
        Status::Ok
    } else if free_mb > 0 {
        Status::Warn
    } else {
        Status::Fail
    };
    ttyui::row("/roms free", &format!("{free_mb} MB"), st);

    ttyui::section("Local DB");
    let db = crate::db::open(&crate::cfg::db_path());
    ttyui::row(
        "sqlite db",
        if db.is_ok() { "OK" } else { "FAIL" },
        if db.is_ok() { Status::Ok } else { Status::Fail },
    );
    let pending = db
        .as_ref()
        .ok()
        .and_then(|c| crate::db::count_pending(c).ok())
        .unwrap_or(0);
    ttyui::row("pending events", &pending.to_string(), Status::Info);

    ttyui::section("Server");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    let url = format!("{}/dashboard", cfg.server_url.trim_end_matches('/'));
    match client.get(&url).send() {
        Ok(r) if r.status().is_success() => ttyui::row(
            "dashboard reachable",
            &format!("HTTP {}", r.status()),
            Status::Ok,
        ),
        Ok(r) => ttyui::row(
            "dashboard reachable",
            &format!("HTTP {}", r.status()),
            Status::Warn,
        ),
        Err(e) => ttyui::row(
            "dashboard reachable",
            &short_err(&e.to_string()),
            Status::Fail,
        ),
    }

    ttyui::section("Tools");
    for (tool, hint) in [
        ("retroarch", "install dArkOSRE base packages"),
        (
            "qrencode",
            "sudo apt-get install qrencode (Cloud Setup uses it)",
        ),
        ("fbv", "sudo apt-get install fbv (framebuffer image viewer)"),
        ("chdman", "sudo apt-get install mame-tools (Compress ROMs)"),
        ("rclone", "reinstall via install-to-sd.sh (Cloud)"),
    ] {
        let found = which_any(tool).is_some();
        let st = if found { Status::Ok } else { Status::Warn };
        ttyui::row(tool, if found { "found" } else { "missing" }, st);
        if !found {
            ttyui::note(hint);
        }
    }

    ttyui::section("Cloud");
    let rclone_conf = std::path::Path::new("/roms/.playora/rclone.conf");
    let cloud_ready = rclone_conf
        .exists()
        .then(|| std::fs::read_to_string(rclone_conf).ok())
        .flatten()
        .map(|s| s.contains("[gdrive]"))
        .unwrap_or(false);
    ttyui::row(
        "gdrive configured",
        if cloud_ready { "yes" } else { "not yet" },
        if cloud_ready {
            Status::Ok
        } else {
            Status::Warn
        },
    );
    if !cloud_ready {
        ttyui::note("Run Cloud Setup to pair Google Drive.");
    }

    ttyui::section("RetroArch — exit-game freeze workaround");
    let ra_cfg = find_retroarch_cfg();
    if let Some(path) = ra_cfg.as_ref() {
        let threaded = retroarch_video_threaded(path);
        ttyui::row(
            "video_threaded",
            match threaded {
                Some(true) => "true (causes black-screen on exit)",
                Some(false) => "false (fix applied)",
                None => "unset (defaults to true)",
            },
            if threaded == Some(false) {
                Status::Ok
            } else {
                Status::Warn
            },
        );
        ttyui::note(&format!("config: {}", path.display()));
        if threaded != Some(false) {
            if apply_fixes {
                match patch_retroarch_threaded(path) {
                    Ok(_) => ttyui::ok("patched video_threaded=false (restart RetroArch / reboot)"),
                    Err(e) => ttyui::fail(&format!("patch failed: {e}")),
                }
            } else {
                ttyui::note("Run `playora-agent doctor --apply-fixes` to patch automatically.");
            }
        }
    } else {
        ttyui::row("retroarch.cfg", "not found", Status::Warn);
    }

    ttyui::section("Hardware");
    ttyui::row(
        "cpu",
        &format!("{} ({} cores)", snap.cpu_model, snap.cpu_cores),
        Status::Info,
    );
    ttyui::row("memory", &format!("{} MB", snap.mem_total_mb), Status::Info);
    ttyui::row("kernel", &snap.kernel, Status::Info);
    ttyui::row(
        "panel",
        snap.panel_compatible.as_deref().unwrap_or("?"),
        Status::Info,
    );
    ttyui::row(
        "retroarch",
        if snap.retroarch_detected {
            "detected"
        } else {
            "absent"
        },
        if snap.retroarch_detected {
            Status::Ok
        } else {
            Status::Warn
        },
    );

    println!();
    println!("  Doctor complete.");
    Ok(())
}

fn is_writeable(path: &str) -> bool {
    let test = format!("{path}/.playora-doctor-write-test");
    let res = std::fs::write(&test, b"x");
    std::fs::remove_file(&test).ok();
    res.is_ok()
}

fn which_any(tool: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let p = std::path::Path::new(dir).join(tool);
        if p.is_file() {
            return Some(p);
        }
    }
    let bundled = std::path::Path::new("/roms/.playora/bin").join(tool);
    if bundled.is_file() {
        return Some(bundled);
    }
    None
}

fn find_retroarch_cfg() -> Option<std::path::PathBuf> {
    let candidates = [
        "/home/ark/.config/retroarch/retroarch.cfg",
        "/root/.config/retroarch/retroarch.cfg",
        "/opt/retroarch/.config/retroarch/retroarch.cfg",
        "/home/pi/.config/retroarch/retroarch.cfg",
    ];
    for c in &candidates {
        let p = std::path::Path::new(c);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    None
}

fn retroarch_video_threaded(path: &std::path::Path) -> Option<bool> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("video_threaded") {
            return Some(t.contains("true"));
        }
    }
    None
}

fn patch_retroarch_threaded(path: &std::path::Path) -> Result<()> {
    let backup = path.with_extension("cfg.playora-bak");
    if !backup.exists() {
        std::fs::copy(path, &backup)?;
    }
    let content = std::fs::read_to_string(path)?;
    let mut found = false;
    let mut out = String::new();
    for line in content.lines() {
        if line.trim_start().starts_with("video_threaded") {
            out.push_str("video_threaded = \"false\"\n");
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        out.push_str("video_threaded = \"false\"\n");
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn short_err(e: &str) -> String {
    let mut s = e.to_string();
    if s.len() > 60 {
        s.truncate(60);
        s.push('…');
    }
    s
}

pub fn cmd_test_session(cfg: AgentConfig, system: &str, game: &str, duration: u64) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let session_id = SessionId::new();
    let started = Utc::now();
    crate::db::enqueue(
        &conn,
        &Event {
            event_id: EventId::new(),
            device_id: cfg.device_id.clone(),
            created_at: started,
            payload: EventPayload::GameSessionStarted(GameSessionStarted {
                session_id: session_id.clone(),
                system: GameSystem::from_folder(system),
                game_name: game.into(),
                rom_path: format!("/roms/{system}/{game}"),
                rom_hash: None,
                core: None,
                started_at: started,
            }),
        },
    )?;
    println!(
        "started fake session {} for '{}' ({})",
        session_id, game, system
    );
    std::thread::sleep(Duration::from_secs(duration));
    let ended = Utc::now();
    crate::db::enqueue(
        &conn,
        &Event {
            event_id: EventId::new(),
            device_id: cfg.device_id.clone(),
            created_at: ended,
            payload: EventPayload::GameSessionFinished(GameSessionFinished {
                session_id,
                ended_at: ended,
                duration_seconds: duration,
                exit_code: Some(0),
                save_changed: false,
                max_cpu_percent: Some(0.0),
                max_memory_mb: Some(0),
            }),
        },
    )?;
    println!("finished fake session: {duration}s");
    // Best effort: trigger one sync if server reachable
    let _ = crate::sync::cmd_sync_once(cfg);
    Ok(())
}

pub fn cmd_hardware_test(cfg: AgentConfig, mode: &str, _interactive: bool) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let mut results = vec![];

    // 1) sqlite OK
    results.push(HardwareTestResult {
        test_id: TestId::new(),
        test_type: "sqlite".into(),
        status: "pass".into(),
        score: None,
        payload: serde_json::json!({"path": crate::cfg::db_path()}),
        error: None,
        created_at: Utc::now(),
    });

    // 2) config OK
    results.push(HardwareTestResult {
        test_id: TestId::new(),
        test_type: "config".into(),
        status: "pass".into(),
        score: None,
        payload: serde_json::json!({"path": crate::cfg::config_path()}),
        error: None,
        created_at: Utc::now(),
    });

    // 3) server reachable
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let server_ok = client
        .get(format!("{}/health", cfg.server_url.trim_end_matches('/')))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    results.push(HardwareTestResult {
        test_id: TestId::new(),
        test_type: "server_reachable".into(),
        status: if server_ok { "pass" } else { "fail" }.into(),
        score: None,
        payload: serde_json::json!({"url": cfg.server_url}),
        error: None,
        created_at: Utc::now(),
    });

    // 4) free space
    let snap = crate::hw::snapshot();
    let free_ok = snap
        .disks
        .iter()
        .any(|d| d.mount == "/roms" && d.free_bytes > 500 * 1024 * 1024);
    results.push(HardwareTestResult {
        test_id: TestId::new(),
        test_type: "free_space".into(),
        status: if free_ok { "pass" } else { "warn" }.into(),
        score: None,
        payload: serde_json::json!({"disks": snap.disks}),
        error: None,
        created_at: Utc::now(),
    });

    if mode == "full" {
        // 5) storage speed (write small temp file, time it)
        let tmp = std::env::temp_dir().join("playora_speed.tmp");
        let buf = vec![0u8; 4 * 1024 * 1024];
        let start = std::time::Instant::now();
        std::fs::write(&tmp, &buf)?;
        let elapsed = start.elapsed().as_secs_f64();
        let mbs = (buf.len() as f64 / 1024.0 / 1024.0) / elapsed;
        let _ = std::fs::remove_file(&tmp);
        results.push(HardwareTestResult {
            test_id: TestId::new(),
            test_type: "storage_write_speed".into(),
            status: if mbs > 2.0 { "pass" } else { "warn" }.into(),
            score: Some(mbs as f32),
            payload: serde_json::json!({"mb_per_sec": mbs}),
            error: None,
            created_at: Utc::now(),
        });
    }

    for r in &results {
        println!("[{}] {} score={:?}", r.status, r.test_type, r.score);
        crate::db::enqueue(
            &conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: Utc::now(),
                payload: EventPayload::HardwareTestResult(r.clone()),
            },
        )?;
    }
    Ok(())
}
