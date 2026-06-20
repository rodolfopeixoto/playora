use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::process::Command;
use std::time::{Duration, Instant};

pub fn cmd_launch(
    cfg: AgentConfig,
    system: &str,
    core: Option<&str>,
    rom: &str,
    command: &[String],
) -> Result<()> {
    let session_id = SessionId::new();
    let started = Utc::now();
    let conn_open = crate::db::open(&crate::cfg::db_path());
    let game_name = std::path::Path::new(rom)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rom)
        .to_string();

    // Enqueue start event (best-effort)
    if let Ok(ref conn) = conn_open {
        let _ = crate::db::enqueue(
            conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: started,
                payload: EventPayload::GameSessionStarted(GameSessionStarted {
                    session_id: session_id.clone(),
                    system: GameSystem::from_folder(system),
                    game_name: game_name.clone(),
                    rom_path: rom.into(),
                    rom_hash: None,
                    core: core.map(|s| s.into()),
                    started_at: started,
                }),
            },
        );
    }

    // Run the original emulator command. NEVER block the game.
    let start_inst = Instant::now();
    let mut max_cpu = 0.0f32;
    let mut max_mem_mb = 0u64;

    let mut child = if command.is_empty() {
        // No command — only a fake run for testing
        anyhow::bail!("no command provided after `--`");
    } else {
        let mut c = Command::new(&command[0]);
        c.args(&command[1..]);
        c.spawn()?
    };

    // Light sampling loop every 30s, polling child + system. Yields immediately if child gone.
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if start_inst.elapsed().as_secs() % 30 == 0 {
            let s = crate::resources::sample();
            max_cpu = max_cpu.max(s.cpu_total_percent);
            max_mem_mb = max_mem_mb.max(s.memory_used_mb);
        }
    }
    let exit = child.wait()?;
    let ended = Utc::now();
    let duration = (ended - started).num_seconds().max(0) as u64;

    if let Ok(ref conn) = conn_open {
        let _ = crate::db::enqueue(
            conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: ended,
                payload: EventPayload::GameSessionFinished(GameSessionFinished {
                    session_id,
                    ended_at: ended,
                    duration_seconds: duration,
                    exit_code: exit.code(),
                    save_changed: false,
                    max_cpu_percent: Some(max_cpu),
                    max_memory_mb: Some(max_mem_mb),
                }),
            },
        );
    }
    println!("session finished: {duration}s exit={:?}", exit.code());
    Ok(())
}
