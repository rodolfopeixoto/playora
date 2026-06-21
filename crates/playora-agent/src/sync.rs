use anyhow::{Context, Result};
use chrono::Utc;
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
    loop {
        let _ = cmd_heartbeat(cfg.clone());
        if let Err(e) = cmd_sync_once(cfg.clone()) {
            tracing::warn!("sync: {e}");
        }
        std::thread::sleep(Duration::from_secs(cfg.sync_interval_seconds as u64));
    }
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
