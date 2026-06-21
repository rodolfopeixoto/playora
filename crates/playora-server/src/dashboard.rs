use crate::State;
use axum::{
    extract::{Path as AxPath, State as AxState},
    response::Html,
};
use chrono::Utc;

const CSS: &str = r#"
body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;background:#0e0e10;color:#e6e6e6;margin:0;padding:24px}
h1{margin:0 0 16px 0;font-size:22px}
h2{font-size:14px;color:#aaa;margin:24px 0 8px;text-transform:uppercase;letter-spacing:.5px}
a{color:#9ad;text-decoration:none}
a:hover{text-decoration:underline}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;margin-bottom:24px}
.card{background:#1b1b1f;border:1px solid #2a2a30;border-radius:8px;padding:16px}
.card .v{font-size:24px;font-weight:600;margin-top:4px}
.card .l{color:#8a8a90;font-size:11px;text-transform:uppercase;letter-spacing:.5px}
table{width:100%;border-collapse:collapse;margin:8px 0;background:#15151a;border-radius:6px;overflow:hidden}
th,td{padding:8px 12px;border-bottom:1px solid #25252a;font-size:13px;text-align:left;vertical-align:top}
th{color:#8a8a90;font-weight:500;background:#181820}
tr:hover td{background:#1a1a22}
code{color:#9ad;font-family:ui-monospace,monospace;font-size:12px}
.muted{color:#666;font-size:11px}
.pill{display:inline-block;padding:2px 8px;border-radius:10px;font-size:11px;background:#252530;color:#aab}
footer{color:#666;font-size:11px;margin-top:32px}
nav{margin-bottom:16px}
nav a{margin-right:12px;color:#9ad}
pre{background:#15151a;border:1px solid #2a2a30;padding:12px;border-radius:6px;overflow:auto;font-size:11px;max-height:400px}
"#;

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
    let samples: i64 = conn
        .query_row("SELECT COUNT(*) FROM resource_samples", [], |r| r.get(0))
        .unwrap_or(0);
    let total_play: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds),0) FROM game_sessions",
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
    let mut stmt = conn.prepare("SELECT device_id, device_name, device_profile, last_seen_at FROM devices ORDER BY last_seen_at DESC LIMIT 25").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1).unwrap_or_default(),
                r.get::<_, String>(2).unwrap_or_default(),
                r.get::<_, String>(3).unwrap_or_default(),
            ))
        })
        .unwrap();
    for row in rows.flatten() {
        let did = esc(&row.0);
        devices_html.push_str(&format!(
            "<tr><td><a href=\"/dashboard/device/{}\"><code>{}</code></a></td><td>{}</td><td><span class=\"pill\">{}</span></td><td class=\"muted\">{}</td></tr>",
            did, did, esc(&row.1), esc(&row.2), esc(&row.3)
        ));
    }
    if devices_html.is_empty() {
        devices_html.push_str("<tr><td colspan=4 class=\"muted\">No devices registered yet. Boot a console and run any Playora menu entry.</td></tr>");
    }

    let mut ranking_html = String::new();
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
    for row in rows.flatten() {
        ranking_html.push_str(&format!(
            "<tr><td>{}</td><td><span class=\"pill\">{}</span></td><td>{}</td></tr>",
            esc(&row.0),
            esc(&row.1),
            fmt_dur(row.2)
        ));
    }
    if ranking_html.is_empty() {
        ranking_html.push_str("<tr><td colspan=3 class=\"muted\">No play sessions yet.</td></tr>");
    }

    let mut sys_html = String::new();
    let mut stmt = conn.prepare("SELECT system, COUNT(*), SUM(duration_seconds) FROM game_sessions WHERE system IS NOT NULL GROUP BY system ORDER BY 3 DESC LIMIT 15").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .unwrap();
    for row in rows.flatten() {
        sys_html.push_str(&format!(
            "<tr><td><span class=\"pill\">{}</span></td><td>{}</td><td>{}</td></tr>",
            esc(&row.0),
            row.1,
            fmt_dur(row.2)
        ));
    }
    if sys_html.is_empty() {
        sys_html.push_str("<tr><td colspan=3 class=\"muted\">No system data yet.</td></tr>");
    }

    let mut events_html = String::new();
    let mut stmt = conn.prepare("SELECT event_id, device_id, event_type, received_at FROM events ORDER BY id DESC LIMIT 20").unwrap();
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
    for row in rows.flatten() {
        events_html.push_str(&format!(
            "<tr><td><code>{}</code></td><td><a href=\"/dashboard/device/{}\"><code>{}</code></a></td><td><span class=\"pill\">{}</span></td><td class=\"muted\">{}</td></tr>",
            esc(&row.0), esc(&row.1), esc(&row.1), esc(&row.2), esc(&row.3)
        ));
    }
    if events_html.is_empty() {
        events_html.push_str("<tr><td colspan=4 class=\"muted\">No events yet.</td></tr>");
    }

    let html = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Playora Hub</title>
<meta http-equiv="refresh" content="15">
<style>{css}</style></head>
<body>
  <nav><a href="/dashboard">Overview</a></nav>
  <h1>Playora Hub</h1>
  <div class="grid">
    <div class="card"><div class="l">Devices</div><div class="v">{devices}</div></div>
    <div class="card"><div class="l">Events</div><div class="v">{events}</div></div>
    <div class="card"><div class="l">Sessions</div><div class="v">{sessions}</div></div>
    <div class="card"><div class="l">Total playtime</div><div class="v">{play}</div></div>
    <div class="card"><div class="l">HW snapshots</div><div class="v">{snapshots}</div></div>
    <div class="card"><div class="l">Resource samples</div><div class="v">{samples}</div></div>
    <div class="card"><div class="l">Last heartbeat</div><div class="v" style="font-size:14px">{last_hb}</div></div>
  </div>

  <h2>Devices (click for detail)</h2>
  <table><tr><th>ID</th><th>Name</th><th>Profile</th><th>Last seen</th></tr>{devices_html}</table>

  <h2>Top games by playtime</h2>
  <table><tr><th>Game</th><th>System</th><th>Total time</th></tr>{ranking_html}</table>

  <h2>Top systems</h2>
  <table><tr><th>System</th><th>Sessions</th><th>Total time</th></tr>{sys_html}</table>

  <h2>Latest events</h2>
  <table><tr><th>Event ID</th><th>Device</th><th>Type</th><th>Received</th></tr>{events_html}</table>

  <footer>Playora — {now}</footer>
</body></html>"#,
        css = CSS,
        play = fmt_dur(total_play),
        now = Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
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
        [id.clone()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).ok();

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
    for row in rows.flatten() {
        games_html.push_str(&format!(
            "<tr><td>{}</td><td><span class=\"pill\">{}</span></td><td>{}</td><td>{}</td></tr>",
            esc(&row.0),
            esc(&row.1),
            row.3,
            fmt_dur(row.2)
        ));
    }
    if games_html.is_empty() {
        games_html.push_str("<tr><td colspan=4 class=\"muted\">No games tracked yet.</td></tr>");
    }

    let mut hw_html = String::new();
    let hw_row: Option<String> = conn.query_row(
        "SELECT payload_json FROM hardware_snapshots WHERE device_id=?1 OR device_id='' ORDER BY id DESC LIMIT 1",
        [id.clone()], |r| r.get(0)
    ).ok();
    if let Some(j) = hw_row {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&j) {
            let cpu = v.get("cpu_model").and_then(|x| x.as_str()).unwrap_or("?");
            let arch = v.get("cpu_arch").and_then(|x| x.as_str()).unwrap_or("?");
            let cores = v.get("cpu_cores").and_then(|x| x.as_u64()).unwrap_or(0);
            let mem = v.get("mem_total_mb").and_then(|x| x.as_u64()).unwrap_or(0);
            let kernel = v.get("kernel").and_then(|x| x.as_str()).unwrap_or("?");
            let hw_str = v
                .get("hardware_string")
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let panel = v
                .get("panel_compatible")
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let res = v.get("panel_resolution").and_then(|x| x.as_array());
            let res_s = res
                .map(|a| {
                    format!(
                        "{}x{}",
                        a.first().and_then(|v| v.as_u64()).unwrap_or(0),
                        a.get(1).and_then(|v| v.as_u64()).unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| "?".into());
            hw_html.push_str(&format!(
                r#"
                <tr><th>CPU</th><td>{} ({}, {} cores)</td></tr>
                <tr><th>Memory</th><td>{} MB</td></tr>
                <tr><th>Kernel</th><td>{}</td></tr>
                <tr><th>Hardware string</th><td>{}</td></tr>
                <tr><th>Panel</th><td>{} / {}</td></tr>
            "#,
                esc(cpu),
                esc(arch),
                cores,
                mem,
                esc(kernel),
                esc(hw_str),
                esc(panel),
                res_s
            ));
        }
    }
    if hw_html.is_empty() {
        hw_html.push_str("<tr><td colspan=2 class=\"muted\">No hardware snapshot yet. Open Ports → Playora Hardware on the device.</td></tr>");
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
    for row in rows.flatten() {
        sess_html.push_str(&format!("<tr><td><span class=\"pill\">{}</span></td><td>{}</td><td>{}</td><td class=\"muted\">{}</td></tr>", esc(&row.0), esc(&row.1), fmt_dur(row.2), esc(&row.3)));
    }
    if sess_html.is_empty() {
        sess_html.push_str("<tr><td colspan=4 class=\"muted\">No sessions tracked yet.</td></tr>");
    }

    let header = match dev.as_ref() {
        Some((id, name, profile, os, _)) => format!("<h1>{} <span class=\"pill\">{}</span> <span class=\"muted\">{}</span></h1><p><code>{}</code></p>", esc(name), esc(profile), esc(os), esc(id)),
        None => format!("<h1>Unknown device <code>{}</code></h1><p class=\"muted\">No record found. Run any Playora menu action to register.</p>", esc(&id)),
    };

    let html = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Playora — Device</title>
<meta http-equiv="refresh" content="20">
<style>{css}</style></head>
<body>
  <nav><a href="/dashboard">← Overview</a></nav>
  {header}
  <div class="grid">
    <div class="card"><div class="l">Sessions</div><div class="v">{sess_count}</div></div>
    <div class="card"><div class="l">Total playtime</div><div class="v">{play}</div></div>
  </div>

  <h2>Hardware</h2>
  <table>{hw_html}</table>

  <h2>Top games</h2>
  <table><tr><th>Game</th><th>System</th><th>Sessions</th><th>Total time</th></tr>{games_html}</table>

  <h2>Recent sessions</h2>
  <table><tr><th>System</th><th>Game</th><th>Duration</th><th>Started at</th></tr>{sess_html}</table>

  <footer>Playora — {now}</footer>
</body></html>"#,
        css = CSS,
        play = fmt_dur(total_play),
        now = Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
    );
    Html(html)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
