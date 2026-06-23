use playora_common::*;
use std::collections::BTreeMap;

fn wait_health(client: &reqwest::blocking::Client, url: &str) -> bool {
    for _ in 0..100 {
        if let Ok(r) = client.get(url).send() {
            if r.status().is_success() {
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

#[test]
fn server_full_cycle() {
    let bin = env!("CARGO_BIN_EXE_playora-server");
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("s.db");
    let port = 17_200 + std::process::id() as u16 % 1_000;
    let bind = format!("127.0.0.1:{port}");
    let mut child = std::process::Command::new(bin)
        .arg("--db")
        .arg(&db)
        .arg("--bind")
        .arg(&bind)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn server");

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let base = format!("http://{bind}");

    assert!(
        wait_health(&client, &format!("{base}/health")),
        "server did not become healthy in time"
    );
    assert_eq!(
        client
            .get(format!("{base}/health"))
            .send()
            .unwrap()
            .text()
            .unwrap_or_default(),
        "ok"
    );

    let dev = DeviceId::new();
    let batch = SyncBatch {
        device_id: dev.clone(),
        agent_version: "test".into(),
        events: vec![Event {
            event_id: EventId::new(),
            device_id: dev.clone(),
            created_at: chrono::Utc::now(),
            payload: EventPayload::GameSessionStarted(GameSessionStarted {
                session_id: SessionId::new(),
                system: GameSystem::Snes,
                game_name: "Integration".into(),
                rom_path: "/roms/snes/integration.sfc".into(),
                rom_hash: None,
                core: None,
                started_at: chrono::Utc::now(),
            }),
        }],
    };
    let resp = client
        .post(format!("{base}/api/v1/events/batch"))
        .json(&batch)
        .send()
        .unwrap();
    assert!(resp.status().is_success());
    let ack: SyncAck = resp.json().unwrap();
    assert_eq!(ack.accepted.len(), 1);
    assert!(ack.duplicates.is_empty());

    let resp2 = client
        .post(format!("{base}/api/v1/events/batch"))
        .json(&batch)
        .send()
        .unwrap();
    let ack2: SyncAck = resp2.json().unwrap();
    assert_eq!(ack2.duplicates.len(), 1, "deduplication");

    let sources: Vec<playora_common::sources::RomSource> = client
        .get(format!("{base}/api/v1/sources"))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert!(!sources.is_empty());

    let systems: serde_json::Value = client
        .get(format!("{base}/api/v1/systems"))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert!(systems.as_array().map(|a| !a.is_empty()).unwrap_or(false));

    let mut map = BTreeMap::new();
    map.insert("netplay".to_string(), "enabled".to_string());
    let upd = client
        .put(format!("{base}/api/v1/devices/{}/features", dev))
        .json(&map)
        .send()
        .unwrap();
    assert!(upd.status().is_success());

    let manifest: FeatureManifest = client
        .get(format!("{base}/api/v1/devices/{}/manifest", dev))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        manifest.features.get("netplay"),
        Some(&FeatureStatus::Enabled)
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn sse_stream_emits_new_events() {
    use std::io::Read;
    use std::time::{Duration, Instant};

    let bin = env!("CARGO_BIN_EXE_playora-server");
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("s.db");
    let port = 17_300 + std::process::id() as u16 % 1_000;
    let bind = format!("127.0.0.1:{port}");
    let mut child = std::process::Command::new(bin)
        .arg("--db")
        .arg(&db)
        .arg("--bind")
        .arg(&bind)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn server");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let base = format!("http://{bind}");
    assert!(wait_health(&client, &format!("{base}/health")));

    let dev = DeviceId::new();

    // Open SSE stream first so polling baseline = empty.
    let stream_url = format!("{base}/api/v1/devices/{}/events/stream", dev);
    let mut resp = client.get(&stream_url).send().unwrap();

    // Push event from background thread.
    let client2 = client.clone();
    let base2 = base.clone();
    let dev2 = dev.clone();
    let pusher = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let batch = SyncBatch {
            device_id: dev2.clone(),
            agent_version: "sse-test".into(),
            events: vec![Event {
                event_id: EventId::new(),
                device_id: dev2,
                created_at: chrono::Utc::now(),
                payload: EventPayload::GameSessionStarted(GameSessionStarted {
                    session_id: SessionId::new(),
                    system: GameSystem::Snes,
                    game_name: "SSE".into(),
                    rom_path: "/roms/snes/sse.sfc".into(),
                    rom_hash: None,
                    core: None,
                    started_at: chrono::Utc::now(),
                }),
            }],
        };
        let _ = client2
            .post(format!("{base2}/api/v1/events/batch"))
            .json(&batch)
            .send();
    });

    let mut buf = [0u8; 4096];
    let mut received = String::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        match resp.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                received.push_str(&String::from_utf8_lossy(&buf[..n]));
                if received.contains("game_session_started") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = pusher.join();

    assert!(
        received.contains("game_session_started"),
        "SSE stream did not emit event in time. got: {received}"
    );

    let _ = child.kill();
    let _ = child.wait();
}
