use anyhow::Result;
use chrono::Utc;
use playora_common::{Event, EventId};
use rusqlite::{params, Connection};
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let c = Connection::open(path)?;
    for p in [
        "PRAGMA journal_mode=WAL",
        "PRAGMA synchronous=NORMAL",
        "PRAGMA temp_store=MEMORY",
        "PRAGMA foreign_keys=ON",
        // SD-card friendly: wait up to 15s for other process to release lock.
        "PRAGMA busy_timeout=15000",
        // Bigger cache reduces page churn on slow storage.
        "PRAGMA cache_size=-8000",
        "PRAGMA wal_autocheckpoint=2000",
    ] {
        c.execute_batch(p)?;
    }
    c.execute_batch(SCHEMA)?;
    apply_migrations(&c)?;
    Ok(c)
}

/// Versioned migration runner. Schema bootstraps via `CREATE TABLE IF NOT
/// EXISTS` in `SCHEMA`; each migration here is a delta on top. New
/// migrations append to MIGRATIONS — never reorder or rewrite history.
fn apply_migrations(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL,
            note TEXT
         );",
    )?;
    let current: i32 = c
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    for (v, note, sql) in MIGRATIONS {
        if *v > current {
            c.execute_batch(sql)?;
            c.execute(
                "INSERT INTO schema_migrations(version, applied_at, note) VALUES (?1, ?2, ?3)",
                params![v, Utc::now().to_rfc3339(), note],
            )?;
        }
    }
    Ok(())
}

const MIGRATIONS: &[(i32, &str, &str)] = &[
    (
        1,
        "initial bootstrap marker (schema already created via CREATE IF NOT EXISTS)",
        "-- no-op (records schema baseline)",
    ),
    (
        2,
        "sprint-1 event types — system_issue_detected, doctor_report, rom_audit_result, script_started/finished, session crashed/orphaned, save_changed, black_screen_recovered, es_restarted",
        "-- no-op: events_outbox.event_type is free-text, no DDL needed",
    ),
    (
        3,
        "file_hashes — full sha256 cache keyed by (path,size,mtime) so audit --hash is incremental",
        "CREATE TABLE IF NOT EXISTS file_hashes (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            sha256 TEXT NOT NULL,
            computed_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_file_hashes_sha ON file_hashes(sha256);",
    ),
];

