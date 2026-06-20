use crate::State;
use axum::{
    extract::{Path, Query, State as AxState},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use playora_common::*;
use serde::Deserialize;
use serde_json::Value;

pub async fn health() -> &'static str {
    "ok"
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub device_id: String,
    pub device_name: String,
    pub device_profile: String,
    pub os_family: String,
    pub agent_version: String,
}

pub async fn register(
    AxState(state): AxState<State>,
    Json(r): Json<RegisterRequest>,
) -> (StatusCode, Json<Value>) {
    let conn = state.lock().await;
    let now = Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT INTO devices(device_id,device_name,device_profile,os_family,agent_version,created_at,last_seen_at)
         VALUES (?1,?2,?3,?4,?5,?6,?6)
         ON CONFLICT(device_id) DO UPDATE SET device_name=excluded.device_name, device_profile=excluded.device_profile,
           agent_version=excluded.agent_version, last_seen_at=excluded.last_seen_at",
        rusqlite::params![r.device_id, r.device_name, r.device_profile, r.os_family, r.agent_version, now],
    );
    (StatusCode::OK, Json(serde_json::json!({"ok": true})))
}

pub async fn heartbeat(
    AxState(state): AxState<State>,
    Json(hb): Json<DeviceHeartbeat>,
) -> Json<Value> {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO heartbeats(device_id,wifi_connected,free_disk_mb,pending_events,received_at) VALUES (?1,?2,?3,?4,?5)",
        rusqlite::params!["", hb.wifi_connected as i32, hb.free_disk_mb as i64, hb.pending_events as i64, Utc::now().to_rfc3339()],
    );
    Json(serde_json::json!({"ok": true}))
}

pub async fn capabilities(
    AxState(state): AxState<State>,
    Json(c): Json<CapabilityReport>,
) -> Json<Value> {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO device_capabilities(device_id,payload_json,created_at) VALUES (?1,?2,?3)",
        rusqlite::params![
            "",
            serde_json::to_string(&c).unwrap_or_default(),
            Utc::now().to_rfc3339()
        ],
    );
    Json(serde_json::json!({"ok": true}))
}

pub async fn hardware_snapshot(
    AxState(state): AxState<State>,
    Json(s): Json<HardwareSnapshot>,
) -> Json<Value> {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO hardware_snapshots(device_id,payload_json,received_at) VALUES (?1,?2,?3)",
        rusqlite::params![
            "",
            serde_json::to_string(&s).unwrap_or_default(),
            Utc::now().to_rfc3339()
        ],
    );
    Json(serde_json::json!({"ok": true}))
}

pub async fn hardware_test_result(
    AxState(state): AxState<State>,
    Json(t): Json<HardwareTestResult>,
) -> Json<Value> {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO hardware_tests(device_id,test_type,status,score,payload_json,received_at) VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params!["", t.test_type, t.status, t.score.map(|x| x as f64), serde_json::to_string(&t.payload).unwrap_or_default(), Utc::now().to_rfc3339()],
    );
    Json(serde_json::json!({"ok": true}))
}

pub async fn resource_sample(
    AxState(state): AxState<State>,
    Json(s): Json<ResourceUsageSample>,
) -> Json<Value> {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO resource_samples(device_id,payload_json,received_at) VALUES (?1,?2,?3)",
        rusqlite::params![
            "",
            serde_json::to_string(&s).unwrap_or_default(),
            Utc::now().to_rfc3339()
        ],
    );
    Json(serde_json::json!({"ok": true}))
}

