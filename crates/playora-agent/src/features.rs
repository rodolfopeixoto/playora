use anyhow::Result;
use playora_common::*;
use std::time::Duration;

pub fn cmd_fetch(cfg: AgentConfig) -> Result<()> {
    let url = format!("{}/api/v1/devices/{}/manifest", cfg.server_url.trim_end_matches('/'), cfg.device_id);
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build()?;
    let resp = client.get(&url).send()?;
    let m: FeatureManifest = resp.json()?;
    let conn = crate::db::open(&crate::cfg::db_path())?;
    for (k, v) in &m.features {
        conn.execute(
            "INSERT INTO feature_flags(feature_key,status,source,payload_json,updated_at)
             VALUES (?1, ?2, 'server', ?3, datetime('now'))
             ON CONFLICT(feature_key) DO UPDATE SET status=excluded.status, source='server', payload_json=excluded.payload_json, updated_at=excluded.updated_at",
            rusqlite::params![k, format!("{:?}", v).to_lowercase(), serde_json::to_string(v)?],
        )?;
    }
    println!("{}", serde_json::to_string_pretty(&m)?);
    Ok(())
}

pub fn cmd_show(_cfg: AgentConfig) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let mut stmt = conn.prepare("SELECT feature_key, status, source, updated_at FROM feature_flags ORDER BY feature_key")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?)))?;
    for r in rows {
        let (k, s, src, ts) = r?;
        println!("  {k:<24} {s:<10} src={src} at={ts}");
    }
    Ok(())
}
