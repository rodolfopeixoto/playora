//! Single-instance lockfile for heavy commands (scan, extract-roms).
//!
//! Writes /tmp/playora-<name>.lock with current PID. If the file exists
//! and its PID is still alive, [`acquire`] returns an error. On drop the
//! file is removed.

use anyhow::{bail, Result};
use std::path::PathBuf;

pub struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn acquire(name: &str) -> Result<Lock> {
    let safe: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect();
    let path = PathBuf::from(format!("/tmp/playora-{safe}.lock"));
    if path.exists() {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(pid) = s.trim().parse::<i32>() {
                if pid_alive(pid) {
                    bail!(
                        "another '{name}' is already running (pid {pid}). Wait for it or `kill {pid}`."
                    );
                }
            }
        }
        // Stale lockfile — remove and reacquire.
        std::fs::remove_file(&path).ok();
    }
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(Lock { path })
}

fn pid_alive(pid: i32) -> bool {
    // /proc/<pid> exists → process alive (Linux only; on macOS we just return true to be safe)
    if cfg!(target_os = "linux") {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    } else {
        // On non-Linux dev hosts, treat any pid >0 as alive — tests use unique names.
        pid > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_releases_on_drop() {
        let name = format!("test-acquire-{}", std::process::id());
        let p = format!("/tmp/playora-{name}.lock");
        {
            let _l = acquire(&name).unwrap();
            assert!(std::path::Path::new(&p).exists());
        }
        assert!(!std::path::Path::new(&p).exists());
    }
}
