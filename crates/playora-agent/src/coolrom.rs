//! CoolROM downloader — port to Rust.
//!
//! Author of this Rust port: Rodolfo Peixoto (rodolfog.peixoto@gmail.com), 2026.
//! Inspired by the Python prototype by Victor Oliveira (WTFPL, 2018).
//! Implementation, error handling, streaming and CLI/TUI are original here.
//!
//! Use responsibly. Respect site ToS and copyright in your jurisdiction.

use anyhow::{anyhow, Context, Result};
use scraper::{Html, Selector};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

const BASE_URL: &str = "http://coolrom.com.au";
const USER_AGENT: &str = "playora-coolrom/0.1 (+https://github.com/ropeixoto/playora)";
const CHUNK: usize = 64 * 1024;

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(USER_AGENT)
        .build()?)
}

pub fn list_consoles() -> Result<Vec<String>> {
    let url = format!("{BASE_URL}/roms/");
    let body = client()?.get(&url).send()?.error_for_status()?.text()?;
    let doc = Html::parse_document(&body);
    let sel = Selector::parse("a[href^=\"/roms/\"]").unwrap();
    let mut out = Vec::new();
    for a in doc.select(&sel) {
        if let Some(href) = a.value().attr("href") {
            let trimmed = href.trim_start_matches("/roms/").trim_end_matches('/');
            if !trimmed.is_empty() && !trimmed.contains('/') && !out.iter().any(|x| x == trimmed) {
                out.push(trimmed.to_string());
            }
        }
    }
    if out.is_empty() {
        return Err(anyhow!(
            "no consoles parsed from {url} (site layout changed?)"
        ));
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct RomEntry {
    pub name: String,
    pub url_path: String,
}

pub fn list_roms(console: &str, letter: char) -> Result<Vec<RomEntry>> {
    let url = format!("{BASE_URL}/roms/{console}/{letter}/");
    let body = client()?.get(&url).send()?.error_for_status()?.text()?;
    let doc = Html::parse_document(&body);
    let sel = Selector::parse("a[href*=\".php\"]").unwrap();
    let mut out = Vec::new();
    for a in doc.select(&sel) {
        let Some(href) = a.value().attr("href") else {
            continue;
        };
        if !href.contains(&format!("/roms/{console}/")) || !href.ends_with(".php") {
            continue;
        }
        let name = href
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(".php")
            .replace('_', " ");
        if name.is_empty() {
            continue;
        }
        out.push(RomEntry {
            name,
            url_path: href.to_string(),
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.name == b.name);
    Ok(out)
}

pub fn download(
    rom: &RomEntry,
    dest_dir: &Path,
    progress: impl Fn(u64, Option<u64>),
) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let parts: Vec<&str> = rom.url_path.split('/').collect();
    let rom_id = parts.get(3).ok_or_else(|| anyhow!("bad rom path"))?;
    let popup_url = format!("{BASE_URL}/dlpop.php?id={rom_id}");

    let c = client()?;
    let popup_body = c.get(&popup_url).send()?.error_for_status()?.text()?;
    let popup_doc = Html::parse_document(&popup_body);
    let form_sel = Selector::parse("form[action]").unwrap();
    let form_action = popup_doc
        .select(&form_sel)
        .next()
        .and_then(|f| f.value().attr("action"))
        .ok_or_else(|| anyhow!("download form not found"))?
        .to_string();

    let referer = format!("{BASE_URL}{}", rom.url_path);
    let mut resp = c
        .get(&form_action)
        .header("Referer", &referer)
        .send()?
        .error_for_status()?;

    let total = resp.content_length();
    let filename = resp
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(extract_filename)
        .unwrap_or_else(|| format!("{}.zip", rom.name.replace(' ', "_")));
    let dest = dest_dir.join(&filename);

    let mut file = std::fs::File::create(&dest).with_context(|| dest.display().to_string())?;
    let mut buf = vec![0u8; CHUNK];
    let mut downloaded: u64 = 0;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        progress(downloaded, total);
    }
    Ok(dest)
}

fn extract_filename(cd: &str) -> Option<String> {
    cd.split(';')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("filename="))
        .map(|s| s.trim_matches('"').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filename_from_content_disposition() {
        assert_eq!(
            extract_filename("attachment; filename=\"Pokemon Crystal.zip\""),
            Some("Pokemon Crystal.zip".into())
        );
        assert_eq!(extract_filename("inline"), None);
    }
}
