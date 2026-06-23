use crate::State;
use axum::{
    extract::{Path as AxPath, State as AxState},
    response::{Html, Redirect},
    Form,
};
use chrono::Utc;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct DeleteRomForm {
    pub rom_path: String,
}

#[derive(Deserialize)]
pub struct CloudTokenForm {
    pub token: String,
}

pub async fn cloud_setup_submit(
    AxState(state): AxState<State>,
    AxPath(device_id): AxPath<String>,
    Form(form): Form<CloudTokenForm>,
) -> Redirect {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT OR REPLACE INTO cloud_auth_tokens(device_id, token, consumed_at, received_at) VALUES (?1, ?2, NULL, ?3)",
        rusqlite::params![device_id, form.token, chrono::Utc::now().to_rfc3339()],
    );
    Redirect::to(&format!("/dashboard/cloud-setup/{device_id}?submitted=1"))
}

pub async fn cloud_setup_page(
    AxState(state): AxState<State>,
    AxPath(device_id): AxPath<String>,
) -> Html<String> {
    // Pull the latest authorize command from this device's most recent
    // Cloud Setup activity event so the dashboard can show the exact
    // blob the user has to paste into `rclone authorize`.
    let conn = state.lock().await;
    let summary: Option<String> = conn
        .query_row(
            "SELECT summary FROM activities WHERE device_id=?1 AND script='Cloud Setup' ORDER BY id DESC LIMIT 1",
            [device_id.clone()],
            |r| r.get(0),
        )
        .ok();
    let consumed: Option<String> = conn
        .query_row(
            "SELECT consumed_at FROM cloud_auth_tokens WHERE device_id=?1",
            [device_id.clone()],
            |r| r.get(0),
        )
        .ok();

    let auth_cmd = summary
        .as_deref()
        .and_then(|s| s.split("AUTH_CMD:").nth(1).map(|s| s.trim().to_string()))
        .unwrap_or_default();

    let status_block = if let Some(t) = consumed {
        format!(
            "<div class=\"ok-banner\">✓ Token received (consumed at <code>{}</code>). The agent has written rclone.conf.</div>",
            esc(&t)
        )
    } else if auth_cmd.is_empty() {
        "<div class=\"warn-banner\">No active Cloud Setup. Click <code>Ports → Playora Cloud Setup</code> on the console first, then refresh this page.</div>".to_string()
    } else {
        String::new()
    };

    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Cloud Setup · Playora</title>
<style>{css}
.ok-banner{{background:#0f2818;color:#5fbf76;padding:14px;border-radius:8px;margin:14px 0;font-size:14px}}
.warn-banner{{background:#2a1f0a;color:#d4a648;padding:14px;border-radius:8px;margin:14px 0;font-size:14px}}
.step{{margin:18px 0;padding:14px;border:1px solid #1f1f26;border-radius:8px;background:#101015}}
.step h3{{margin:0 0 8px 0;font-size:14px;color:#9aa}}
pre.cmd{{background:#0a0a0a;border:1px solid #1f1f1f;padding:12px;border-radius:6px;overflow:auto;font-size:12px;color:#9ad;white-space:pre-wrap;word-break:break-all}}
textarea{{width:100%;min-height:140px;background:#0a0a0a;color:#cfcfcf;border:1px solid #1f1f1f;border-radius:6px;padding:10px;font-family:monospace;font-size:12px}}
button.submit{{background:#1a3d5c;color:#7c9eff;border:1px solid #2a5078;border-radius:6px;padding:10px 20px;cursor:pointer;font-size:13px;margin-top:10px}}
button.submit:hover{{background:#234e75}}
.qr-box{{text-align:center;padding:20px;background:#fff;border-radius:8px;display:inline-block}}
.qr-box img{{display:block;max-width:240px}}
</style></head>
<body><div class="wrap">
{hdr}
<h1>Cloud Setup — Google Drive</h1>
<p class="sub">Device <code>{did}</code></p>
{status_block}
<p>This device has no browser, so authorization happens on your computer (any machine with <code>rclone</code> installed). Follow the steps below — your phone or PC works equally well.</p>

<div class="step">
<h3>Step 1 · Install rclone on your PC (one-time)</h3>
<pre class="cmd">brew install rclone     # macOS
sudo apt install rclone  # Debian/Ubuntu
winget install Rclone.Rclone  # Windows</pre>
</div>

<div class="step">
<h3>Step 2 · On your PC, run the command below</h3>
<p class="muted">This opens your browser, signs in to Google, and prints a JSON token.</p>
<pre class="cmd">{auth_cmd_html}</pre>
</div>

<div class="step">
<h3>Step 3 · Paste the JSON token here</h3>
<form method="post" action="/dashboard/cloud-setup/{did}">
    <textarea name="token" placeholder='{{"access_token":"...","token_type":"Bearer","refresh_token":"...","expiry":"..."}}'></textarea>
    <button class="submit" type="submit">Send token to device</button>
</form>
</div>

<div class="step">
<h3>Step 4 · Wait ~5 seconds</h3>
<p class="muted">The agent polls this server every 5s. Once the token arrives, rclone writes its config and the Cloud Setup activity finishes with status <span class="pill ok">ok</span>. Then your <code>Cloud Backup</code> / <code>Cloud Restore</code> ports will work.</p>
</div>

</div></body></html>"#,
        css = CSS,
        hdr = header("devices"),
        did = esc(&device_id),
        status_block = status_block,
        auth_cmd_html = if auth_cmd.is_empty() {
            "(waiting — start <em>Playora Cloud Setup</em> on the console)".to_string()
        } else {
            esc(&auth_cmd)
        }
    );
    Html(html)
}

#[derive(Deserialize)]
pub struct CloudDownloadForm {
    pub rel_path: String,
}

pub async fn cloud_download_form(
    AxState(state): AxState<State>,
    AxPath(device_id): AxPath<String>,
    Form(form): Form<CloudDownloadForm>,
) -> Redirect {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO cloud_download_requests(device_id, rel_path, status, requested_at) VALUES (?1, ?2, 'pending', ?3)",
        rusqlite::params![device_id, form.rel_path, chrono::Utc::now().to_rfc3339()],
    );
    Redirect::to(&format!("/dashboard/cloud-roms/{device_id}?queued=1"))
}

pub async fn update_request_form(
    AxState(state): AxState<State>,
    AxPath(device_id): AxPath<String>,
) -> Redirect {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO update_requests(device_id, status, requested_at) VALUES (?1, 'pending', ?2)",
        rusqlite::params![device_id, chrono::Utc::now().to_rfc3339()],
    );
    Redirect::to(&format!("/dashboard/device/{device_id}?update=queued"))
}

pub async fn cloud_roms_page(
    AxState(state): AxState<State>,
    AxPath(device_id): AxPath<String>,
) -> Html<String> {
    let conn = state.lock().await;
    // Load catalog grouped by system.
    let mut stmt = conn
        .prepare(
            "SELECT rel_path, system, name, size FROM cloud_catalog WHERE device_id=?1 ORDER BY system, name",
        )
        .unwrap();
    let rows = stmt
        .query_map([device_id.clone()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .unwrap();

    let mut by_system: std::collections::BTreeMap<String, Vec<(String, String, i64)>> =
        std::collections::BTreeMap::new();
    for (rel, sys, name, size) in rows.flatten() {
        by_system.entry(sys).or_default().push((rel, name, size));
    }

    // Pending download queue.
    let mut pend = std::collections::BTreeMap::<String, String>::new();
    let mut stmt = conn
        .prepare(
            "SELECT rel_path, status FROM cloud_download_requests WHERE device_id=?1 ORDER BY id DESC",
        )
        .unwrap();
    for r in stmt
        .query_map([device_id.clone()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .unwrap()
        .flatten()
    {
        pend.entry(r.0).or_insert(r.1);
    }

    let mut sections = String::new();
    if by_system.is_empty() {
        sections.push_str(
            "<p class=\"sub muted\">Catalog empty. On the device run <code>Ports → Playora Cloud Catalog</code> (or wait for nightly refresh).</p>",
        );
    }
    for (sys, items) in &by_system {
        sections.push_str(&format!(
            "<h2>{} <span class=\"muted\">({} ROMs)</span></h2><table><thead><tr><th>Name</th><th>Size</th><th>Status</th><th></th></tr></thead><tbody>",
            esc(sys),
            items.len()
        ));
        for (rel, name, size) in items {
            let status = pend.get(rel).cloned().unwrap_or_default();
            let pill_class = match status.as_str() {
                "ok" => "pill ok",
                "fail" => "pill err",
                "pending" => "pill warn",
                _ => "pill",
            };
            let pill_text = if status.is_empty() {
                String::from("cloud only")
            } else {
                status
            };
            let action = format!(
                r#"<form method="post" action="/dashboard/cloud-roms/{did}" style="display:inline">
                    <input type="hidden" name="rel_path" value="{path}">
                    <button class="dl" type="submit">Download</button>
                </form>"#,
                did = esc(&device_id),
                path = esc(rel)
            );
            sections.push_str(&format!(
                "<tr><td>{}</td><td class=\"muted\">{:.1} MB</td><td><span class=\"{}\">{}</span></td><td>{}</td></tr>",
                esc(name),
                *size as f64 / 1024.0 / 1024.0,
                pill_class,
                esc(&pill_text),
                action
            ));
        }
        sections.push_str("</tbody></table>");
    }

    Html(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Cloud ROMs · Playora</title>
<meta http-equiv="refresh" content="30">
<style>{css}
.pill.ok{{background:#0f2818;color:#5fbf76}}.pill.warn{{background:#2a1f0a;color:#d4a648}}.pill.err{{background:#2a0f0f;color:#d65656}}
button.dl{{background:#1a3d5c;color:#7c9eff;border:1px solid #2a5078;border-radius:6px;padding:4px 12px;cursor:pointer;font-size:11px}}
button.dl:hover{{background:#234e75}}
</style></head>
<body><div class="wrap">
{hdr}
<h1>Cloud ROMs</h1>
<p class="sub">Device <code>{did}</code> · Click Download to queue. Agent picks it up within 60 s and runs <code>rclone copy gdrive:R36S/roms/&lt;name&gt; /roms/&lt;system&gt;/</code>.</p>
{sections}
</div></body></html>"#,
        css = CSS,
        hdr = header("devices"),
        did = esc(&device_id),
    ))
}

pub async fn delete_rom_form(
    AxState(state): AxState<State>,
    AxPath(device_id): AxPath<String>,
    Form(form): Form<DeleteRomForm>,
) -> Redirect {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO delete_requests(device_id, rom_path, status, requested_at) VALUES (?1, ?2, 'pending', ?3)",
        rusqlite::params![device_id, form.rom_path, Utc::now().to_rfc3339()],
    );
    Redirect::to(&format!("/dashboard/device/{device_id}"))
}

const CSS: &str = r#"
*{box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Inter','Segoe UI',Roboto,sans-serif;background:#0a0a0d;color:#e6e6ea;margin:0;padding:0;min-height:100vh}
.wrap{max-width:1200px;margin:0 auto;padding:24px}
header{display:flex;align-items:center;justify-content:space-between;border-bottom:1px solid #1f1f26;padding-bottom:14px;margin-bottom:24px}
header .brand{display:flex;align-items:center;gap:12px;font-size:18px;font-weight:600}
header .brand .dot{width:10px;height:10px;border-radius:50%;background:linear-gradient(135deg,#7c4dff,#42a5f5)}
header nav a{color:#9aa;text-decoration:none;margin-left:18px;font-size:13px;letter-spacing:.3px}
header nav a.active,header nav a:hover{color:#fff}
h1{font-size:24px;margin:0 0 4px 0;font-weight:600;letter-spacing:-.3px}
.sub{color:#666;font-size:13px;margin:0 0 20px 0}
h2{font-size:11px;color:#7a7a85;margin:28px 0 10px;text-transform:uppercase;letter-spacing:1.2px;font-weight:600}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:12px;margin:8px 0 24px}
.card{background:linear-gradient(180deg,#15151b,#101015);border:1px solid #1f1f26;border-radius:10px;padding:18px;transition:transform .15s,border-color .15s}
.card:hover{transform:translateY(-1px);border-color:#2a2a35}
.card .l{color:#666;font-size:10px;text-transform:uppercase;letter-spacing:1px;font-weight:600}
.card .v{font-size:26px;font-weight:600;margin-top:6px;color:#fff;letter-spacing:-.5px}
table{width:100%;border-collapse:separate;border-spacing:0;background:#101015;border:1px solid #1f1f26;border-radius:10px;overflow:hidden;font-size:13px}
th,td{padding:11px 14px;text-align:left;vertical-align:middle;border-bottom:1px solid #1a1a1f}
tr:last-child td{border-bottom:none}
th{color:#666;font-weight:500;font-size:11px;text-transform:uppercase;letter-spacing:.5px;background:#0d0d12}
tbody tr:hover td{background:#13131a}
a{color:#7c9eff;text-decoration:none}
a:hover{text-decoration:underline}
code{color:#9ad;font-family:'JetBrains Mono','SF Mono',ui-monospace,monospace;font-size:12px}
.pill{display:inline-block;padding:3px 10px;border-radius:12px;font-size:10px;background:#1f1f2a;color:#9aa;text-transform:uppercase;letter-spacing:.5px;font-weight:600}
.empty{padding:32px;text-align:center;color:#555;font-size:13px}
.muted{color:#555;font-size:12px}
footer{color:#444;font-size:11px;margin-top:48px;padding-top:16px;border-top:1px solid #1a1a1f;text-align:center}
.row2{display:grid;grid-template-columns:1fr 1fr;gap:16px}
@media(max-width:700px){.row2{grid-template-columns:1fr}}
.bar{height:6px;background:#1a1a22;border-radius:3px;overflow:hidden;margin-top:6px}
.bar>div{height:100%;background:linear-gradient(90deg,#7c4dff,#42a5f5)}
button.del{background:#3a0a0a;color:#ff7676;border:1px solid #4a1414;border-radius:6px;padding:5px 10px;cursor:pointer;font-size:11px}
button.del:hover{background:#4a1010;color:#fff}
button.upd{background:#0f2818;color:#5fbf76;border:1px solid #1f3d28;border-radius:6px;padding:4px 10px;cursor:pointer;font-size:11px}
button.upd:hover{background:#1a3a25}
.pill.playing{background:#1a3d5c;color:#7c9eff;animation:pulse 2s infinite}
@keyframes pulse{0%,100%{opacity:1}50%{opacity:.65}}
"#;

fn header(active: &str) -> String {
    let mark = |k: &str| if k == active { "active" } else { "" };
    format!(
        r#"<header>
            <div class="brand"><span class="dot"></span>Playora Hub</div>
            <nav>
                <a class="{}" href="/dashboard">Overview</a>
                <a class="{}" href="/dashboard/devices">Devices</a>
                <a class="{}" href="/dashboard/games">Games</a>
                <a class="{}" href="/dashboard/activity">Activity</a>
            </nav>
        </header>"#,
        mark("overview"),
        mark("devices"),
        mark("games"),
        mark("activity")
    )
}

pub async fn page(AxState(state): AxState<State>) -> Html<String> {
    let conn = state.lock().await;
    let devices: i64 = conn
        .query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))
        .unwrap_or(0);
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap_or(0);
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM game_sessions", [], |r| r.get(0))
        .unwrap_or(0);
    let snapshots: i64 = conn
        .query_row("SELECT COUNT(*) FROM hardware_snapshots", [], |r| r.get(0))
        .unwrap_or(0);
    let total_play: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds),0) FROM game_sessions",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let unique_games: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT game_name) FROM game_sessions WHERE game_name IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let unique_systems: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT system) FROM game_sessions WHERE system IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let last_hb: String = conn
        .query_row(
            "SELECT received_at FROM heartbeats ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "—".into());

    let mut devices_html = String::new();
    let mut stmt = conn.prepare("SELECT device_id, COALESCE(device_name,'?'), COALESCE(device_profile,'?'), COALESCE(last_seen_at,'') FROM devices ORDER BY last_seen_at DESC LIMIT 25").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .unwrap();
    for (id, name, profile, seen) in rows.flatten() {
        let did = esc(&id);
        devices_html.push_str(&format!(
            "<tr><td><a href=\"/dashboard/device/{}\">{}</a></td><td><span class=\"pill\">{}</span></td><td><code>{}</code></td><td class=\"muted\">{}</td></tr>",
            did, esc(&name), esc(&profile), did, esc(&relative_time(&seen))
        ));
    }
    if devices_html.is_empty() {
        devices_html.push_str("<tr><td colspan=4 class=\"empty\">No devices yet. Open <code>Ports → Playora Doctor</code> on the console.</td></tr>");
    }

    let mut top_games = String::new();
    let mut stmt = conn.prepare("SELECT game_name, system, SUM(duration_seconds) FROM game_sessions WHERE game_name IS NOT NULL GROUP BY game_name, system ORDER BY 3 DESC LIMIT 10").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .unwrap();
    let collected: Vec<_> = rows.flatten().collect();
    let max_dur = collected.iter().map(|r| r.2).max().unwrap_or(1).max(1);
    for (game, system, dur) in &collected {
        let pct = (*dur as f64 / max_dur as f64 * 100.0) as u32;
        top_games.push_str(&format!(
            "<tr><td>{}</td><td><span class=\"pill\">{}</span></td><td>{}<div class=\"bar\"><div style=\"width:{}%\"></div></div></td></tr>",
            esc(game), esc(system), fmt_dur(*dur), pct
        ));
    }
    if top_games.is_empty() {
        top_games.push_str("<tr><td colspan=3 class=\"empty\">No play sessions yet.</td></tr>");
    }

    let mut top_systems = String::new();
    let mut stmt = conn.prepare("SELECT system, COUNT(*), SUM(duration_seconds) FROM game_sessions WHERE system IS NOT NULL GROUP BY system ORDER BY 3 DESC LIMIT 10").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .unwrap();
    let sys_collected: Vec<_> = rows.flatten().collect();
    let max_sys = sys_collected.iter().map(|r| r.2).max().unwrap_or(1).max(1);
    for (system, count, dur) in &sys_collected {
        let pct = (*dur as f64 / max_sys as f64 * 100.0) as u32;
        top_systems.push_str(&format!(
            "<tr><td><span class=\"pill\">{}</span></td><td>{}</td><td>{}<div class=\"bar\"><div style=\"width:{}%\"></div></div></td></tr>",
            esc(system), count, fmt_dur(*dur), pct
        ));
    }
    if top_systems.is_empty() {
        top_systems.push_str("<tr><td colspan=3 class=\"empty\">No system data yet.</td></tr>");
    }

    let mut events_html = String::new();
    let mut stmt = conn
        .prepare("SELECT event_type, device_id, received_at FROM events ORDER BY id DESC LIMIT 12")
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap();
    for (typ, did, recv) in rows.flatten() {
        events_html.push_str(&format!(
            "<tr><td><span class=\"pill\">{}</span></td><td><a href=\"/dashboard/device/{}\"><code>{}</code></a></td><td class=\"muted\">{}</td></tr>",
            esc(&typ), esc(&did), esc(&did), esc(&relative_time(&recv))
        ));
    }
    if events_html.is_empty() {
        events_html.push_str("<tr><td colspan=3 class=\"empty\">No events yet.</td></tr>");
    }

    let mut activity_html = String::new();
    let mut stmt = conn.prepare("SELECT id, script, status, started_at, COALESCE(ended_at,''), COALESCE(exit_code,-1), COALESCE(summary,'') FROM activities ORDER BY id DESC LIMIT 8").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .unwrap();
    for (id, script, status, started, ended, code, summary) in rows.flatten() {
        let pill_class = match status.as_str() {
            "ok" => "pill ok",
            "fail" => "pill err",
            _ => "pill warn",
        };
        let when_label = if ended.is_empty() {
            format!("started {}", relative_time(&started))
        } else {
            format!("finished {}", relative_time(&ended))
        };
        activity_html.push_str(&format!(
            "<tr><td><a href=\"/dashboard/activity/{id}\">{}</a></td><td><span class=\"{}\">{}</span></td><td class=\"muted\">{}</td><td class=\"muted\">{}</td><td class=\"muted\">{}</td></tr>",
            esc(&script), pill_class, esc(&status), esc(&summary), esc(&when_label),
            if code >= 0 { format!("exit {code}") } else { String::new() }
        ));
    }
    if activity_html.is_empty() {
        activity_html.push_str("<tr><td colspan=5 class=\"empty\">No menu activity yet. Open <code>Ports → Playora Doctor</code> on the device.</td></tr>");
    }

    let html = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Playora Hub</title>
<meta http-equiv="refresh" content="10">
<style>{css}.pill.ok{{background:#0f2818;color:#5fbf76}}.pill.warn{{background:#2a1f0a;color:#d4a648}}.pill.err{{background:#2a0f0f;color:#d65656}}</style></head>
<body><div class="wrap">
{hdr}
<h1>Overview</h1>
<p class="sub">Auto-refresh every 10s · last heartbeat: <code>{last_hb}</code></p>

<h2>Recent activity</h2>
<table><thead><tr><th>Script</th><th>Status</th><th>Summary</th><th>When</th><th></th></tr></thead><tbody>{activity_html}</tbody></table>

<div class="grid">
    <div class="card"><div class="l">Devices</div><div class="v">{devices}</div></div>
    <div class="card"><div class="l">Sessions</div><div class="v">{sessions}</div></div>
    <div class="card"><div class="l">Total playtime</div><div class="v">{play}</div></div>
    <div class="card"><div class="l">Unique games</div><div class="v">{unique_games}</div></div>
    <div class="card"><div class="l">Systems played</div><div class="v">{unique_systems}</div></div>
    <div class="card"><div class="l">Events received</div><div class="v">{events}</div></div>
    <div class="card"><div class="l">HW snapshots</div><div class="v">{snapshots}</div></div>
</div>

<div class="row2">
    <div>
        <h2>Top games by playtime</h2>
        <table><thead><tr><th>Game</th><th>System</th><th>Total time</th></tr></thead><tbody>{top_games}</tbody></table>
    </div>
    <div>
        <h2>Top systems</h2>
        <table><thead><tr><th>System</th><th>Sessions</th><th>Total time</th></tr></thead><tbody>{top_systems}</tbody></table>
    </div>
</div>

<h2>Devices</h2>
<table><thead><tr><th>Name</th><th>Profile</th><th>ID</th><th>Last seen</th></tr></thead><tbody>{devices_html}</tbody></table>

<h2>Latest events</h2>
<table><thead><tr><th>Type</th><th>Device</th><th>Received</th></tr></thead><tbody>{events_html}</tbody></table>

<footer>Playora · {now}</footer>
</div></body></html>"#,
        css = CSS,
        hdr = header("overview"),
        play = fmt_dur(total_play),
        now = Utc::now().format("%Y-%m-%d %H:%M UTC"),
    );
    Html(html)
}

pub async fn devices_list_page(AxState(state): AxState<State>) -> Html<String> {
    let conn = state.lock().await;
    let mut rows_html = String::new();
    let mut stmt = conn.prepare("SELECT device_id, COALESCE(device_name,'?'), COALESCE(device_profile,'?'), COALESCE(agent_version,''), COALESCE(last_seen_at,'') FROM devices ORDER BY last_seen_at DESC").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .unwrap();
    for (id, name, profile, ver, seen) in rows.flatten() {
        let did = esc(&id);
        rows_html.push_str(&format!(
            "<tr><td><a href=\"/dashboard/device/{}\">{}</a></td><td><span class=\"pill\">{}</span></td><td><code>{}</code></td><td>{}</td><td class=\"muted\">{}</td></tr>",
            did, esc(&name), esc(&profile), did, esc(&ver), esc(&relative_time(&seen))
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan=5 class=\"empty\">No devices.</td></tr>");
    }
    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Devices · Playora</title><style>{css}</style></head><body><div class="wrap">{hdr}<h1>Devices</h1><table><thead><tr><th>Name</th><th>Profile</th><th>ID</th><th>Agent</th><th>Last seen</th></tr></thead><tbody>{rows_html}</tbody></table></div></body></html>"#,
        css = CSS,
        hdr = header("devices")
    );
    Html(html)
}

pub async fn activity_page(AxState(state): AxState<State>) -> Html<String> {
    let conn = state.lock().await;
    let mut running_banner = String::new();
    let mut running_stmt = conn
        .prepare(
            "SELECT script, device_id, started_at, COALESCE(summary,'') FROM activities
         WHERE status='running' ORDER BY id DESC LIMIT 10",
        )
        .unwrap();
    let runs: Vec<(String, String, String, String)> = running_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect();
    drop(running_stmt);
    if !runs.is_empty() {
        running_banner.push_str("<div class=\"running-banner\">");
        running_banner.push_str(&format!("<strong>▶ {} running</strong>", runs.len()));
        for (script, did, started, summary) in &runs {
            running_banner.push_str(&format!(
                "<div class=\"run-row\"><span class=\"pill warn\">{}</span> <span class=\"muted\">{} — started {} on <a href=\"/dashboard/device/{}\"><code>{}</code></a></span></div>",
                esc(script),
                esc(summary),
                esc(&relative_time(started)),
                esc(did),
                esc(did)
            ));
        }
        running_banner.push_str("</div>");
    }

    let mut rows_html = String::new();
    let mut stmt = conn.prepare("SELECT id, device_id, script, status, started_at, COALESCE(ended_at,''), COALESCE(exit_code,-1), COALESCE(summary,'') FROM activities ORDER BY id DESC LIMIT 200").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
            ))
        })
        .unwrap();
    for (id, did, script, status, started, ended, code, summary) in rows.flatten() {
        let pill_class = match status.as_str() {
            "ok" => "pill ok",
            "fail" => "pill err",
            _ => "pill warn",
        };
        let when_label = if ended.is_empty() {
            format!("started {}", relative_time(&started))
        } else {
            format!("finished {}", relative_time(&ended))
        };
        rows_html.push_str(&format!(
            "<tr><td><a href=\"/dashboard/activity/{id}\">{}</a></td><td><span class=\"{}\">{}</span></td><td class=\"muted\">{}</td><td><a href=\"/dashboard/device/{}\"><code>{}</code></a></td><td class=\"muted\">{}</td><td class=\"muted\">{}</td></tr>",
            esc(&script), pill_class, esc(&status),
            esc(&summary),
            esc(&did), esc(&did),
            esc(&when_label),
            if code >= 0 { format!("exit {code}") } else { String::new() }
        ));
    }
    if rows_html.is_empty() {
        rows_html
            .push_str("<tr><td colspan=6 class=\"empty\">No menu activity recorded yet.</td></tr>");
    }
    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Activity · Playora</title><meta http-equiv="refresh" content="10"><style>{css}
.pill.ok{{background:#0f2818;color:#5fbf76}}.pill.warn{{background:#2a1f0a;color:#d4a648}}.pill.err{{background:#2a0f0f;color:#d65656}}
.running-banner{{background:linear-gradient(90deg,#1a3d5c,#2a1f0a);border:1px solid #2a5078;border-radius:10px;padding:14px;margin:14px 0;font-size:13px}}
.running-banner strong{{display:block;margin-bottom:8px;color:#7c9eff}}
.running-banner .run-row{{margin-top:6px}}
</style></head><body><div class="wrap">{hdr}<h1>Activity</h1>{running_banner}<p class="sub">Every menu click on the console shows up here within seconds. Click a script name to see its log tail.</p><table><thead><tr><th>Script</th><th>Status</th><th>Summary</th><th>Device</th><th>When</th><th></th></tr></thead><tbody>{rows_html}</tbody></table></div></body></html>"#,
        css = CSS,
        hdr = header("activity")
    );
    Html(html)
}

pub async fn activity_detail_page(
    AxState(state): AxState<State>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Html<String> {
    let conn = state.lock().await;
    let row: Option<(String, String, String, String, String, i64, String, String, String)> = conn.query_row(
        "SELECT device_id, script, status, started_at, COALESCE(ended_at,''), COALESCE(exit_code,-1), COALESCE(log_path,''), COALESCE(summary,''), COALESCE(stdout_tail,'') FROM activities WHERE id=?1",
        [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?))
    ).ok();
    let Some((did, script, status, started, ended, code, log_path, summary, tail)) = row else {
        return Html(format!(
            r#"<!doctype html><html><head><meta charset="utf-8"><title>Not found</title><style>{css}</style></head><body><div class="wrap">{hdr}<h1>Activity not found</h1><p>id={id}</p></div></body></html>"#,
            css = CSS,
            hdr = header("activity")
        ));
    };
    let pill_class = match status.as_str() {
        "ok" => "pill ok",
        "fail" => "pill err",
        _ => "pill warn",
    };
    let when_label = if ended.is_empty() {
        format!("started {}", relative_time(&started))
    } else {
        format!(
            "finished {} (started {})",
            relative_time(&ended),
            relative_time(&started)
        )
    };
    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{script} · Activity</title><style>{css}.pill.ok{{background:#0f2818;color:#5fbf76}}.pill.warn{{background:#2a1f0a;color:#d4a648}}.pill.err{{background:#2a0f0f;color:#d65656}}pre.log{{background:#0a0a0a;border:1px solid #1f1f1f;padding:12px;border-radius:6px;overflow:auto;max-height:600px;font-size:12px;color:#cfcfcf}}</style></head><body><div class="wrap">{hdr}
<p><a href="/dashboard/activity">← back to activity</a></p>
<h1>{script}</h1>
<p><span class="{pill_class}">{status}</span> · {when_label} · exit {code}</p>
<p class="sub">Device: <a href="/dashboard/device/{did_esc}"><code>{did_esc}</code></a></p>
<h2>Summary</h2>
<p>{summary}</p>
<h2>Log tail</h2>
<pre class="log">{tail}</pre>
<p class="muted">Source on device: <code>{log_path}</code></p>
</div></body></html>"#,
        css = CSS,
        hdr = header("activity"),
        script = esc(&script),
        status = esc(&status),
        did_esc = esc(&did),
        summary = if summary.is_empty() {
            "<em>(none)</em>".into()
        } else {
            esc(&summary)
        },
        tail = if tail.is_empty() {
            "(no log captured — script did not pass --log to activity-end)".into()
        } else {
            esc(&tail)
        },
        log_path = esc(&log_path),
    );
    Html(html)
}

pub async fn games_by_system_page(
    AxState(state): AxState<State>,
    AxPath(system): AxPath<String>,
) -> Html<String> {
    games_render(state, Some(system), None).await
}

pub async fn games_list_page(
    AxState(state): AxState<State>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Html<String> {
    games_render(state, None, q.get("device").cloned()).await
}

pub async fn device_games_page(
    AxState(state): AxState<State>,
    AxPath(device_id): AxPath<String>,
) -> Html<String> {
    games_render(state, None, Some(device_id)).await
}

async fn games_render(
    state: State,
    filter_system: Option<String>,
    filter_device: Option<String>,
) -> Html<String> {
    let conn = state.lock().await;
    let mut systems_html = String::new();
    let all_cls = if filter_system.is_none() {
        "pill ok"
    } else {
        "pill"
    };
    systems_html.push_str(&format!(
        r#"<a href="/dashboard/games" class="{all_cls}" style="margin-right:6px;text-decoration:none">All</a>"#
    ));
    let mut sys_stmt = conn
        .prepare(
            "SELECT system, COUNT(DISTINCT game_name) FROM game_sessions
             WHERE system IS NOT NULL GROUP BY system ORDER BY 2 DESC",
        )
        .unwrap();
    let sys_rows: Vec<(String, i64)> = sys_stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .unwrap()
        .flatten()
        .collect();
    drop(sys_stmt);
    for (sys, n) in &sys_rows {
        let cls = if filter_system.as_deref() == Some(sys.as_str()) {
            "pill ok"
        } else {
            "pill"
        };
        systems_html.push_str(&format!(
            r#"<a href="/dashboard/games/{}" class="{cls}" style="margin-right:6px;text-decoration:none">{} ({n})</a>"#,
            esc(sys),
            esc(sys)
        ));
    }

    let mut rows_html = String::new();
    let mut where_parts: Vec<String> = vec!["s.game_name IS NOT NULL".into()];
    let mut params: Vec<String> = Vec::new();
    if let Some(s) = filter_system.as_ref() {
        params.push(s.clone());
        where_parts.push(format!("s.system = ?{}", params.len()));
    }
    if let Some(d) = filter_device.as_ref() {
        params.push(d.clone());
        where_parts.push(format!("s.device_id = ?{}", params.len()));
    }
    let sql = format!(
        "SELECT s.system, s.game_name, COUNT(*), SUM(s.duration_seconds), MAX(s.started_at),
                COALESCE(m.cover_url,''), COALESCE(m.year,''), COALESCE(m.genre,''),
                COALESCE(m.display_name, s.game_name)
         FROM game_sessions s
         LEFT JOIN game_metadata m
           ON m.system = s.system
          AND m.name_query = REPLACE(s.game_name,'_',' ')
         WHERE {}
         GROUP BY s.system, s.game_name
         ORDER BY 4 DESC",
        where_parts.join(" AND ")
    );
    let mut stmt = conn.prepare(&sql).unwrap();
    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<(
        String,
        String,
        i64,
        i64,
        String,
        String,
        String,
        String,
        String,
    )> {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4).unwrap_or_default(),
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
        ))
    };
    let param_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
    let collected: Vec<_> = stmt
        .query_map(param_refs.as_slice(), map_row)
        .unwrap()
        .flatten()
        .collect();
    for (sys, game, n, dur, last, cover, year, genre, display) in collected {
        let cover_cell = if cover.is_empty() {
            "<div class=\"cover-fallback\">🎮</div>".to_string()
        } else {
            format!(
                "<img class=\"cover\" loading=\"lazy\" src=\"{}\">",
                esc(&cover)
            )
        };
        let meta_line = if year.is_empty() && genre.is_empty() {
            String::new()
        } else {
            format!(
                "<div class=\"muted\" style=\"font-size:11px\">{} {}</div>",
                esc(&year),
                esc(&genre)
            )
        };
        rows_html.push_str(&format!(
            "<tr><td class=\"cover-cell\">{}</td><td><div>{}</div>{}<div class=\"muted\" style=\"font-size:10px\">{}</div></td><td><span class=\"pill\">{}</span></td><td>{}</td><td>{}</td><td class=\"muted\">{}</td></tr>",
            cover_cell,
            esc(&display),
            meta_line,
            esc(&game),
            esc(&sys),
            n,
            fmt_dur(dur),
            esc(&relative_time(&last))
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan=6 class=\"empty\">No games tracked yet. Open a ROM from EmulationStation — the agent will pick it up within 60 s.</td></tr>");
    }
    let title = match (&filter_system, &filter_device) {
        (Some(s), _) => format!("Games · {}", esc(s)),
        (_, Some(d)) => format!("Games · device <code>{}</code>", esc(d)),
        _ => "Games".to_string(),
    };
    let device_link = if let Some(did) = filter_device.as_ref() {
        let ip: Option<String> = conn
            .query_row(
                "SELECT last_ip FROM devices WHERE device_id=?1",
                [did.as_str()],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        match ip {
            Some(ip) if !ip.is_empty() => format!(
                r#"<p class="sub"><a href="http://{ip}:7878/" target="_blank">Open File Browser at {ip} →</a> · <a href="/dashboard/device/{}">← back to device</a></p>"#,
                esc(did)
            ),
            _ => format!(
                r#"<p class="sub muted"><a href="/dashboard/device/{}">← back to device</a></p>"#,
                esc(did)
            ),
        }
    } else {
        String::new()
    };
    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Games · Playora</title><style>{css}
.cover-cell{{width:64px;padding:6px}}
.cover{{width:48px;height:64px;object-fit:cover;border-radius:4px;display:block}}
.cover-fallback{{width:48px;height:64px;background:#1a1a22;border-radius:4px;display:flex;align-items:center;justify-content:center;font-size:24px}}
.pill.ok{{background:#0f2818;color:#5fbf76}}
</style></head><body><div class="wrap">{hdr}<h1>{title}</h1>{device_link}<p class="sub">{systems_html}</p><table><thead><tr><th></th><th>Game</th><th>System</th><th>Sessions</th><th>Total time</th><th>Last played</th></tr></thead><tbody>{rows_html}</tbody></table></div></body></html>"#,
        css = CSS,
        hdr = header("games")
    );
    Html(html)
}

pub async fn device_page(
    AxState(state): AxState<State>,
    AxPath(id): AxPath<String>,
) -> Html<String> {
    let conn = state.lock().await;
    let dev: Option<(String, String, String, String, String)> = conn.query_row(
        "SELECT device_id, COALESCE(device_name,''), COALESCE(device_profile,''), COALESCE(os_family,''), COALESCE(last_seen_at,'') FROM devices WHERE device_id=?1",
        [id.clone()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).ok();
    let last_ip: Option<String> = conn
        .query_row(
            "SELECT last_ip FROM devices WHERE device_id=?1",
            [id.clone()],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    let total_play: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds),0) FROM game_sessions WHERE device_id=?1",
            [id.clone()],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let sess_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM game_sessions WHERE device_id=?1",
            [id.clone()],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut games_html = String::new();
    let mut stmt = conn.prepare("SELECT game_name, system, SUM(duration_seconds), COUNT(*) FROM game_sessions WHERE device_id=?1 AND game_name IS NOT NULL GROUP BY game_name, system ORDER BY 3 DESC LIMIT 20").unwrap();
    let rows = stmt
        .query_map([id.clone()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .unwrap();
    for (g, s, d, c) in rows.flatten() {
        games_html.push_str(&format!(
            "<tr><td>{}</td><td><span class=\"pill\">{}</span></td><td>{}</td><td>{}</td></tr>",
            esc(&g),
            esc(&s),
            c,
            fmt_dur(d)
        ));
    }
    if games_html.is_empty() {
        games_html.push_str(
            "<tr><td colspan=4 class=\"empty\">No games tracked yet for this device.</td></tr>",
        );
    }

    let mut hw_html = String::new();
    let hw_row: Option<String> = conn.query_row("SELECT payload_json FROM hardware_snapshots WHERE device_id=?1 ORDER BY id DESC LIMIT 1", [id.clone()], |r| r.get(0)).ok();
    if let Some(j) = hw_row {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&j) {
            let g = |k: &str| {
                v.get(k)
                    .map(|x| match x {
                        serde_json::Value::String(s) => s.clone(),
                        _ => x.to_string(),
                    })
                    .unwrap_or_else(|| "?".into())
            };
            let panel_res = v
                .get("panel_resolution")
                .and_then(|x| x.as_array())
                .map(|a| {
                    format!(
                        "{}×{}",
                        a.first().and_then(|v| v.as_u64()).unwrap_or(0),
                        a.get(1).and_then(|v| v.as_u64()).unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| "?".into());
            hw_html.push_str(&format!(
                r#"<tr><th>CPU</th><td>{} ({}, {} cores)</td></tr>
                <tr><th>Memory</th><td>{} MB total</td></tr>
                <tr><th>Kernel</th><td>{}</td></tr>
                <tr><th>Hardware string</th><td><code>{}</code></td></tr>
                <tr><th>Panel</th><td>{} @ {}</td></tr>
                <tr><th>RetroArch</th><td>{}</td></tr>"#,
                esc(&g("cpu_model")),
                esc(&g("cpu_arch")),
                g("cpu_cores"),
                g("mem_total_mb"),
                esc(&g("kernel")),
                esc(&g("hardware_string")),
                esc(&g("panel_compatible")),
                panel_res,
                if v.get("retroarch_detected")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
                {
                    "detected"
                } else {
                    "not detected"
                }
            ));
        }
    }
    if hw_html.is_empty() {
        hw_html.push_str("<tr><td colspan=2 class=\"empty\">No hardware snapshot. Open <code>Ports → Playora Hardware</code>.</td></tr>");
    }

    let mut sess_html = String::new();
    let mut stmt = conn.prepare("SELECT system, game_name, duration_seconds, started_at FROM game_sessions WHERE device_id=?1 ORDER BY id DESC LIMIT 15").unwrap();
    let rows = stmt
        .query_map([id.clone()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1).unwrap_or_default(),
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3).unwrap_or_default(),
            ))
        })
        .unwrap();
    for (s, g, d, t) in rows.flatten() {
        sess_html.push_str(&format!("<tr><td><span class=\"pill\">{}</span></td><td>{}</td><td>{}</td><td class=\"muted\">{}</td></tr>", esc(&s), esc(&g), fmt_dur(d), esc(&t)));
    }
    if sess_html.is_empty() {
        sess_html.push_str("<tr><td colspan=4 class=\"empty\">No sessions yet.</td></tr>");
    }

    // Per-device recent activity (last 20).
    let mut act_html = String::new();
    let mut stmt = conn.prepare("SELECT id, script, status, started_at, COALESCE(ended_at,''), COALESCE(exit_code,-1), COALESCE(summary,'') FROM activities WHERE device_id=?1 ORDER BY id DESC LIMIT 20").unwrap();
    let rows = stmt
        .query_map([id.clone()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .unwrap();
    for (aid, script, status, started, ended, code, summary) in rows.flatten() {
        let pill_class = match status.as_str() {
            "ok" => "pill ok",
            "fail" => "pill err",
            _ => "pill warn",
        };
        let when = if ended.is_empty() {
            format!("started {}", relative_time(&started))
        } else {
            format!("finished {}", relative_time(&ended))
        };
        act_html.push_str(&format!(
            "<tr><td><a href=\"/dashboard/activity/{aid}\">{}</a></td><td><span class=\"{}\">{}</span></td><td class=\"muted\">{}</td><td class=\"muted\">{}</td><td class=\"muted\">{}</td></tr>",
            esc(&script), pill_class, esc(&status), esc(&summary), esc(&when),
            if code >= 0 { format!("exit {code}") } else { String::new() }
        ));
    }
    if act_html.is_empty() {
        act_html.push_str(
            "<tr><td colspan=5 class=\"empty\">No activity recorded for this device.</td></tr>",
        );
    }

    // Per-device ROMs (latest scanned, with delete button).
    let mut roms_html = String::new();
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(payload_json,'$.payload.data.metadata.rom_path'),
                json_extract(payload_json,'$.payload.data.metadata.name'),
                json_extract(payload_json,'$.payload.data.metadata.system'),
                json_extract(payload_json,'$.payload.data.metadata.file_size'),
                MAX(received_at)
         FROM events WHERE device_id=?1 AND event_type='rom_scanned'
         GROUP BY 1 ORDER BY 5 DESC LIMIT 30",
        )
        .unwrap();
    let rows = stmt
        .query_map([id.clone()], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .unwrap();
    for (path, name, system, size, recv) in rows.flatten() {
        let Some(path) = path else { continue };
        let name = name.unwrap_or_else(|| "?".into());
        let system = system.unwrap_or_else(|| "?".into());
        let size_mb = size.unwrap_or(0) as f64 / 1024.0 / 1024.0;
        let recv = recv.unwrap_or_default();
        roms_html.push_str(&format!(
            "<tr><td>{}</td><td><span class=\"pill\">{}</span></td><td class=\"muted\">{:.1} MB</td><td class=\"muted\">{}</td><td><form method=\"post\" action=\"/dashboard/device/{}/delete-rom\" onsubmit=\"return confirm('Queue delete of {}?')\"><input type=\"hidden\" name=\"rom_path\" value=\"{}\"><button class=\"del\" type=\"submit\">Queue delete</button></form></td></tr>",
            esc(&name), esc(&system), size_mb, esc(&relative_time(&recv)),
            esc(&id),
            esc(&name),
            esc(&path)
        ));
    }
    if roms_html.is_empty() {
        roms_html.push_str("<tr><td colspan=5 class=\"empty\">No ROMs scanned for this device. Run <code>Playora Scan ROMs</code>.</td></tr>");
    }

    // Pending delete requests for this device.
    let mut pend_html = String::new();
    let mut stmt = conn.prepare(
        "SELECT rom_path, status, requested_at, COALESCE(processed_at,''), COALESCE(error,'') FROM delete_requests WHERE device_id=?1 ORDER BY id DESC LIMIT 20",
    ).unwrap();
    let rows = stmt
        .query_map([id.clone()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .unwrap();
    for (path, status, req_at, proc_at, err) in rows.flatten() {
        let pill = match status.as_str() {
            "ok" => "pill ok",
            "fail" => "pill err",
            _ => "pill warn",
        };
        pend_html.push_str(&format!(
            "<tr><td><code>{}</code></td><td><span class=\"{}\">{}</span></td><td class=\"muted\">{}</td><td class=\"muted\">{}</td><td class=\"muted\">{}</td></tr>",
            esc(&path), pill, esc(&status), esc(&relative_time(&req_at)),
            esc(&relative_time(&proc_at)), esc(&err)
        ));
    }
    if pend_html.is_empty() {
        pend_html.push_str("<tr><td colspan=5 class=\"empty\">No delete requests.</td></tr>");
    }

    // Per-device recent events (last 30, grouped by type).
    let mut ev_html = String::new();
    let mut stmt = conn.prepare("SELECT event_type, COUNT(*) as n, MAX(received_at) FROM events WHERE device_id=?1 GROUP BY event_type ORDER BY n DESC").unwrap();
    let rows = stmt
        .query_map([id.clone()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap();
    for (typ, n, last) in rows.flatten() {
        ev_html.push_str(&format!(
            "<tr><td><span class=\"pill\">{}</span></td><td>{}</td><td class=\"muted\">{}</td></tr>",
            esc(&typ), n, esc(&relative_time(&last))
        ));
    }
    if ev_html.is_empty() {
        ev_html.push_str("<tr><td colspan=3 class=\"empty\">No events yet.</td></tr>");
    }

    let events_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE device_id=?1",
            [id.clone()],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let activities_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM activities WHERE device_id=?1",
            [id.clone()],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let hw_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM hardware_snapshots WHERE device_id=?1",
            [id.clone()],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Now-playing: most recent session without ended_at.
    let now_playing: Option<(String, String)> = conn
        .query_row(
            "SELECT system, COALESCE(game_name,'') FROM game_sessions
             WHERE device_id=?1 AND ended_at IS NULL
             ORDER BY id DESC LIMIT 1",
            [id.clone()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();

    // Heartbeat recency drives the "Auto-running" badge.
    let recent_heartbeat: Option<String> = conn
        .query_row(
            "SELECT received_at FROM heartbeats WHERE device_id=?1 ORDER BY id DESC LIMIT 1",
            [id.clone()],
            |r| r.get(0),
        )
        .ok();
    let auto_running = recent_heartbeat
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds() < 120)
        .unwrap_or(false);
    let auto_badge = if auto_running {
        "<span class=\"pill ok\" title=\"autosync is running on the device — file browser + game tracker live\">Auto-running</span>"
    } else {
        "<span class=\"pill warn\" title=\"last heartbeat is older than 2 min — run Ports → Playora Autosync Enable on the device\">Idle</span>"
    };
    let now_playing_badge = match &now_playing {
        Some((sys, game)) if !game.is_empty() => format!(
            r#"<span class="pill playing" title="Playora session tracker sees this game open">▶ {} · {}</span>"#,
            esc(game),
            esc(sys)
        ),
        Some((sys, _)) => format!(r#"<span class="pill playing">▶ {}</span>"#, esc(sys)),
        None => String::new(),
    };

    let fs_block = match last_ip.as_deref().filter(|s| !s.is_empty()) {
        Some(ip) => format!(
            r#"<p class="sub">Last LAN IP: <code>{ip}</code> {auto_badge} {now_playing_badge} · <a href="http://{ip}:7878/" target="_blank" class="open-fs">Open File Browser →</a> · <a href="/dashboard/device/{did}/games">Games →</a> · <a href="/dashboard/cloud-roms/{did}">Cloud ROMs →</a> · <form method="post" action="/dashboard/device/{did}/update" style="display:inline" onsubmit="return confirm('Queue agent self-update?')"><button class="upd" type="submit">Update Agent</button></form></p>"#,
            did = esc(&id)
        ),
        None => format!(
            "<p class=\"sub muted\">No LAN IP recorded yet {auto_badge} {now_playing_badge}. <a href=\"/dashboard/device/{did}/games\">Games →</a> · <a href=\"/dashboard/cloud-roms/{did}\">Cloud ROMs →</a> · <form method=\"post\" action=\"/dashboard/device/{did}/update\" style=\"display:inline\"><button class=\"upd\" type=\"submit\">Update Agent</button></form></p>",
            did = esc(&id)
        ),
    };

    let title = match dev.as_ref() {
        Some((_, name, profile, os, seen)) => format!(
            "<h1>{}</h1><p class=\"sub\"><span class=\"pill\">{}</span> · {} · last seen <code>{}</code></p><p><code>{}</code></p>",
            esc(name), esc(profile), esc(os), esc(seen), esc(&id)
        ),
        None => format!(
            "<h1>Unknown device</h1><p class=\"sub\">No record. Run any Playora menu entry on the device.</p><p><code>{}</code></p>",
            esc(&id)
        ),
    };

    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Device · Playora</title>
<meta http-equiv="refresh" content="20">
<style>{css}.pill.ok{{background:#0f2818;color:#5fbf76}}.pill.warn{{background:#2a1f0a;color:#d4a648}}.pill.err{{background:#2a0f0f;color:#d65656}}</style></head>
<body><div class="wrap">
{hdr}
{title}
{fs_block}
<div class="grid">
    <div class="card"><div class="l">Sessions</div><div class="v">{sess_count}</div></div>
    <div class="card"><div class="l">Total playtime</div><div class="v">{play}</div></div>
    <div class="card"><div class="l">Activities</div><div class="v">{activities_count}</div></div>
    <div class="card"><div class="l">Events</div><div class="v">{events_count}</div></div>
    <div class="card"><div class="l">HW snapshots</div><div class="v">{hw_count}</div></div>
</div>
<h2>Recent activity</h2>
<table><thead><tr><th>Script</th><th>Status</th><th>Summary</th><th>When</th><th></th></tr></thead><tbody>{act_html}</tbody></table>
<h2>Hardware</h2>
<table>{hw_html}</table>
<h2>ROMs (scanned)</h2>
<p class="sub">Click "Queue delete" to schedule removal — the agent processes the queue every ~60s and the file is removed from the SD.</p>
<table><thead><tr><th>Name</th><th>System</th><th>Size</th><th>Last scanned</th><th></th></tr></thead><tbody>{roms_html}</tbody></table>
<h2>Delete queue</h2>
<table><thead><tr><th>Path</th><th>Status</th><th>Requested</th><th>Processed</th><th>Error</th></tr></thead><tbody>{pend_html}</tbody></table>
<h2>Events by type</h2>
<table><thead><tr><th>Type</th><th>Count</th><th>Last received</th></tr></thead><tbody>{ev_html}</tbody></table>
<h2>Top games</h2>
<table><thead><tr><th>Game</th><th>System</th><th>Sessions</th><th>Total time</th></tr></thead><tbody>{games_html}</tbody></table>
<h2>Recent sessions</h2>
<table><thead><tr><th>System</th><th>Game</th><th>Duration</th><th>Started</th></tr></thead><tbody>{sess_html}</tbody></table>
<footer>Playora · {now}</footer>
</div></body></html>"#,
        css = CSS,
        hdr = header("devices"),
        play = fmt_dur(total_play),
        now = Utc::now().format("%Y-%m-%d %H:%M UTC"),
    );
    Html(html)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn fmt_dur(s: i64) -> String {
    let s = s.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

fn relative_time(ts: &str) -> String {
    if ts.is_empty() || ts == "—" {
        return "never".into();
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(ts).ok();
    if let Some(t) = parsed {
        let now = chrono::Utc::now();
        let delta = now.signed_duration_since(t.with_timezone(&chrono::Utc));
        let secs = delta.num_seconds().max(0);
        if secs < 5 {
            return "just now".into();
        }
        if secs < 60 {
            return format!("{secs}s ago");
        }
        let mins = secs / 60;
        if mins < 60 {
            return format!("{mins}m ago");
        }
        let hours = mins / 60;
        if hours < 24 {
            return format!("{hours}h ago");
        }
        let days = hours / 24;
        format!("{days}d ago")
    } else {
        ts.to_string()
    }
}

// ---------------------------------------------------------------
// Sprint 3H: Issues + ROM Audit + Doctor Report pages
// ---------------------------------------------------------------

const TAB_STYLE: &str = r#"
<style>
body{background:#0a0a0a;color:#d8d8d8;font-family:system-ui,sans-serif;margin:0;padding:24px}
a{color:#9ad;text-decoration:none}a:hover{text-decoration:underline}
h1{font-size:22px;margin:0 0 12px} h2{font-size:16px;margin:24px 0 8px;color:#9ad}
.tabs{display:flex;gap:8px;margin:8px 0 16px}
.tabs a{padding:6px 12px;background:#181818;border:1px solid #2a2a2a;border-radius:4px;color:#d8d8d8}
.tabs a.active{background:#2a4060;border-color:#3a5070}
table{border-collapse:collapse;width:100%;margin-top:8px}
th,td{padding:6px 10px;text-align:left;border-bottom:1px solid #1f1f1f;font-size:13px;vertical-align:top}
th{color:#9ad;font-weight:600}
.badge{display:inline-block;padding:2px 8px;border-radius:10px;font-size:11px;font-weight:600}
.badge.critical{background:#4a1010;color:#ff7575}
.badge.warning{background:#4a3010;color:#d4a648}
.badge.info{background:#102a4a;color:#9ad}
.muted{color:#666;font-size:11px}
.card{background:#101010;border:1px solid #1f1f1f;border-radius:6px;padding:12px;margin:8px 0}
code{background:#1a1a1a;padding:2px 5px;border-radius:3px;font-size:11px}
</style>
"#;

fn nav_tabs(device_id: &str, active: &str) -> String {
    let tabs = [
        (
            "overview",
            "Overview",
            format!("/dashboard/device/{device_id}"),
        ),
        (
            "issues",
            "Issues",
            format!("/dashboard/device/{device_id}/issues"),
        ),
        (
            "rom-audit",
            "ROM Audit",
            format!("/dashboard/device/{device_id}/rom-audit"),
        ),
        (
            "doctor",
            "Doctor Report",
            format!("/dashboard/device/{device_id}/doctor"),
        ),
        (
            "sessions",
            "Sessions",
            format!("/dashboard/device/{device_id}/sessions"),
        ),
        (
            "games",
            "Games",
            format!("/dashboard/device/{device_id}/games"),
        ),
    ];
    let mut out = String::from("<div class=\"tabs\">");
    for (id, label, href) in tabs {
        let cls = if id == active { "active" } else { "" };
        out.push_str(&format!("<a class=\"{cls}\" href=\"{href}\">{label}</a>"));
    }
    out.push_str("</div>");
    out
}

#[derive(Deserialize)]
pub struct IssuesQuery {
    pub sev: Option<String>,
}

pub async fn device_issues_page(
    AxState(state): AxState<State>,
    AxPath(id): AxPath<String>,
    axum::extract::Query(q): axum::extract::Query<IssuesQuery>,
) -> Html<String> {
    let filter = q.sev.as_deref().map(|s| s.to_ascii_lowercase());
    let conn = state.lock().await;
    let mut rows_html = String::new();
    let mut by_sev = (0u32, 0u32, 0u32); // critical, warning, info
    if let Ok(mut stmt) = conn.prepare(
        "SELECT payload_json, received_at FROM events
         WHERE device_id=?1 AND event_type='system_issue_detected'
         ORDER BY id DESC LIMIT 200",
    ) {
        let rows = stmt
            .query_map([&id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .ok();
        if let Some(rs) = rows {
            for (payload, recv) in rs.flatten() {
                let p: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();
                let data = p.get("data").unwrap_or(&p);
                let sev = data
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("info");
                let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                let code = data.get("code").and_then(|v| v.as_str()).unwrap_or("?");
                let fix = data
                    .get("suggested_fix")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let evidence = data.get("evidence").and_then(|v| v.as_str()).unwrap_or("");
                let auto = data
                    .get("auto_fixable")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                match sev {
                    "critical" => by_sev.0 += 1,
                    "warning" => by_sev.1 += 1,
                    _ => by_sev.2 += 1,
                }
                if let Some(f) = &filter {
                    if sev != f {
                        continue;
                    }
                }
                rows_html.push_str(&format!(
                    "<tr><td><span class=\"badge {}\">{}</span></td>\
                     <td>{}<br/><code>{}</code></td>\
                     <td>{}</td>\
                     <td>{}{}</td>\
                     <td class=\"muted\">{}</td></tr>",
                    esc(sev),
                    esc(sev),
                    esc(title),
                    esc(code),
                    if evidence.is_empty() {
                        String::new()
                    } else {
                        format!("<code>{}</code>", esc(evidence))
                    },
                    esc(fix),
                    if auto {
                        " <span class=\"badge info\">auto-fixable</span>"
                    } else {
                        ""
                    },
                    esc(&relative_time(&recv)),
                ));
            }
        }
    }
    let tabs = nav_tabs(&id, "issues");
    let filter_label = filter
        .as_deref()
        .map(|f| format!(" (filtered: {f})"))
        .unwrap_or_default();
    let html = format!(
        "<!doctype html><html><head><title>Issues — {id}</title>{TAB_STYLE}</head><body>\
         <h1>Device {id} — Issues{filter_label}</h1>{tabs}\
         <div class=\"card\">\
           <a href=\"?sev=critical\"><span class=\"badge critical\">critical: {}</span></a> \
           <a href=\"?sev=warning\"><span class=\"badge warning\">warning: {}</span></a> \
           <a href=\"?sev=info\"><span class=\"badge info\">info: {}</span></a> \
           <a href=\"./issues\" class=\"muted\">clear filter</a>\
         </div>\
         <table><thead><tr><th>Severity</th><th>Title / Code</th><th>Evidence</th><th>Suggested fix</th><th>When</th></tr></thead>\
         <tbody>{rows_html}</tbody></table></body></html>",
        by_sev.0, by_sev.1, by_sev.2,
    );
    Html(html)
}

pub async fn device_sessions_page(
    AxState(state): AxState<State>,
    AxPath(id): AxPath<String>,
) -> Html<String> {
    let conn = state.lock().await;
    let mut rows = String::new();
    let mut total_secs: i64 = 0;
    let mut count: u32 = 0;
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(game_name,'?'), COALESCE(system,'?'), COALESCE(core,''),
                COALESCE(started_at,''), COALESCE(ended_at,''),
                COALESCE(duration_seconds,0), COALESCE(save_changed,0),
                COALESCE(max_cpu_percent,0.0), COALESCE(max_memory_mb,0)
         FROM game_sessions WHERE device_id=?1 ORDER BY id DESC LIMIT 200",
    ) {
        let rs = stmt
            .query_map([&id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, f64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            })
            .ok();
        if let Some(rs) = rs {
            for (name, system, core, started, _ended, dur, save_changed, cpu, mem) in rs.flatten() {
                total_secs += dur;
                count += 1;
                rows.push_str(&format!(
                    "<tr><td>{}</td><td><code>{}</code></td><td><code>{}</code></td>\
                     <td>{}</td><td>{}</td>\
                     <td>{}</td><td>{:.1}%</td><td>{} MiB</td></tr>",
                    esc(&name),
                    esc(&system),
                    esc(&core),
                    esc(&relative_time(&started)),
                    fmt_dur(dur),
                    if save_changed > 0 {
                        "<span class=\"badge info\">yes</span>"
                    } else {
                        "—"
                    },
                    cpu,
                    mem,
                ));
            }
        }
    }
    // Counts of crashes / orphans / black-screen for this device.
    let crashed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE device_id=?1 AND event_type='game_session_crashed'",
            [&id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let orphaned: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE device_id=?1 AND event_type='game_session_orphaned'",
            [&id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let bsr: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE device_id=?1 AND event_type='black_screen_recovered'",
            [&id], |r| r.get(0),
        )
        .unwrap_or(0);
    let tabs = nav_tabs(&id, "sessions");
    Html(format!(
        "<!doctype html><html><head><title>Sessions — {id}</title>{TAB_STYLE}</head><body>\
         <h1>Device {id} — Sessions</h1>{tabs}\
         <div class=\"card\">{count} session(s) · total playtime {} · \
           <span class=\"badge warning\">crashed: {crashed}</span> \
           <span class=\"badge warning\">orphaned: {orphaned}</span> \
           <span class=\"badge critical\">black-screen recovered: {bsr}</span>\
         </div>\
         <table><thead><tr><th>Game</th><th>System</th><th>Core</th><th>Started</th><th>Duration</th><th>Save changed</th><th>Peak CPU</th><th>Peak mem</th></tr></thead>\
         <tbody>{rows}</tbody></table></body></html>",
        fmt_dur(total_secs)
    ))
}

pub async fn device_rom_audit_page(
    AxState(state): AxState<State>,
    AxPath(id): AxPath<String>,
) -> Html<String> {
    let conn = state.lock().await;
    let latest: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM events
             WHERE device_id=?1 AND event_type='rom_audit_result'
             ORDER BY id DESC LIMIT 1",
            [&id],
            |r| r.get(0),
        )
        .ok();
    let tabs = nav_tabs(&id, "rom-audit");
    let body = match latest {
        None => "<p class=\"muted\">No ROM audit reported yet. Run <code>playora-agent audit-roms</code> on the device.</p>".to_string(),
        Some(j) => {
            let v: serde_json::Value = serde_json::from_str(&j).unwrap_or_default();
            let d = v.get("data").unwrap_or(&v);
            let g = |k: &str| d.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
            let bios = d
                .get("bios_missing")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .map(esc)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let path = d
                .get("report_path")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            format!(
                "<table><tbody>\
                 <tr><td>ROMs total</td><td>{}</td></tr>\
                 <tr><td>Zero-byte</td><td>{}</td></tr>\
                 <tr><td>Duplicates</td><td>{}</td></tr>\
                 <tr><td>Broken CUE</td><td>{}</td></tr>\
                 <tr><td>Broken M3U</td><td>{}</td></tr>\
                 <tr><td>Invalid gamelists</td><td>{}</td></tr>\
                 <tr><td>macOS junk</td><td>{}</td></tr>\
                 <tr><td>Unknown extensions</td><td>{}</td></tr>\
                 <tr><td>Missing BIOS</td><td>{}</td></tr>\
                 <tr><td>Report path (device)</td><td><code>{}</code></td></tr>\
                 </tbody></table>",
                g("roms_total"),
                g("zero_byte"),
                g("duplicates"),
                g("broken_cue"),
                g("broken_m3u"),
                g("gamelist_invalid"),
                g("macos_junk"),
                g("unknown_extensions"),
                if bios.is_empty() { "—".into() } else { bios },
                esc(path),
            )
        }
    };
    Html(format!(
        "<!doctype html><html><head><title>ROM Audit — {id}</title>{TAB_STYLE}</head><body>\
         <h1>Device {id} — ROM Audit</h1>{tabs}{body}</body></html>"
    ))
}

pub async fn device_doctor_page(
    AxState(state): AxState<State>,
    AxPath(id): AxPath<String>,
) -> Html<String> {
    let conn = state.lock().await;
    let latest: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM events
             WHERE device_id=?1 AND event_type='doctor_report'
             ORDER BY id DESC LIMIT 1",
            [&id],
            |r| r.get(0),
        )
        .ok();
    let tabs = nav_tabs(&id, "doctor");
    let body = match latest {
        None => "<p class=\"muted\">No doctor report yet. Run <code>playora-agent doctor --deep</code> on the device.</p>".to_string(),
        Some(j) => {
            let v: serde_json::Value = serde_json::from_str(&j).unwrap_or_default();
            let d = v.get("data").unwrap_or(&v);
            let score = d.get("score").and_then(|x| x.as_str()).unwrap_or("?");
            let cls = match score {
                "fail" => "critical",
                "warn" => "warning",
                _ => "info",
            };
            let g = |k: &str| d.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
            let auto = d
                .get("auto_fixes")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| format!("<li>{}</li>", esc(s))).collect::<String>())
                .unwrap_or_default();
            let manual = d
                .get("manual_fixes")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| format!("<li>{}</li>", esc(s))).collect::<String>())
                .unwrap_or_default();
            let report_path = d
                .get("report_path")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            format!(
                "<div class=\"card\"><span class=\"badge {cls}\">score: {score}</span> \
                 ok: {} · warn: {} · fail: {} · total: {}</div>\
                 <h2>Auto-fixable</h2><ul>{}</ul>\
                 <h2>Manual fixes</h2><ul>{}</ul>\
                 <p class=\"muted\">Report on device: <code>{}</code></p>",
                g("checks_ok"),
                g("checks_warn"),
                g("checks_fail"),
                g("checks_total"),
                if auto.is_empty() { "<li class=\"muted\">none</li>".into() } else { auto },
                if manual.is_empty() { "<li class=\"muted\">none</li>".into() } else { manual },
                esc(report_path),
            )
        }
    };
    Html(format!(
        "<!doctype html><html><head><title>Doctor — {id}</title>{TAB_STYLE}</head><body>\
         <h1>Device {id} — Doctor Report</h1>{tabs}{body}</body></html>"
    ))
}