pub async fn events_batch(
    AxState(state): AxState<State>,
    Json(batch): Json<SyncBatch>,
) -> Json<SyncAck> {
    let conn = state.lock().await;
    let mut accepted = vec![];
    let mut duplicates = vec![];
    let mut rejected: Vec<(EventId, String)> = vec![];
    for ev in batch.events {
        let json = match serde_json::to_string(&ev) {
            Ok(s) => s,
            Err(e) => {
                rejected.push((ev.event_id.clone(), e.to_string()));
                continue;
            }
        };
        let etype = match &ev.payload {
            EventPayload::DeviceHeartbeat(_) => "device_heartbeat",
            EventPayload::HardwareSnapshot(_) => "hardware_snapshot",
            EventPayload::HardwareTestResult(_) => "hardware_test_result",
            EventPayload::ResourceSample(_) => "resource_sample",
            EventPayload::GameSessionStarted(_) => "game_session_started",
            EventPayload::GameSessionFinished(_) => "game_session_finished",
            EventPayload::RomScanned(_) => "rom_scanned",
            EventPayload::SaveSnapshot(_) => "save_snapshot",
        };
        let r = conn.execute(
            "INSERT INTO events(event_id,device_id,event_type,payload_json,received_at) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![ev.event_id.0, ev.device_id.0, etype, json, Utc::now().to_rfc3339()],
        );
        match r {
            Ok(_) => {
                accepted.push(ev.event_id.clone());
                if let EventPayload::GameSessionStarted(g) = &ev.payload {
                    let _ = conn.execute(
                        "INSERT OR REPLACE INTO game_sessions(session_id,device_id,system,game_name,rom_hash,core,started_at,ended_at,duration_seconds,max_cpu_percent,max_memory_mb)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,0,0,0)",
                        rusqlite::params![g.session_id.0, ev.device_id.0, format!("{:?}", g.system).to_lowercase(), g.game_name, g.rom_hash, g.core, g.started_at.to_rfc3339()],
                    );
                } else if let EventPayload::GameSessionFinished(g) = &ev.payload {
                    let _ = conn.execute(
                        "UPDATE game_sessions SET ended_at=?2, duration_seconds=?3, max_cpu_percent=?4, max_memory_mb=?5 WHERE session_id=?1",
                        rusqlite::params![g.session_id.0, g.ended_at.to_rfc3339(), g.duration_seconds as i64, g.max_cpu_percent.unwrap_or(0.0) as f64, g.max_memory_mb.unwrap_or(0) as i64],
                    );
                }
            }
            Err(e) if e.to_string().contains("UNIQUE") => duplicates.push(ev.event_id.clone()),
            Err(e) => rejected.push((ev.event_id.clone(), e.to_string())),
        }
    }
    Json(SyncAck {
        accepted,
        duplicates,
        rejected,
    })
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub limit: Option<u32>,
}

pub async fn events_list(
    AxState(state): AxState<State>,
    Query(q): Query<EventsQuery>,
) -> Json<Vec<Value>> {
    let conn = state.lock().await;
    let limit = q.limit.unwrap_or(50);
    let mut stmt = conn.prepare("SELECT event_id, device_id, event_type, received_at FROM events ORDER BY id DESC LIMIT ?1").unwrap();
    let rows = stmt
        .query_map([limit], |r| {
            Ok(serde_json::json!({
                "event_id": r.get::<_, String>(0)?,
                "device_id": r.get::<_, String>(1)?,
                "event_type": r.get::<_, String>(2)?,
                "received_at": r.get::<_, String>(3)?,
            }))
        })
        .unwrap();
    Json(rows.flatten().collect())
}

pub async fn devices_list(AxState(state): AxState<State>) -> Json<Vec<Value>> {
    let conn = state.lock().await;
    let mut stmt = conn.prepare("SELECT device_id, device_name, device_profile, os_family, last_seen_at FROM devices ORDER BY last_seen_at DESC").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "device_id": r.get::<_, String>(0)?,
                "device_name": r.get::<_, String>(1).unwrap_or_default(),
                "device_profile": r.get::<_, String>(2).unwrap_or_default(),
                "os_family": r.get::<_, String>(3).unwrap_or_default(),
                "last_seen_at": r.get::<_, String>(4).unwrap_or_default(),
            }))
        })
        .unwrap();
    Json(rows.flatten().collect())
}

