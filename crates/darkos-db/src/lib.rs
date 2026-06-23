//! SQLite layer for darkOs. Schema + migrations + simple repositories.

use darkos_core::{Error, Result};
use darkos_hw::HardwareSnapshot;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(p).map_err(|e| Error::Db(e.to_string()))?;
        // pragmas suitable for SD card (small, conservative)
        for pragma in [
            "PRAGMA journal_mode = WAL",
            "PRAGMA synchronous = NORMAL",
            "PRAGMA temp_store = MEMORY",
            "PRAGMA foreign_keys = ON",
        ] {
            conn.execute_batch(pragma)
                .map_err(|e| Error::Db(e.to_string()))?;
        }
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(SCHEMA_SQL)
            .map_err(|e| Error::Db(e.to_string()))
    }

    pub fn record_hw_snapshot(&self, snap: &HardwareSnapshot) -> Result<i64> {
        let json = serde_json::to_string(snap)
            .map_err(|e| Error::Db(format!("serialize snapshot: {e}")))?;
        self.conn
            .execute(
                "INSERT INTO hw_snapshots(ts, json) VALUES (?1, ?2)",
                params![snap.timestamp.to_rfc3339(), json],
            )
            .map_err(|e| Error::Db(e.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn latest_hw_snapshot(&self) -> Result<Option<HardwareSnapshot>> {
        let mut stmt = self
            .conn
            .prepare("SELECT json FROM hw_snapshots ORDER BY id DESC LIMIT 1")
            .map_err(|e| Error::Db(e.to_string()))?;
        let mut rows = stmt.query([]).map_err(|e| Error::Db(e.to_string()))?;
        if let Some(row) = rows.next().map_err(|e| Error::Db(e.to_string()))? {
            let json: String = row.get(0).map_err(|e| Error::Db(e.to_string()))?;
            let snap: HardwareSnapshot =
                serde_json::from_str(&json).map_err(|e| Error::Db(format!("deserialize: {e}")))?;
            return Ok(Some(snap));
        }
        Ok(None)
    }

    pub fn upsert_rom(&self, rom: &RomRecord) -> Result<()> {
        self.conn
            .execute(
                r#"INSERT INTO roms(system, path, name, sha256, size_bytes, mtime)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                   ON CONFLICT(path) DO UPDATE SET
                     system=excluded.system,
                     name=excluded.name,
                     sha256=excluded.sha256,
                     size_bytes=excluded.size_bytes,
                     mtime=excluded.mtime"#,
                params![
                    rom.system,
                    rom.path,
                    rom.name,
                    rom.sha256,
                    rom.size_bytes as i64,
                    rom.mtime
                ],
            )
            .map_err(|e| Error::Db(e.to_string()))?;
        Ok(())
    }

    pub fn count_roms_by_system(&self) -> Result<Vec<(String, i64, i64)>> {
        let mut stmt = self.conn
            .prepare("SELECT system, COUNT(*), COALESCE(SUM(size_bytes),0) FROM roms GROUP BY system ORDER BY system")
            .map_err(|e| Error::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| Error::Db(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Db(e.to_string()))?);
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct RomRecord {
    pub system: String,
    pub path: String,
    pub name: String,
    pub sha256: Option<String>,
    pub size_bytes: u64,
    pub mtime: i64,
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS hw_snapshots (
    id    INTEGER PRIMARY KEY AUTOINCREMENT,
    ts    TEXT NOT NULL,
    json  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS roms (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    system      TEXT NOT NULL,
    path        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    sha256      TEXT,
    size_bytes  INTEGER NOT NULL,
    mtime       INTEGER NOT NULL,
    play_count  INTEGER NOT NULL DEFAULT 0,
    play_seconds INTEGER NOT NULL DEFAULT 0,
    favorite    INTEGER NOT NULL DEFAULT 0,
    rating      INTEGER
);
CREATE INDEX IF NOT EXISTS roms_system_idx ON roms(system);

CREATE TABLE IF NOT EXISTS saves (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    rom_id     INTEGER REFERENCES roms(id) ON DELETE CASCADE,
    path       TEXT NOT NULL,
    kind       TEXT NOT NULL, -- srm|sav|state|mcr|...
    size_bytes INTEGER NOT NULL,
    mtime      INTEGER NOT NULL,
    sha256     TEXT
);
CREATE INDEX IF NOT EXISTS saves_rom_idx ON saves(rom_id);

CREATE TABLE IF NOT EXISTS firmwares (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    version      TEXT NOT NULL,
    source_url   TEXT,
    image_sha256 TEXT,
    installed_at TEXT,
    notes        TEXT
);

CREATE TABLE IF NOT EXISTS themes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    source_url  TEXT,
    install_dir TEXT,
    active      INTEGER NOT NULL DEFAULT 0,
    installed_at TEXT
);

CREATE TABLE IF NOT EXISTS rom_sources (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    system    TEXT NOT NULL,
    base_url  TEXT NOT NULL,
    note      TEXT
);

CREATE TABLE IF NOT EXISTS events (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    ts   TEXT NOT NULL,
    kind TEXT NOT NULL,
    data TEXT
);
"#;
