use anyhow::Result;
use playora_common::GameSystem;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameMeta {
    pub system: String,
    pub name_query: String,
    pub display_name: String,
    pub genre: String,
    pub year: String,
    pub publisher: String,
    pub cover_url: String,
    pub source: String,
}

const UA: &str = concat!("playora-agent/", env!("CARGO_PKG_VERSION"));

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_metadata (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            system TEXT NOT NULL,
            name_query TEXT NOT NULL,
            display_name TEXT,
            genre TEXT,
            year TEXT,
            publisher TEXT,
            cover_url TEXT,
            source TEXT,
            fetched_at TEXT NOT NULL,
            UNIQUE(system, name_query)
        );
        CREATE INDEX IF NOT EXISTS game_metadata_sys ON game_metadata(system);",
    )?;
    Ok(())
}

pub fn cached(conn: &Connection, system: &str, name_query: &str) -> Option<GameMeta> {
    conn.query_row(
        "SELECT display_name, genre, year, publisher, cover_url, source
         FROM game_metadata WHERE system=?1 AND name_query=?2",
        rusqlite::params![system, name_query],
        |r| {
            Ok(GameMeta {
                system: system.into(),
                name_query: name_query.into(),
                display_name: r.get::<_, String>(0).unwrap_or_default(),
                genre: r.get::<_, String>(1).unwrap_or_default(),
                year: r.get::<_, String>(2).unwrap_or_default(),
                publisher: r.get::<_, String>(3).unwrap_or_default(),
                cover_url: r.get::<_, String>(4).unwrap_or_default(),
                source: r.get::<_, String>(5).unwrap_or_default(),
            })
        },
    )
    .ok()
}

pub fn store(conn: &Connection, m: &GameMeta) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO game_metadata
            (system, name_query, display_name, genre, year, publisher, cover_url, source, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            m.system, m.name_query, m.display_name, m.genre, m.year, m.publisher,
            m.cover_url, m.source, chrono::Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

pub fn fetch(system: GameSystem, name: &str) -> Result<GameMeta> {
    let cleaned = clean_query(name);
    let system_slug = system_slug(system);
    let url = format!(
        "https://api.thegamesdb.net/v1/Games/ByGameName?apikey={}&name={}&fields=publishers,genres&include=boxart",
        thegamesdb_apikey(),
        urlencoding::encode(&cleaned)
    );
    let client = reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(8))
        .build()?;
    let resp = client.get(&url).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("thegamesdb HTTP {}", resp.status());
    }
    let v: serde_json::Value = resp.json()?;
    let games = v
        .pointer("/data/games")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let game = games
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no match for {cleaned}"))?;
    let display_name = game
        .get("game_title")
        .and_then(|x| x.as_str())
        .unwrap_or(&cleaned)
        .to_string();
    let year = game
        .get("release_date")
        .and_then(|x| x.as_str())
        .map(|s| s.split('-').next().unwrap_or(s).to_string())
        .unwrap_or_default();
    Ok(GameMeta {
        system: system_slug.to_string(),
        name_query: cleaned.clone(),
        display_name,
        genre: String::new(),
        year,
        publisher: String::new(),
        cover_url: String::new(),
        source: "thegamesdb".into(),
    })
}

fn thegamesdb_apikey() -> String {
    std::env::var("THEGAMESDB_API_KEY").unwrap_or_else(|_| "PUBLIC_DEMO_KEY".into())
}

fn system_slug(s: GameSystem) -> &'static str {
    use GameSystem::*;
    match s {
        Nes => "nes",
        Snes => "snes",
        Gb => "gb",
        Gbc => "gbc",
        Gba => "gba",
        Megadrive => "megadrive",
        _ => "other",
    }
}

pub fn clean_query(rom_name: &str) -> String {
    let mut s = rom_name.to_string();
    if let Some(dot) = s.rfind('.') {
        s.truncate(dot);
    }
    let cuts = [" (", " [", "_("];
    for c in cuts {
        if let Some(i) = s.find(c) {
            s.truncate(i);
        }
    }
    s.replace('_', " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clean_query_strips_region_tags() {
        assert_eq!(clean_query("Super Mario (USA).smc"), "Super Mario");
        assert_eq!(clean_query("Castlevania [Hack].nes"), "Castlevania");
        assert_eq!(
            clean_query("Donkey_Kong_Country.smc"),
            "Donkey Kong Country"
        );
        assert_eq!(clean_query("Sonic.gen"), "Sonic");
    }

    #[test]
    fn cache_roundtrip() {
        let dir = tempdir().unwrap();
        let conn = Connection::open(dir.path().join("m.db")).unwrap();
        ensure_table(&conn).unwrap();
        let m = GameMeta {
            system: "snes".into(),
            name_query: "Mario".into(),
            display_name: "Super Mario World".into(),
            genre: "Platformer".into(),
            year: "1990".into(),
            publisher: "Nintendo".into(),
            cover_url: "https://example/cover.png".into(),
            source: "test".into(),
        };
        store(&conn, &m).unwrap();
        let got = cached(&conn, "snes", "Mario").unwrap();
        assert_eq!(got.display_name, "Super Mario World");
        assert_eq!(got.year, "1990");
    }
}