pub async fn device_detail(AxState(state): AxState<State>, Path(id): Path<String>) -> Json<Value> {
    let conn = state.lock().await;
    let dev: Option<Value> = conn.query_row(
        "SELECT device_id, device_name, device_profile, os_family, agent_version, last_seen_at FROM devices WHERE device_id=?1",
        [id.clone()], |r| Ok(serde_json::json!({
            "device_id": r.get::<_,String>(0)?,
            "device_name": r.get::<_,String>(1).unwrap_or_default(),
            "device_profile": r.get::<_,String>(2).unwrap_or_default(),
            "os_family": r.get::<_,String>(3).unwrap_or_default(),
            "agent_version": r.get::<_,String>(4).unwrap_or_default(),
            "last_seen_at": r.get::<_,String>(5).unwrap_or_default(),
        }))
    ).ok();
    Json(dev.unwrap_or_else(|| serde_json::json!({"error":"not found","device_id":id})))
}

pub async fn manifest(
    AxState(state): AxState<State>,
    Path(id): Path<String>,
) -> Json<FeatureManifest> {
    let conn = state.lock().await;
    let mut stmt = conn
        .prepare("SELECT feature_key, status FROM device_features WHERE device_id=?1")
        .unwrap();
    let rows = stmt
        .query_map([id.clone()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .unwrap();
    let mut features = std::collections::BTreeMap::new();
    for r in rows.flatten() {
        let st = match r.1.as_str() {
            "enabled" => FeatureStatus::Enabled,
            "locked" => FeatureStatus::Locked,
            "planned" => FeatureStatus::Planned,
            _ => FeatureStatus::Disabled,
        };
        features.insert(r.0, st);
    }
    for (k, v) in [
        ("catalog", FeatureStatus::Enabled),
        ("rom_download", FeatureStatus::Enabled),
        ("cloud_save", FeatureStatus::Planned),
        ("netplay", FeatureStatus::Planned),
        ("runtime_probe", FeatureStatus::Disabled),
        ("hardware_tests", FeatureStatus::Enabled),
        ("beta_features", FeatureStatus::Planned),
        ("community", FeatureStatus::Enabled),
    ] {
        features.entry(k.into()).or_insert(v);
    }
    let mut requirements: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    requirements.insert(
        "catalog".into(),
        vec!["wifi_ok".into(), "storage_ok".into()],
    );
    requirements.insert(
        "runtime_probe".into(),
        vec!["tester_official".into(), "manual_enable".into()],
    );
    requirements.insert(
        "netplay".into(),
        vec!["wifi_ok".into(), "game_compatible".into()],
    );
    Json(FeatureManifest {
        device_id: DeviceId(id),
        features,
        requirements,
    })
}

pub async fn set_features(
    AxState(state): AxState<State>,
    Path(id): Path<String>,
    Json(map): Json<std::collections::BTreeMap<String, String>>,
) -> Json<Value> {
    let conn = state.lock().await;
    for (k, v) in map {
        let _ = conn.execute(
            "INSERT INTO device_features(device_id,feature_key,status,updated_at) VALUES (?1,?2,?3,?4)
             ON CONFLICT(device_id, feature_key) DO UPDATE SET status=excluded.status, updated_at=excluded.updated_at",
            rusqlite::params![id, k, v, Utc::now().to_rfc3339()],
        );
    }
    Json(serde_json::json!({"ok": true}))
}

pub async fn games_list(AxState(state): AxState<State>) -> Json<Vec<Value>> {
    let conn = state.lock().await;
    let mut stmt = conn.prepare("SELECT DISTINCT system, game_name FROM game_sessions WHERE game_name IS NOT NULL ORDER BY game_name LIMIT 200").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({"system": r.get::<_,String>(0)?, "name": r.get::<_,String>(1)?}))
        })
        .unwrap();
    Json(rows.flatten().collect())
}

pub async fn ranking_playtime(AxState(state): AxState<State>) -> Json<Vec<Value>> {
    let conn = state.lock().await;
    let mut stmt = conn.prepare("SELECT game_name, system, SUM(duration_seconds) total FROM game_sessions GROUP BY game_name, system ORDER BY total DESC LIMIT 25").unwrap();
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({
        "game": r.get::<_,String>(0)?, "system": r.get::<_,String>(1)?, "duration_seconds": r.get::<_,i64>(2)?
    }))).unwrap();
    Json(rows.flatten().collect())
}

