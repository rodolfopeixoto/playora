//! Shared types, errors, and conventions for the darkOs workspace.
//!
//! Pequeno por design: tudo aqui é estável e usado por múltiplas crates.
//! Mudanças aqui são breaking — pense duas vezes.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("config: {0}")]
    Config(String),

    #[error("db: {0}")]
    Db(String),

    #[error("hw probe: {0}")]
    Hw(String),

    #[error("network: {0}")]
    Net(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("other: {0}")]
    Other(String),
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::Other(value.to_string())
    }
}

/// Standard paths used across the system. Override via env or config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paths {
    pub roms_dir: String,
    pub state_dir: String,
    pub db_path: String,
    pub cache_dir: String,
    pub log_dir: String,
}

impl Default for Paths {
    fn default() -> Self {
        let base = std::env::var("DARKOS_HOME").unwrap_or_else(|_| "/roms/.darkos".into());
        Self {
            roms_dir: std::env::var("DARKOS_ROMS_DIR").unwrap_or_else(|_| "/roms".into()),
            state_dir: format!("{base}/state"),
            db_path: format!("{base}/state/darkos.db"),
            cache_dir: format!("{base}/cache"),
            log_dir: format!("{base}/logs"),
        }
    }
}

impl Paths {
    /// Create all directories if missing.
    pub fn ensure(&self) -> Result<()> {
        for d in [&self.state_dir, &self.cache_dir, &self.log_dir] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}
