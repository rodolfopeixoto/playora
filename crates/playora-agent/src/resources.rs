use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::collections::BTreeMap;
use sysinfo::System;

pub fn sample() -> ResourceUsageSample {
    let mut s = System::new_all();
    s.refresh_all();
    // Need 2 refreshes for accurate CPU
    std::thread::sleep(std::time::Duration::from_millis(200));
    s.refresh_cpu_all();
    let total_cpu = s.global_cpu_usage();
    let per_core: Vec<f32> = s.cpus().iter().map(|c| c.cpu_usage()).collect();
    let mem_total = s.total_memory() / 1024 / 1024;
    let mem_used = (s.total_memory() - s.available_memory()) / 1024 / 1024;
    let temps = crate::hw::snapshot().temps_c;
    ResourceUsageSample {
        sample_id: SampleId::new(),
        cpu_total_percent: total_cpu,
        cpu_per_core: per_core,
        memory_total_mb: mem_total,
        memory_used_mb: mem_used,
        process: None,
        temperatures: temps,
        created_at: Utc::now(),
    }
}

pub fn cmd_sample(cfg: AgentConfig) -> Result<()> {
    let s = sample();
    println!("{}", serde_json::to_string_pretty(&s)?);
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let ev = Event {
        event_id: EventId::new(),
        device_id: cfg.device_id,
        created_at: Utc::now(),
        payload: EventPayload::ResourceSample(s),
    };
    crate::db::enqueue(&conn, &ev)?;
    Ok(())
}

pub fn cmd_watch(interval: u64) -> Result<()> {
    loop {
        let s = sample();
        println!(
            "[{}] cpu={:.1}% mem={}/{}MB",
            s.created_at, s.cpu_total_percent, s.memory_used_mb, s.memory_total_mb
        );
        std::thread::sleep(std::time::Duration::from_secs(interval));
    }
}

// reserved for runtime probe later (kept off by default)
pub fn _stub() -> BTreeMap<String, String> {
    BTreeMap::new()
}
