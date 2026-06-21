use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::time::Duration;

const DEFAULT_URL: &str = "http://localhost:8080/jsonrpc";

pub struct KodiClient {
    url: String,
    http: reqwest::blocking::Client,
}

impl KodiClient {
    pub fn new(url: Option<&str>) -> Self {
        Self {
            url: url.unwrap_or(DEFAULT_URL).into(),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }

    fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let r = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .with_context(|| format!("POST {}", self.url))?;
        let v: serde_json::Value = r.json()?;
        if let Some(e) = v.get("error") {
            return Err(anyhow!("kodi error: {e}"));
        }
        Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    pub fn ping(&self) -> Result<String> {
        let r = self.call("JSONRPC.Ping", json!({}))?;
        Ok(r.to_string())
    }

    pub fn install_addon(&self, addon_id: &str) -> Result<()> {
        self.call("Addons.ExecuteAddon", json!({"addonid": addon_id}))
            .map(|_| ())
    }

    pub fn list_addons(&self, kind: &str) -> Result<serde_json::Value> {
        self.call(
            "Addons.GetAddons",
            json!({"type": kind, "enabled": "all", "properties": ["name", "version", "summary"]}),
        )
    }
}

pub fn cmd_setup() -> Result<()> {
    println!("Playora — Kodi Setup");
    println!("(make sure Kodi is running and Settings → Services → Allow remote control via HTTP is ON, port 8080)");
    println!();

    let client = KodiClient::new(None);
    println!("Ping Kodi...");
    let p = client
        .ping()
        .context("Kodi not reachable. Open Kodi first, enable Web server in Services.")?;
    println!("  -> {p}");

    println!();
    println!("Installed video add-ons:");
    let v = client
        .list_addons("xbmc.addon.video")
        .unwrap_or_else(|_| json!({"addons":[]}));
    if let Some(arr) = v.get("addons").and_then(|x| x.as_array()) {
        for a in arr {
            let name = a.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let id = a.get("addonid").and_then(|x| x.as_str()).unwrap_or("?");
            let enabled = a.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false);
            println!("  [{}] {} ({})", if enabled { "x" } else { " " }, name, id);
        }
    }

    println!();
    println!("Recommended legal add-ons:");
    println!("  plugin.video.youtube         — YouTube");
    println!("  plugin.video.plutotv         — Pluto TV (free live TV)");
    println!("  plugin.video.tubitv          — Tubi");
    println!("  plugin.video.jellyfin        — Jellyfin (self-hosted media)");
    println!("  pvr.iptvsimple               — IPTV (point at your M3U)");
    println!();
    println!("To install an add-on now, run: playora-agent kodi install <addon-id>");
    println!("Note: add-ons must exist in Kodi's enabled repository. Default repo covers YouTube, Plex, Jellyfin.");
    Ok(())
}

pub fn cmd_install(addon_id: &str) -> Result<()> {
    let client = KodiClient::new(None);
    client.install_addon(addon_id)?;
    println!("triggered install for {addon_id}");
    println!("Open Kodi to complete confirmation prompts if any.");
    Ok(())
}
