use anyhow::{Context, Result};
use chrono::{Timelike, Utc};
use playora_common::*;
use std::time::Duration;

pub fn cmd_status(cfg: AgentConfig) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let pending = crate::db::count_pending(&conn)?;
    let last_sync: Option<String> = conn
        .query_row(
            "SELECT last_success_at FROM sync_state WHERE server_url=?1",
            rusqlite::params![cfg.server_url],
            |r| r.get(0),
        )
        .ok();
    let svc = std::process::Command::new("systemctl")
        .args(["is-active", "playora-agent.service"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let pids = std::process::Command::new("pgrep")
        .args(["-f", "playora-agent.*run"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("== Playora Status ==");
    println!("device_id:        {}", cfg.device_id.0);
    println!("device_name:      {}", cfg.device_name);
    println!("server_url:       {}", cfg.server_url);
    println!("agent_version:    {}", crate::AGENT_VERSION);
    println!("pending_events:   {pending}");
    println!(
        "last_sync_at:     {}",
        last_sync.unwrap_or_else(|| "(never)".into())
    );
    println!("autosync_service: {svc}");
    println!(
        "running_pids:     {}",
        if pids.is_empty() {
            "(none)".into()
        } else {
            pids
        }
    );
    println!("now:              {}", Utc::now());
    Ok(())
}

pub fn cmd_heartbeat(cfg: AgentConfig) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let pending = crate::db::count_pending(&conn)?;
    let free_disk_mb = free_disk_mb(&cfg.rom_paths);
    let wifi_connected = wifi_connected();
    let hb = DeviceHeartbeat {
        agent_version: crate::AGENT_VERSION.into(),
        wifi_connected,
        free_disk_mb,
        pending_events: pending,
        captured_at: Utc::now(),
    };
    let ev = Event {
        event_id: EventId::new(),
        device_id: cfg.device_id.clone(),
        created_at: Utc::now(),
        payload: EventPayload::DeviceHeartbeat(hb),
    };
    crate::db::enqueue(&conn, &ev)?;
    println!("heartbeat queued ({} pending now)", pending + 1);
    Ok(())
}

pub fn cmd_sync_once(cfg: AgentConfig) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let events = crate::db::pending_events(&conn, cfg.max_batch_size)?;
    if events.is_empty() {
        println!("no pending events");
        return Ok(());
    }
    let batch = SyncBatch {
        device_id: cfg.device_id.clone(),
        agent_version: crate::AGENT_VERSION.into(),
        events: events.clone(),
    };
    let url = format!(
        "{}/api/v1/events/batch",
        cfg.server_url.trim_end_matches('/')
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    match client.post(&url).json(&batch).send() {
        Ok(resp) if resp.status().is_success() => {
            let ack: SyncAck = resp.json().context("decode ack")?;
            let acked: Vec<EventId> = ack
                .accepted
                .into_iter()
                .chain(ack.duplicates.into_iter())
                .collect();
            crate::db::mark_sent(&conn, &acked)?;
            crate::db::set_sync_success(&conn, &cfg.server_url)?;
            println!(
                "synced {} events; rejected={}",
                acked.len(),
                ack.rejected.len()
            );
        }
        Ok(resp) => {
            let s = format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
            crate::db::set_sync_error(&conn, &cfg.server_url, &s)?;
            anyhow::bail!(s);
        }
        Err(e) => {
            crate::db::set_sync_error(&conn, &cfg.server_url, &e.to_string())?;
            anyhow::bail!("sync error: {e}");
        }
    }
    Ok(())
}

pub fn cmd_run(cfg: AgentConfig) -> Result<()> {
    println!(
        "playora-agent run — heartbeat every {}s, sync every {}s",
        60, cfg.sync_interval_seconds
    );
    if let Some(h) = cfg.cloud_backup_daily_hour_utc {
        println!("scheduled: cloud backup daily at {h:02}:00 UTC");
    }
    if let Some(h) = cfg.scan_daily_hour_utc {
        println!("scheduled: scan ROMs daily at {h:02}:00 UTC");
    }
    if let Some(h) = cfg.extract_roms_daily_hour_utc {
        println!("scheduled: extract ROMs daily at {h:02}:00 UTC");
    }
    let mut sched = Scheduler::default();
    loop {
        let _ = cmd_heartbeat(cfg.clone());
        if let Err(e) = cmd_sync_once(cfg.clone()) {
            tracing::warn!("sync: {e}");
        }
        // Pull dashboard delete queue + apply.
        let _ = crate::cleanup::cmd_cleanup(cfg.clone(), true);
        // Scheduled jobs.
        sched.tick(&cfg);
        std::thread::sleep(Duration::from_secs(cfg.sync_interval_seconds as u64));
    }
}

#[derive(Default)]
struct Scheduler {
    last_cloud_backup_day: Option<i32>,
    last_scan_day: Option<i32>,
    last_extract_day: Option<i32>,
}

impl Scheduler {
    fn tick(&mut self, cfg: &AgentConfig) {
        let now = Utc::now();
        let h = now.hour() as u8;
        let day_key = now.format("%Y%j").to_string().parse::<i32>().unwrap_or(0);

        if let Some(target) = cfg.cloud_backup_daily_hour_utc {
            if h == target && self.last_cloud_backup_day != Some(day_key) {
                println!("[schedule] firing cloud backup");
                run_inhibited("cloud-backup", &["cloud", "backup"], cfg);
                self.last_cloud_backup_day = Some(day_key);
            }
        }
        if let Some(target) = cfg.scan_daily_hour_utc {
            if h == target && self.last_scan_day != Some(day_key) {
                println!("[schedule] firing scan");
                run_inhibited("scan", &["scan"], cfg);
                self.last_scan_day = Some(day_key);
            }
        }
        if let Some(target) = cfg.extract_roms_daily_hour_utc {
            if h == target && self.last_extract_day != Some(day_key) {
                println!("[schedule] firing extract-roms");
                run_inhibited("extract-roms", &["extract-roms"], cfg);
                self.last_extract_day = Some(day_key);
            }
        }
    }
}

fn run_inhibited(label: &str, args: &[&str], cfg: &AgentConfig) {
    use std::process::Command;
    let exe = std::env::current_exe().unwrap_or_else(|_| "playora-agent".into());
    // Try systemd-inhibit so console can't suspend mid-backup. Falls back to plain spawn.
    let has_inhibit = Command::new("systemd-inhibit")
        .arg("--version")
        .status()
        .is_ok();
    let _ = crate::activity::progress(cfg, label, &format!("scheduled {label} started"));
    let status = if has_inhibit {
        Command::new("systemd-inhibit")
            .args([
                "--what=sleep:idle:handle-power-key",
                "--why=playora-scheduled-job",
                "--mode=block",
            ])
            .arg(&exe)
            .args(args)
            .status()
    } else {
        Command::new(&exe).args(args).status()
    };
    let rc = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
    let _ = crate::activity::end(cfg, label, rc, None);
    println!("[schedule] {label} finished rc={rc}");
}

fn free_disk_mb(paths: &[String]) -> u64 {
    paths
        .iter()
        .filter_map(|p| {
            let snap = crate::hw::snapshot();
            snap.disks
                .into_iter()
                .find(|d| p.starts_with(&d.mount))
                .map(|d| d.free_bytes / 1024 / 1024)
        })
        .next()
        .unwrap_or(0)
}

fn wifi_connected() -> bool {
    let snap = crate::hw::snapshot();
    snap.net_ifs.iter().any(|n| n.is_wireless && n.up)
}
