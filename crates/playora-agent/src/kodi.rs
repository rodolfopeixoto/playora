use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::time::Duration;

const DEFAULT_URL: &str = "http://localhost:8080/jsonrpc";

/// Curated set of legal, official-repo addons we always try to enable.
/// Format: (addon_id, human_name, group)
const RECOMMENDED: &[(&str, &str, &str)] = &[
    ("plugin.video.youtube", "YouTube", "video"),
    ("plugin.video.tubitv", "Tubi", "video"),
    ("plugin.video.jellyfin", "Jellyfin", "video"),
    ("pvr.iptvsimple", "IPTV Simple Client (PVR)", "pvr"),
    ("pvr.hts", "Tvheadend HTSP (PVR)", "pvr"),
    (
        "plugin.program.iagl",
        "Internet Archive Game Launcher",
        "game",
    ),
    ("plugin.video.plutotv", "Pluto TV", "video"),
];

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
        // Trigger Kodi's built-in InstallAddon — pulls from enabled repos.
        self.call(
            "Addons.ExecuteAddon",
            json!({"addonid": addon_id, "wait": false}),
        )
        .or_else(|_| {
            self.call(
                "Input.ExecuteAction",
                json!({"action": format!("RunPlugin(plugin://{})", addon_id)}),
            )
        })
        .map(|_| ())
    }

    pub fn enable_addon(&self, addon_id: &str) -> Result<()> {
        self.call(
            "Addons.SetAddonEnabled",
            json!({"addonid": addon_id, "enabled": true}),
        )
        .map(|_| ())
    }

    pub fn addon_details(&self, addon_id: &str) -> Result<serde_json::Value> {
        self.call(
            "Addons.GetAddonDetails",
            json!({"addonid": addon_id, "properties": ["name", "enabled", "installed"]}),
        )
    }

    pub fn list_addons(&self, kind: &str) -> Result<serde_json::Value> {
        self.call(
            "Addons.GetAddons",
            json!({"type": kind, "enabled": "all", "properties": ["name", "version", "summary", "enabled"]}),
        )
    }
}

pub fn cmd_setup() -> Result<()> {
    println!("Playora — Kodi Setup");
    println!("(Kodi: Settings → Services → Allow remote control via HTTP must be ON, port 8080)");
    println!();

    let client = KodiClient::new(None);
    println!("ping kodi...");
    let p = client
        .ping()
        .context("Kodi not reachable. Open Kodi first, enable Web server in Services.")?;
    println!("  -> {p}");
    println!();

    let mut enabled = 0;
    let mut missing = Vec::new();
    let mut already = 0;
    for (id, name, group) in RECOMMENDED {
        match client.addon_details(id) {
            Ok(v) => {
                let is_enabled = v
                    .get("addon")
                    .and_then(|a| a.get("enabled"))
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                if is_enabled {
                    println!("  [x] {name:30} ({id}) [{group}] already enabled");
                    already += 1;
                    continue;
                }
                match client.enable_addon(id) {
                    Ok(_) => {
                        println!("  [+] {name:30} ({id}) [{group}] ENABLED");
                        enabled += 1;
                    }
                    Err(e) => {
                        println!("  [!] {name:30} ({id}) [{group}] enable failed: {e}");
                        missing.push((id, name, group));
                    }
                }
            }
            Err(_) => {
                // Not installed locally. Try kodi's installer.
                match client.install_addon(id) {
                    Ok(_) => println!("  [↓] {name:30} ({id}) [{group}] install triggered"),
                    Err(_) => {
                        println!("  [?] {name:30} ({id}) [{group}] not installed");
                        missing.push((id, name, group));
                    }
                }
            }
        }
    }

    println!();
    println!("== Kodi setup summary ==");
    println!("enabled now:       {enabled}");
    println!("already enabled:   {already}");
    println!("missing/unreachable:{}", missing.len());
    if !missing.is_empty() {
        println!();
        println!("To install missing addons, open Kodi:");
        println!("  Settings → Add-ons → Install from repository → Kodi Add-on repository");
        for (id, name, group) in &missing {
            println!("  - {name} ({id}) [{group}]");
        }
    }
    Ok(())
}

pub fn cmd_install(addon_id: &str) -> Result<()> {
    let client = KodiClient::new(None);
    if client.enable_addon(addon_id).is_ok() {
        println!("enabled {addon_id}");
        return Ok(());
    }
    client.install_addon(addon_id)?;
    println!("triggered install for {addon_id}");
    Ok(())
}