pub fn enqueue(conn: &Connection, ev: &Event) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO events_outbox(event_id, event_type, payload_json, status, retry_count, created_at)
         VALUES (?1, ?2, ?3, 'pending', 0, ?4)",
        params![
            ev.event_id.0,
            event_type_str(&ev.payload),
            serde_json::to_string(ev)?,
            ev.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn pending_events(conn: &Connection, limit: u32) -> Result<Vec<Event>> {
    let mut stmt = conn.prepare(
        "SELECT payload_json FROM events_outbox WHERE status='pending' ORDER BY id LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        let s = r?;
        let ev: Event = serde_json::from_str(&s)?;
        out.push(ev);
    }
    Ok(out)
}

pub fn mark_sent(conn: &Connection, ids: &[EventId]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        tx.execute(
            "UPDATE events_outbox SET status='sent', synced_at=?2 WHERE event_id=?1",
            params![id.0, Utc::now().to_rfc3339()],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[allow(dead_code)] // reserved for future retry/backoff logic
pub fn mark_error(conn: &Connection, id: &EventId, err: &str) -> Result<()> {
    conn.execute(
        "UPDATE events_outbox SET retry_count=retry_count+1, last_error=?2 WHERE event_id=?1",
        params![id.0, err],
    )?;
    Ok(())
}

pub fn count_pending(conn: &Connection) -> Result<u32> {
    let n: u32 = conn.query_row(
        "SELECT COUNT(*) FROM events_outbox WHERE status='pending'",
        [],
        |r| r.get(0),
    )?;
    Ok(n)
}

pub fn set_sync_success(conn: &Connection, server_url: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state(server_url, last_success_at) VALUES (?1, ?2)
         ON CONFLICT(server_url) DO UPDATE SET last_success_at=excluded.last_success_at, last_error=NULL",
        params![server_url, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn set_sync_error(conn: &Connection, server_url: &str, err: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state(server_url, last_error) VALUES (?1, ?2)
         ON CONFLICT(server_url) DO UPDATE SET last_error=excluded.last_error",
        params![server_url, err],
    )?;
    Ok(())
}

fn event_type_str(p: &playora_common::EventPayload) -> &'static str {
    use playora_common::EventPayload::*;
    match p {
        DeviceHeartbeat(_) => "device_heartbeat",
        HardwareSnapshot(_) => "hardware_snapshot",
        HardwareTestResult(_) => "hardware_test_result",
        ResourceSample(_) => "resource_sample",
        GameSessionStarted(_) => "game_session_started",
        GameSessionFinished(_) => "game_session_finished",
        RomScanned(_) => "rom_scanned",
        SaveSnapshot(_) => "save_snapshot",
        Activity(_) => "activity",
        RestoreProgress(_) => "restore_progress",
        GameMetadata(_) => "game_metadata",
        SystemIssueDetected(_) => "system_issue_detected",
        DoctorReport(_) => "doctor_report",
        RomAuditResult(_) => "rom_audit_result",
        ScriptStarted(_) => "script_started",
        ScriptFinished(_) => "script_finished",
        GameSessionCrashed(_) => "game_session_crashed",
        GameSessionOrphaned(_) => "game_session_orphaned",
        SaveChanged(_) => "save_changed",
        BlackScreenRecovered(_) => "black_screen_recovered",
        EmulationStationRestarted(_) => "emulation_station_restarted",
        NetplayRoomCreated(_) => "netplay_room_created",
        NetplayRoomJoined(_) => "netplay_room_joined",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use playora_common::{DeviceHeartbeat, DeviceId, Event, EventId, EventPayload};
    use tempfile::tempdir;

    fn sample_event() -> Event {
        Event {
            event_id: EventId::new(),
            device_id: DeviceId::new(),
            created_at: Utc::now(),
            payload: EventPayload::DeviceHeartbeat(DeviceHeartbeat {
                agent_version: "0.0.0-test".into(),
                wifi_connected: false,
                free_disk_mb: 0,
                pending_events: 0,
                captured_at: Utc::now(),
            }),
        }
    }

    #[test]
    fn enqueue_and_pending_count() {
        let dir = tempdir().unwrap();
        let db = open(&dir.path().join("t.db")).unwrap();
        for _ in 0..3 {
            enqueue(&db, &sample_event()).unwrap();
        }
        assert_eq!(count_pending(&db).unwrap(), 3);
    }

    #[test]
    fn duplicate_event_id_idempotent() {
        let dir = tempdir().unwrap();
        let db = open(&dir.path().join("t.db")).unwrap();
        let ev = sample_event();
        enqueue(&db, &ev).unwrap();
        enqueue(&db, &ev).unwrap();
        assert_eq!(count_pending(&db).unwrap(), 1);
    }

    #[test]
    fn mark_sent_removes_from_pending() {
        let dir = tempdir().unwrap();
        let db = open(&dir.path().join("t.db")).unwrap();
        let ev = sample_event();
        enqueue(&db, &ev).unwrap();
        mark_sent(&db, &[ev.event_id]).unwrap();
        assert_eq!(count_pending(&db).unwrap(), 0);
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT NOT NULL UNIQUE,
    device_name TEXT, device_profile TEXT, os_family TEXT,
    agent_version TEXT, created_at TEXT, last_seen_at TEXT
);
CREATE TABLE IF NOT EXISTS hardware_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, payload_json TEXT, created_at TEXT, synced_at TEXT
);
CREATE TABLE IF NOT EXISTS hardware_tests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    test_id TEXT UNIQUE, test_type TEXT, status TEXT, score REAL,
    payload_json TEXT, error TEXT, created_at TEXT, synced_at TEXT
);
CREATE TABLE IF NOT EXISTS resource_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sample_id TEXT UNIQUE,
    cpu_total_percent REAL, cpu_per_core_json TEXT,
    memory_total_mb INTEGER, memory_used_mb INTEGER,
    process_name TEXT, process_pid INTEGER,
    process_cpu_percent REAL, process_memory_mb INTEGER,
    temperature_json TEXT, created_at TEXT, synced_at TEXT
);
CREATE TABLE IF NOT EXISTS games (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    system TEXT, name TEXT, rom_path TEXT UNIQUE, rom_hash TEXT,
    file_size INTEGER, extension TEXT, image_path TEXT,
    metadata_json TEXT, last_scanned_at TEXT
);
CREATE TABLE IF NOT EXISTS play_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT UNIQUE, game_id INTEGER REFERENCES games(id) ON DELETE SET NULL,
    system TEXT, game_name TEXT, rom_path TEXT, rom_hash TEXT, core TEXT,
    emulator_command TEXT, started_at TEXT, ended_at TEXT,
    duration_seconds INTEGER, exit_code INTEGER, save_changed INTEGER,
    max_cpu_percent REAL, max_memory_mb INTEGER, synced_at TEXT
);
CREATE TABLE IF NOT EXISTS events_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT UNIQUE, event_type TEXT, payload_json TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TEXT, last_error TEXT,
    created_at TEXT, synced_at TEXT
);
CREATE INDEX IF NOT EXISTS events_outbox_status_idx ON events_outbox(status);
CREATE TABLE IF NOT EXISTS save_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id INTEGER REFERENCES games(id) ON DELETE SET NULL,
    save_path TEXT UNIQUE, save_hash TEXT, file_size INTEGER,
    modified_at TEXT, created_at TEXT, synced_at TEXT
);
CREATE TABLE IF NOT EXISTS sync_state (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_url TEXT UNIQUE,
    last_success_at TEXT, last_error TEXT, pending_events INTEGER
);
CREATE TABLE IF NOT EXISTS feature_flags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feature_key TEXT UNIQUE, status TEXT, source TEXT,
    payload_json TEXT, updated_at TEXT
);
CREATE TABLE IF NOT EXISTS catalog_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    catalog_item_id TEXT UNIQUE, title TEXT, system TEXT,
    type TEXT, payload_json TEXT, cached_at TEXT
);
CREATE TABLE IF NOT EXISTS downloads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    catalog_item_id TEXT, title TEXT, system TEXT,
    status TEXT, file_path TEXT, expected_sha256 TEXT, actual_sha256 TEXT,
    created_at TEXT, completed_at TEXT, error TEXT
);
"#;
