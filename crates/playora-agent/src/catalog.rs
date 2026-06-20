use anyhow::Result;
use playora_common::*;
use std::time::Duration;

fn fetch_all(cfg: &AgentConfig) -> Result<Vec<CatalogItem>> {
    let url = format!("{}/api/v1/catalog", cfg.server_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build()?;
    Ok(client.get(&url).send()?.json::<Vec<CatalogItem>>()?)
}

pub fn cmd_list(cfg: AgentConfig, _interactive: bool) -> Result<()> {
    let items = fetch_all(&cfg)?;
    println!("{:<32} {:<10} {:<14} {}", "TITLE", "SYSTEM", "TYPE", "LICENSE");
    for it in items {
        println!("{:<32} {:<10} {:<14} {}",
            truncate(&it.title, 32),
            it.system.map(|s| format!("{:?}", s).to_lowercase()).unwrap_or_default(),
            format!("{:?}", it.r#type).to_lowercase(),
            it.license);
    }
    Ok(())
}

pub fn cmd_search(cfg: AgentConfig, term: &str) -> Result<()> {
    let term_l = term.to_lowercase();
    for it in fetch_all(&cfg)?.into_iter().filter(|i| i.title.to_lowercase().contains(&term_l) || i.tags.iter().any(|t| t.to_lowercase().contains(&term_l))) {
        println!("- {} [{}]", it.title, it.id);
    }
    Ok(())
}

pub fn cmd_download(cfg: AgentConfig, id: &str) -> Result<()> {
    let items = fetch_all(&cfg)?;
    let item = items.into_iter().find(|i| i.id == id).ok_or_else(|| anyhow::anyhow!("catalog item not found: {id}"))?;
    let url = item.download_url.clone().ok_or_else(|| anyhow::anyhow!("no download_url"))?;
    let install_path = item.install_path.clone().unwrap_or_else(|| format!("/roms/.playora/cache/{}", item.id));
    let dest = std::path::Path::new(&install_path);
    if let Some(p) = dest.parent() { std::fs::create_dir_all(p)?; }
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(120)).build()?;
    let bytes = client.get(&url).send()?.bytes()?;
    if let Some(expected) = &item.sha256 {
        use sha2::{Digest, Sha256};
        let got = hex::encode(Sha256::digest(&bytes));
        if &got != expected {
            anyhow::bail!("sha256 mismatch: expected={} got={}", expected, got);
        }
    }
    std::fs::write(dest, &bytes)?;
    println!("downloaded {} -> {}", item.title, dest.display());
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n-1).collect::<String>()) }
}
