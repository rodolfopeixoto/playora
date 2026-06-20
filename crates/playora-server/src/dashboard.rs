use crate::State;
use axum::{extract::State as AxState, response::Html};
use chrono::Utc;

pub async fn page(AxState(state): AxState<State>) -> Html<String> {
    let conn = state.lock().await;
    let devices: i64 = conn.query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0)).unwrap_or(0);
    let events: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap_or(0);
    let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM game_sessions", [], |r| r.get(0)).unwrap_or(0);
    let last_hb: String = conn.query_row("SELECT received_at FROM heartbeats ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).unwrap_or_else(|_| "—".into());
    let snapshots: i64 = conn.query_row("SELECT COUNT(*) FROM hardware_snapshots", [], |r| r.get(0)).unwrap_or(0);
    let samples: i64 = conn.query_row("SELECT COUNT(*) FROM resource_samples", [], |r| r.get(0)).unwrap_or(0);
    let downloads: i64 = conn.query_row("SELECT COUNT(*) FROM downloads", [], |r| r.get(0)).unwrap_or(0);

    let mut ranking_html = String::new();
    let mut stmt = conn.prepare("SELECT game_name, system, SUM(duration_seconds) FROM game_sessions WHERE game_name IS NOT NULL GROUP BY game_name, system ORDER BY 3 DESC LIMIT 10").unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,i64>(2)?))).unwrap();
    for row in rows.flatten() {
        ranking_html.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}s</td></tr>", esc(&row.0), esc(&row.1), row.2));
    }
    if ranking_html.is_empty() { ranking_html.push_str("<tr><td colspan=3>no sessions yet</td></tr>"); }

    let mut devices_html = String::new();
    let mut stmt = conn.prepare("SELECT device_id, device_name, device_profile, last_seen_at FROM devices ORDER BY last_seen_at DESC LIMIT 25").unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1).unwrap_or_default(), r.get::<_,String>(2).unwrap_or_default(), r.get::<_,String>(3).unwrap_or_default()))).unwrap();
    for row in rows.flatten() {
        devices_html.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>", esc(&row.0), esc(&row.1), esc(&row.2), esc(&row.3)));
    }
    if devices_html.is_empty() { devices_html.push_str("<tr><td colspan=4>no devices yet</td></tr>"); }

    let mut events_html = String::new();
    let mut stmt = conn.prepare("SELECT event_id, device_id, event_type, received_at FROM events ORDER BY id DESC LIMIT 20").unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?))).unwrap();
    for row in rows.flatten() {
        events_html.push_str(&format!("<tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>", esc(&row.0), esc(&row.1), esc(&row.2), esc(&row.3)));
    }
    if events_html.is_empty() { events_html.push_str("<tr><td colspan=4>no events yet</td></tr>"); }

    let html = format!(r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Playora Hub</title>
<meta http-equiv="refresh" content="10">
<style>
  body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;background:#0e0e10;color:#e6e6e6;margin:0;padding:24px}}
  h1{{margin:0 0 16px 0}}
  .grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;margin-bottom:24px}}
  .card{{background:#1b1b1f;border:1px solid #2a2a30;border-radius:8px;padding:16px}}
  .card .v{{font-size:28px;font-weight:600;margin-top:4px}}
  .card .l{{color:#8a8a90;font-size:12px;text-transform:uppercase;letter-spacing:.5px}}
  table{{width:100%;border-collapse:collapse;margin:12px 0;background:#15151a}}
  th,td{{padding:8px 10px;border-bottom:1px solid #25252a;font-size:13px;text-align:left;vertical-align:top}}
  th{{color:#8a8a90;font-weight:500}}
  section{{margin-bottom:24px}} h2{{font-size:16px;color:#aaa;margin-bottom:8px}}
  code{{color:#9ad;font-family:ui-monospace,monospace;font-size:12px}}
  footer{{color:#666;font-size:11px;margin-top:32px}}
</style></head>
<body>
  <h1>Playora Hub</h1>

  <div class="grid">
    <div class="card"><div class="l">Devices</div><div class="v">{devices}</div></div>
    <div class="card"><div class="l">Events</div><div class="v">{events}</div></div>
    <div class="card"><div class="l">Sessions</div><div class="v">{sessions}</div></div>
    <div class="card"><div class="l">HW snapshots</div><div class="v">{snapshots}</div></div>
    <div class="card"><div class="l">Resource samples</div><div class="v">{samples}</div></div>
    <div class="card"><div class="l">Downloads</div><div class="v">{downloads}</div></div>
    <div class="card"><div class="l">Last heartbeat</div><div class="v" style="font-size:14px">{last_hb}</div></div>
  </div>

  <section>
    <h2>Ranking — top tempo jogado</h2>
    <table><tr><th>Jogo</th><th>Sistema</th><th>Duração total</th></tr>{ranking_html}</table>
  </section>

  <section>
    <h2>Devices</h2>
    <table><tr><th>ID</th><th>Name</th><th>Profile</th><th>Last seen</th></tr>{devices_html}</table>
  </section>

  <section>
    <h2>Latest events</h2>
    <table><tr><th>Event</th><th>Device</th><th>Type</th><th>Received</th></tr>{events_html}</table>
  </section>

  <footer>Playora MVP — {now}</footer>
</body></html>"#,
        now = Utc::now()
    );
    Html(html)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