pub async fn ranking_systems(AxState(state): AxState<State>) -> Json<Vec<Value>> {
    let conn = state.lock().await;
    let mut stmt = conn.prepare("SELECT system, SUM(duration_seconds) total FROM game_sessions GROUP BY system ORDER BY total DESC").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "system": r.get::<_,String>(0)?, "duration_seconds": r.get::<_,i64>(1)?
            }))
        })
        .unwrap();
    Json(rows.flatten().collect())
}

pub async fn catalog_list(AxState(state): AxState<State>) -> Json<Vec<Value>> {
    let conn = state.lock().await;
    let mut stmt = conn
        .prepare("SELECT id,title,system,type,license,author FROM catalog_items ORDER BY title")
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_,String>(0)?, "title": r.get::<_,String>(1)?,
                "system": r.get::<_,String>(2).unwrap_or_default(),
                "type": r.get::<_,String>(3).unwrap_or_default(),
                "license": r.get::<_,String>(4).unwrap_or_default(),
                "author": r.get::<_,String>(5).unwrap_or_default(),
            }))
        })
        .unwrap();
    Json(rows.flatten().collect())
}

pub async fn catalog_detail(AxState(state): AxState<State>, Path(id): Path<String>) -> Json<Value> {
    let conn = state.lock().await;
    let v: Option<Value> = conn
        .query_row(
            "SELECT id,title,system,type,license,author FROM catalog_items WHERE id=?1",
            [id.clone()],
            |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_,String>(0)?, "title": r.get::<_,String>(1)?,
                    "system": r.get::<_,String>(2).unwrap_or_default(),
                    "type": r.get::<_,String>(3).unwrap_or_default(),
                    "license": r.get::<_,String>(4).unwrap_or_default(),
                    "author": r.get::<_,String>(5).unwrap_or_default(),
                }))
            },
        )
        .ok();
    Json(v.unwrap_or_else(|| serde_json::json!({"error":"not found","id":id})))
}

pub async fn catalog_download(
    AxState(_state): AxState<State>,
    Path(id): Path<String>,
) -> Json<Value> {
    Json(serde_json::json!({
        "id": id,
        "note": "download URL must come from catalog item; this MVP returns metadata only."
    }))
}

pub async fn saves_upload(
    Query(q): Query<std::collections::HashMap<String, String>>,
    body: axum::body::Bytes,
) -> (StatusCode, String) {
    let device_id = q
        .get("device_id")
        .cloned()
        .unwrap_or_else(|| "unknown".into());
    let base = std::env::var("PLAYORA_SAVES_DIR").unwrap_or_else(|_| "./saves".into());
    let dir = std::path::PathBuf::from(&base).join(&device_id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("mkdir: {e}"));
    }
    let ts = Utc::now().format("%Y%m%d_%H%M%S");
    let path = dir.join(format!("saves_{ts}.tar.gz"));
    if let Err(e) = std::fs::write(&path, &body) {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("write: {e}"));
    }
    (
        StatusCode::OK,
        format!("stored {} bytes -> {}", body.len(), path.display()),
    )
}

pub async fn sources_list() -> Json<Vec<playora_common::sources::RomSource>> {
    Json(playora_common::sources::built_in())
}

pub async fn systems_list() -> Json<Vec<Value>> {
    let v: Vec<Value> = playora_common::systems::SYSTEMS
        .iter()
        .map(|s| {
            serde_json::json!({
                "folder": s.folder,
                "name": s.display_name,
                "extensions": s.extensions,
                "default_emulator": s.default_emulator,
                "retroarch_core": s.retroarch_core,
            })
        })
        .collect();
    Json(v)
}

pub async fn downloads_report(
    AxState(state): AxState<State>,
    Json(r): Json<DownloadReport>,
) -> Json<Value> {
    let conn = state.lock().await;
    let _ = conn.execute(
        "INSERT INTO downloads(device_id,catalog_item_id,status,payload_json,received_at) VALUES (?1,?2,?3,?4,?5)",
        rusqlite::params!["", r.catalog_item_id, r.status, serde_json::to_string(&r).unwrap_or_default(), Utc::now().to_rfc3339()],
    );
    Json(serde_json::json!({"ok": true}))
}
