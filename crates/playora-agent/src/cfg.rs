use anyhow::{Context, Result};
use playora_common::{AgentConfig, DeviceId};
use std::path::PathBuf;
use std::sync::OnceLock;

static ACTIVE_CFG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn default_config_path() -> PathBuf {
    if std::path::Path::new("/roms").is_dir() {
        PathBuf::from("/roms/playora/agent.toml")
    } else {
        dirs_home().join(".playora/agent.toml")
    }
}

pub fn config_path() -> PathBuf {
    ACTIVE_CFG_PATH.get().cloned().unwrap_or_else(default_config_path)
}

pub fn state_dir() -> PathBuf {
    config_path().parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

pub fn db_path() -> PathBuf { state_dir().join("playora.db") }
pub fn log_path() -> PathBuf { state_dir().join("agent.log") }

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn load(explicit: Option<&str>) -> Result<AgentConfig> {
    let path = explicit.map(PathBuf::from).unwrap_or_else(default_config_path);
    if !path.exists() {
        anyhow::bail!("config not found at {}. Run `playora-agent init` first.", path.display());
    }
    let _ = ACTIVE_CFG_PATH.set(path.clone());
    let txt = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let cfg: AgentConfig = toml::from_str(&txt)?;
    Ok(cfg)
}

pub fn cmd_init(explicit: Option<&str>, server_url: Option<String>, device_name: Option<String>) -> Result<()> {
    let path: PathBuf = explicit.map(PathBuf::from).unwrap_or_else(default_config_path);
    let _ = ACTIVE_CFG_PATH.set(path.clone());
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("mkdir {}", dir.display()))?;
    let mut cfg = if path.exists() {
        load(Some(path.to_string_lossy().as_ref()))?
    } else {
        AgentConfig {
            device_id: DeviceId::new(),
            ..AgentConfig::default()
        }
    };
    if let Some(u) = server_url { cfg.server_url = u; }
    if let Some(n) = device_name { cfg.device_name = n; }

    let txt = toml::to_string_pretty(&cfg)?;
    std::fs::write(&path, txt)?;
    // initialize DB next to config
    let db = dir.join("playora.db");
    let _ = crate::db::open(&db)?;
    println!("config:  {}", path.display());
    println!("db:      {}", db.display());
    println!("device:  {}", cfg.device_id);
    println!("server:  {}", cfg.server_url);
    Ok(())
}
