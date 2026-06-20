//! Myrient (No-Intro / Redump mirror) directory index crawler.
//!
//! Apache-style indexes; light HTML parse for `<a href>` filenames.

use anyhow::{anyhow, Result};
use scraper::{Html, Selector};
use std::time::Duration;

const USER_AGENT: &str = "playora-myrient/0.1 (+https://github.com/ropeixoto/playora)";

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(USER_AGENT)
        .build()?)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexEntry {
    pub name: String,
    pub url: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
}

pub fn list_index(url: &str) -> Result<Vec<IndexEntry>> {
    let body = client()?.get(url).send()?.error_for_status()?.text()?;
    let doc = Html::parse_document(&body);
    let sel = Selector::parse("a[href]").unwrap();
    let mut out = Vec::new();
    let base = url.trim_end_matches('/').to_string();
    for a in doc.select(&sel) {
        let href = a.value().attr("href").unwrap_or("");
        if href.is_empty() || href.starts_with('?') || href == "../" || href.starts_with('#') {
            continue;
        }
        let absolute = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{base}/{}", href.trim_start_matches('/'))
        };
        let name = href.trim_end_matches('/').to_string();
        let is_dir = href.ends_with('/');
        out.push(IndexEntry {
            name,
            url: absolute,
            is_dir,
            size_bytes: None,
        });
    }
    if out.is_empty() {
        return Err(anyhow!("nothing parsed at {url}"));
    }
    Ok(out)
}

pub fn search(url: &str, query: &str) -> Result<Vec<IndexEntry>> {
    let q = query.to_lowercase();
    Ok(list_index(url)?
        .into_iter()
        .filter(|e| e.name.to_lowercase().contains(&q))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apache_like_index() {
        let html = r#"<html><body>
            <a href="../">Parent</a>
            <a href="No-Intro/">No-Intro/</a>
            <a href="Redump/">Redump/</a>
            <a href="readme.txt">readme.txt</a>
        </body></html>"#;
        let doc = Html::parse_document(html);
        let sel = Selector::parse("a[href]").unwrap();
        let n = doc.select(&sel).count();
        assert_eq!(n, 4);
    }
}
