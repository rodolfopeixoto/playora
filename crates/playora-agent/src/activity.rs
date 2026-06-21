use anyhow::Result;
use chrono::Utc;
use playora_common::*;

pub fn begin(cfg: &AgentConfig, script: &str) -> Result<()> {
    enqueue(
        cfg,
        Activity {
            script: script.into(),
            status: ActivityStatus::Running,
            started_at: Utc::now(),
            ended_at: None,
            exit_code: None,
            log_path: None,
        },
    )
}

pub fn end(cfg: &AgentConfig, script: &str, exit_code: i32) -> Result<()> {
    enqueue(
        cfg,
        Activity {
            script: script.into(),
            status: if exit_code == 0 {
                ActivityStatus::Ok
            } else {
                ActivityStatus::Fail
            },
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            exit_code: Some(exit_code),
            log_path: None,
        },
    )
}

fn enqueue(cfg: &AgentConfig, activity: Activity) -> Result<()> {
    let conn = crate::db::open(&crate::cfg::db_path())?;
    let ev = Event {
        event_id: EventId::new(),
        device_id: cfg.device_id.clone(),
        created_at: Utc::now(),
        payload: EventPayload::Activity(activity),
    };
    crate::db::enqueue(&conn, &ev)?;
    let _ = crate::sync::cmd_sync_once(cfg.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;
    use tempfile::tempdir;
    static TMP: OnceLock<tempfile::TempDir> = OnceLock::new();

    #[test]
    fn enqueue_round_trip() {
        let dir = TMP.get_or_init(|| tempdir().unwrap());
        let cfg_path = dir.path().join("agent.toml");
        std::env::set_var("HOME", dir.path());
        let _ = crate::cfg::cmd_init(
            Some(cfg_path.to_string_lossy().as_ref()),
            Some("http://127.0.0.1:1".into()),
            None,
        );
        let cfg = crate::cfg::load(Some(cfg_path.to_string_lossy().as_ref())).unwrap();
        assert!(begin(&cfg, "Test").is_ok());
        let db = crate::db::open(&crate::cfg::db_path()).unwrap();
        assert!(crate::db::count_pending(&db).unwrap() >= 1);
    }
}
