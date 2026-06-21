//! PortMaster integration — fetches the real PortMaster catalog and installs ports.
//!
//! Differences vs the user-supplied Rust sketch:
//!   * Pulls the live JSON catalog from portmaster.games (no hard-coded URL guesses).
//!   * Streams downloads with progress callback (caller decides UI: CLI or TUI).
//!   * Preserves the native PortMaster layout (the .sh script lives at the root of
//!     /roms/ports/, the gamedata folder lives at /roms/ports/<port>/).
//!   * Sets +x on every *.sh extracted, idempotent.
//!   * Detects /roms/ports or /roms2/ports automatically.
//!   * No interactive prompts inside the library — the binary owns the UI.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CATALOG_URLS: &[&str] = &[
    "https://portmaster.games/ports.json",
    "https://raw.githubusercontent.com/PortsMaster/PortMaster-Info/main/ports.json",
];
const PORTMASTER_RELEASE_BASE: &str =
    "https://github.com/PortsMaster/PortMaster-New/releases/latest/download";
const USER_AGENT: &str = "playora-portmaster/0.1 (+https://github.com/ropeixoto/playora)";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CatalogEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub attr: PortAttr,
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub items_opt: Option<Vec<String>>,
    #[serde(default)]
    pub files: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PortAttr {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub porter: Vec<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub rtr: bool,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub arch: Vec<String>,
}

pub struct Catalog {
    pub ports: Vec<CatalogEntry>,
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(USER_AGENT)
        .build()?)
}

pub fn fetch_catalog() -> Result<Catalog> {
    let c = client()?;
    let mut last_err: Option<anyhow::Error> = None;
    for url in CATALOG_URLS {
        match c.get(*url).send() {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text()?;
                if let Ok(catalog) = parse_catalog(&body) {
                    return Ok(catalog);
                }
            }
            Ok(_) | Err(_) => {
                last_err = Some(anyhow!("failed: {url}"));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("no catalog source reachable")))
}

fn parse_catalog(json: &str) -> Result<Catalog> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let map = v
        .get("ports")
        .and_then(|x| x.as_object())
        .ok_or_else(|| anyhow!("malformed catalog: no `ports` map"))?;
    let mut ports = Vec::with_capacity(map.len());
    for (filename, val) in map {
        let mut entry: CatalogEntry = serde_json::from_value(val.clone()).unwrap_or_default();
        if entry.name.is_empty() {
            entry.name = filename.trim_end_matches(".zip").to_string();
        }
        ports.push(entry);
    }
    ports.sort_by(|a, b| {
        a.attr
            .title
            .to_lowercase()
            .cmp(&b.attr.title.to_lowercase())
    });
    Ok(Catalog { ports })
}

pub fn ports_root() -> PathBuf {
    for cand in ["/roms2/ports", "/roms/ports"] {
        if Path::new(cand).is_dir() {
            return PathBuf::from(cand);
        }
    }
    PathBuf::from("/roms/ports")
}

pub fn search<'a>(catalog: &'a Catalog, query: &str) -> Vec<&'a CatalogEntry> {
    let q = query.to_lowercase();
    catalog
        .ports
        .iter()
        .filter(|p| {
            p.attr.title.to_lowercase().contains(&q)
                || p.name.to_lowercase().contains(&q)
                || p.attr.genres.iter().any(|g| g.to_lowercase().contains(&q))
        })
        .collect()
}

pub struct InstallReport {
    pub port_name: String,
    pub installed_files: usize,
    pub requires_data: bool,
}

pub fn install(entry: &CatalogEntry, progress: impl Fn(u64, Option<u64>)) -> Result<InstallReport> {
    let dest = ports_root();
    std::fs::create_dir_all(&dest)?;
    let zip_name = format!("{}.zip", entry.name);
    let url = format!("{PORTMASTER_RELEASE_BASE}/{zip_name}");
    let tmp = dest.join(format!("{}.part", zip_name));

    let c = client()?;
    let mut resp = c.get(&url).send().with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {} fetching {url}", resp.status()));
    }
    let total = resp.content_length();

    let mut out = std::fs::File::create(&tmp)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut got: u64 = 0;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        got += n as u64;
        progress(got, total);
    }
    out.flush()?;
    drop(out);

    let installed = extract_zip(&tmp, &dest)?;
    std::fs::remove_file(&tmp).ok();

    let requires_data =
        !entry.items.is_empty() || entry.items_opt.as_ref().is_some_and(|v| !v.is_empty());
    let name = if entry.attr.title.is_empty() {
        entry.name.clone()
    } else {
        entry.attr.title.clone()
    };
    Ok(InstallReport {
        port_name: name,
        installed_files: installed,
        requires_data,
    })
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<usize> {
    use std::os::unix::fs::PermissionsExt;
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut count = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let raw = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("invalid zip entry"))?
            .to_path_buf();
        let outpath = dest.join(&raw);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&outpath)?;
        std::io::copy(&mut entry, &mut out)?;
        if outpath.extension().and_then(|e| e.to_str()) == Some("sh") {
            let mut perms = std::fs::metadata(&outpath)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&outpath, perms)?;
        }
        count += 1;
    }
    Ok(count)
}

pub fn list_installed() -> Vec<String> {
    let dest = ports_root();
    std::fs::read_dir(&dest)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|e| e.to_str()) == Some("sh") {
                Some(p.file_stem()?.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_catalog() {
        let j = r#"{
            "ports": {
                "vvvvvv.zip": {
                    "name":"vvvvvv",
                    "attr":{"title":"VVVVVV","porter":["someone"],"genres":["platformer"],"desc":"d","rtr":true,"arch":["aarch64"]},
                    "items":[],
                    "items_opt":null,
                    "files":{}
                },
                "stardew.zip": {
                    "attr":{"title":"Stardew Valley","rtr":false},
                    "items":["Stardew Valley.exe"]
                }
            }
        }"#;
        let c = parse_catalog(j).unwrap();
        assert_eq!(c.ports.len(), 2);
        assert!(c.ports.iter().any(|p| p.attr.title == "VVVVVV"));
        let sv = c
            .ports
            .iter()
            .find(|p| p.attr.title == "Stardew Valley")
            .unwrap();
        assert!(!sv.items.is_empty());
    }

    #[test]
    fn search_matches_title_or_genre() {
        let j = r#"{"ports":{"a.zip":{"attr":{"title":"VVVVVV","genres":["platformer"]}}}}"#;
        let c = parse_catalog(j).unwrap();
        assert_eq!(search(&c, "vvv").len(), 1);
        assert_eq!(search(&c, "platform").len(), 1);
        assert_eq!(search(&c, "xyz").len(), 0);
    }
}
