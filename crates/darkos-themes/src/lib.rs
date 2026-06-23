//! Theme catalog and installer.
//! Initial catalog is curated and hard-coded; later moves to remote JSON.

use darkos_core::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeEntry {
    pub name: String,
    pub author: String,
    pub source_url: String,
    pub preview_url: Option<String>,
    pub size_mb_estimate: Option<u32>,
}

pub fn catalog() -> Vec<ThemeEntry> {
    vec![
        ThemeEntry {
            name: "es-theme-art-book-next".into(),
            author: "anthonycaccese".into(),
            source_url: "https://github.com/anthonycaccese/es-theme-art-book-next".into(),
            preview_url: None,
            size_mb_estimate: Some(120),
        },
        ThemeEntry {
            name: "es-theme-epicnoir".into(),
            author: "c64-dev".into(),
            source_url: "https://github.com/c64-dev/es-theme-epicnoir".into(),
            preview_url: None,
            size_mb_estimate: Some(80),
        },
    ]
}

pub fn themes_dir() -> PathBuf {
    PathBuf::from(std::env::var("DARKOS_THEMES_DIR").unwrap_or_else(|_| "/roms/themes".into()))
}

pub fn list_installed() -> Result<Vec<String>> {
    let dir = themes_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for e in std::fs::read_dir(&dir)? {
        let e = e?;
        if e.path().is_dir() {
            if let Some(n) = e.file_name().to_str() {
                out.push(n.to_string());
            }
        }
    }
    Ok(out)
}
