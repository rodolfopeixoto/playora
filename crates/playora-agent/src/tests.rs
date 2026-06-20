use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::time::Duration;

pub fn cmd_doctor(cfg: AgentConfig, _interactive: bool) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path());
    println!("config:   {}", crate::cfg::config_path().display());
    println!(
        "db:       {}  {}",
        crate::cfg::db_path().display(),
        if conn.is_ok() { "OK" } else { "FAIL" }
    );

    // network probe
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let url = format!("{}/health", cfg.server_url.trim_end_matches('/'));
    match client.get(&url).send() {
        Ok(r) => println!("server:   {} -> HTTP {}", url, r.status()),
        Err(e) => println!("server:   {} -> ERROR {e}", url),
    }

    // proc
    println!(
        "/proc:    {}",
        if std::path::Path::new("/proc/cpuinfo").exists() {
            "OK"
        } else {
            "absent"
        }
    );
    let snap = crate::hw::snapshot();
    println!("cpu:      {} cores ({})", snap.cpu_cores, snap.cpu_model);
    println!("mem:      {}MB total", snap.mem_total_mb);
    let pending = conn
        .ok()
        .and_then(|c| crate::db::count_pending(&c).ok())
        .unwrap_or(0);
    println!("pending:  {pending} events");

    Ok(())
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
