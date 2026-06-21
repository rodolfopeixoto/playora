use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use std::fs;
use std::path::Path;

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
            summary: Some(format!("starting {script}")),
            stdout_tail: None,
        },
    )
}

/// Mid-run status update. Server upserts the still-running row by (device, script).
pub fn progress(cfg: &AgentConfig, script: &str, summary: &str) -> Result<()> {
    enqueue(
        cfg,
        Activity {
            script: script.into(),
            status: ActivityStatus::Running,
            started_at: Utc::now(),
            ended_at: None,
            exit_code: None,
            log_path: None,
            summary: Some(summary.into()),
            stdout_tail: None,
        },
    )
}

pub fn end(cfg: &AgentConfig, script: &str, exit_code: i32, log_path: Option<&str>) -> Result<()> {
    let (summary, tail) = match log_path {
        Some(p) => read_summary_and_tail(p, 40),
        None => (None, None),
    };
    let summary = summary.or(Some(if exit_code == 0 {
        format!("{script} ok")
    } else {
        format!("{script} failed (exit {exit_code})")
    }));
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
            log_path: log_path.map(|s| s.to_string()),
            summary,
            stdout_tail: tail,
        },
    )
}

fn read_summary_and_tail(path: &str, n: usize) -> (Option<String>, Option<String>) {
    let p = Path::new(path);
    if !p.exists() {
        return (None, None);
    }
    let s = match fs::read_to_string(p) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    let lines: Vec<&str> = s.lines().collect();
    let tail_start = lines.len().saturating_sub(n);
    let tail = lines[tail_start..].join("\n");
    let summary = lines
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string());
    (summary, Some(tail))
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
        unsafe {
            std::env::set_var("HOME", dir.path());
        }
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
