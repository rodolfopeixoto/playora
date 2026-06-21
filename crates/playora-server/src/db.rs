use anyhow::Result;
use rusqlite::Connection;

pub fn open(path: &str) -> Result<Connection> {
    let c = Connection::open(path)?;
    for p in [
        "PRAGMA journal_mode=WAL",
        "PRAGMA synchronous=NORMAL",
        "PRAGMA temp_store=MEMORY",
        "PRAGMA foreign_keys=ON",
    ] {
        c.execute_batch(p)?;
    }
    c.execute_batch(SCHEMA)?;
    // Best-effort migrations for columns added after initial release.
    let _ = c.execute_batch(
        "ALTER TABLE activities ADD COLUMN summary TEXT;\
         ALTER TABLE activities ADD COLUMN stdout_tail TEXT;",
    );
    seed_catalog(&c)?;
    Ok(c)
}

fn seed_catalog(c: &Connection) -> Result<()> {
    // Idempotent: only seed if catalog is empty
    let n: i64 = c.query_row("SELECT COUNT(*) FROM catalog_items", [], |r| r.get(0))?;
    if n > 0 {
        return Ok(());
    }
    // Minimal mock of LEGAL homebrew/open-source items.
    let items = [
        (
            "hb-cave-story",
            "Cave Story (Doukutsu)",
            "psx",
            "homebrew_game",
            "freeware",
            "Pixel",
        ),
        (
            "ob-2048",
            "2048 SDL",
            "ports",
            "open_source_game",
            "MIT",
            "gabrielecirulli",
        ),
        (
            "hb-pixel-dungeon",
            "Shattered Pixel Dungeon",
            "ports",
            "open_source_game",
            "GPL-3.0",
            "00-Evan",
        ),
        (
            "th-art-book",
            "Theme: ArtBookNext",
            "themes",
            "theme",
            "CC-BY-NC",
            "anthonycaccese",
        ),
        (
            "cfg-rk3326-low",
            "Config Pack: RK3326 low-power",
            "configs",
            "config_pack",
            "Public Domain",
            "playora",
        ),
    ];
    for (id, title, system, ty, license, author) in items {
        c.execute(
            "INSERT INTO catalog_items(id,title,system,type,license,author,payload_json) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![id, title, system, ty, license, author, "{}"],
        )?;
    }
    Ok(())
}

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT UNIQUE NOT NULL,
    device_name TEXT, device_profile TEXT, os_family TEXT,
    agent_version TEXT, last_ip TEXT, last_seen_at TEXT, created_at TEXT
);
CREATE TABLE IF NOT EXISTS device_capabilities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, payload_json TEXT, created_at TEXT
);
CREATE TABLE IF NOT EXISTS device_features (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, feature_key TEXT, status TEXT, reason TEXT, updated_at TEXT,
    UNIQUE(device_id, feature_key)
);
CREATE TABLE IF NOT EXISTS hardware_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, payload_json TEXT, received_at TEXT
);
CREATE TABLE IF NOT EXISTS hardware_tests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, test_type TEXT, status TEXT, score REAL,
    payload_json TEXT, received_at TEXT
);
CREATE TABLE IF NOT EXISTS resource_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, payload_json TEXT, received_at TEXT
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT UNIQUE NOT NULL,
    device_id TEXT, event_type TEXT, payload_json TEXT, received_at TEXT
);
CREATE INDEX IF NOT EXISTS events_device_idx ON events(device_id);
CREATE TABLE IF NOT EXISTS activities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT,
    script TEXT,
    status TEXT,
    started_at TEXT,
    ended_at TEXT,
    exit_code INTEGER,
    log_path TEXT,
    summary TEXT,
    stdout_tail TEXT
);
CREATE INDEX IF NOT EXISTS activities_device_idx ON activities(device_id);
CREATE INDEX IF NOT EXISTS activities_started_idx ON activities(started_at);
CREATE TABLE IF NOT EXISTS restore_progress (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT,
    bytes_done INTEGER,
    bytes_total INTEGER,
    files_done INTEGER,
    current_path TEXT,
    received_at TEXT
);
CREATE INDEX IF NOT EXISTS restore_progress_device_idx ON restore_progress(device_id);
CREATE TABLE IF NOT EXISTS game_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT UNIQUE,
    device_id TEXT, system TEXT, game_name TEXT, rom_hash TEXT,
    core TEXT, started_at TEXT, ended_at TEXT, duration_seconds INTEGER,
    max_cpu_percent REAL, max_memory_mb INTEGER
);
CREATE TABLE IF NOT EXISTS heartbeats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, wifi_connected INTEGER, free_disk_mb INTEGER,
    pending_events INTEGER, received_at TEXT
);
CREATE TABLE IF NOT EXISTS catalog_items (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL, system TEXT, type TEXT,
    license TEXT, description TEXT, cover_url TEXT,
    download_url TEXT, sha256 TEXT, install_path TEXT,
    author TEXT, payload_json TEXT
);
CREATE TABLE IF NOT EXISTS downloads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT, catalog_item_id TEXT, status TEXT,
    payload_json TEXT, received_at TEXT
);
CREATE TABLE IF NOT EXISTS delete_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT NOT NULL,
    rom_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    requested_at TEXT NOT NULL,
    processed_at TEXT,
    error TEXT
);
CREATE INDEX IF NOT EXISTS delete_requests_device_idx ON delete_requests(device_id, status);
CREATE TABLE IF NOT EXISTS cloud_auth_tokens (
    device_id TEXT PRIMARY KEY,
    token TEXT NOT NULL,
    consumed_at TEXT,
    received_at TEXT NOT NULL
);
"#;
