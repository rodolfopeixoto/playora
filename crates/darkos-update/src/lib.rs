//! OS package updates (apt) + self-update of darkOs CLI binary.

use darkos_core::Result;
use std::process::Command;

pub fn apt_update_available() -> Result<u32> {
    // best-effort: parses `apt list --upgradable` if available
    let out = Command::new("sh")
        .arg("-c")
        .arg("apt list --upgradable 2>/dev/null | tail -n +2 | wc -l")
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            Ok(s.parse::<u32>().unwrap_or(0))
        }
        _ => Ok(0),
    }
}

pub fn run_apt_upgrade(dry_run: bool) -> Result<String> {
    let cmd = if dry_run {
        "apt-get -s upgrade"
    } else {
        "sudo apt-get update && sudo apt-get -y upgrade"
    };
    let out = Command::new("sh").arg("-c").arg(cmd).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Live OTA flash of root device is risky on RK3326 — staged for offline PC flow only.
pub fn stage_firmware_image(_url: &str, _dest: &std::path::Path) -> Result<()> {
    Err(darkos_core::Error::Other(
        "live firmware flash not yet implemented — use macOS-side scripts/AUTO_flash_*.sh".into(),
    ))
}

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEFAULT_RELEASE_URL: &str =
    "https://github.com/rodolfopeixoto/playora/releases/latest/download/latest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub version: String,
    pub binary_url: String,
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
}

pub struct SelfUpdateReport {
    pub current: String,
    pub remote: ReleaseManifest,
    pub upgraded: bool,
    pub installed_path: PathBuf,
}

pub fn fetch_manifest(url: Option<&str>) -> Result<ReleaseManifest> {
    let url = url
        .map(str::to_string)
        .or_else(|| std::env::var("DARKOS_RELEASE_URL").ok())
        .unwrap_or_else(|| DEFAULT_RELEASE_URL.to_string());
    let body: String = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| darkos_core::Error::Other(format!("fetch manifest: {e}")))?
        .into_string()
        .map_err(|e| darkos_core::Error::Other(format!("read manifest: {e}")))?;
    let m: ReleaseManifest = serde_json::from_str(&body)
        .map_err(|e| darkos_core::Error::Other(format!("parse manifest: {e}")))?;
    Ok(m)
}

pub fn is_newer(current: &str, remote: &str) -> bool {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let mut it = s
            .trim_start_matches('v')
            .split(|c: char| !c.is_ascii_digit());
        let a = it.next()?.parse().ok()?;
        let b = it.next().unwrap_or("0").parse().ok()?;
        let c = it.next().unwrap_or("0").parse().ok()?;
        Some((a, b, c))
    };
    match (parse(current), parse(remote)) {
        (Some(a), Some(b)) => b > a,
        _ => remote != current,
    }
}

pub fn apply(m: &ReleaseManifest, install_path: Option<&Path>) -> Result<PathBuf> {
    let install_path = match install_path {
        Some(p) => p.to_path_buf(),
        None => std::env::current_exe()
            .map_err(|e| darkos_core::Error::Other(format!("current_exe: {e}")))?,
    };

    let mut reader = ureq::get(&m.binary_url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| darkos_core::Error::Other(format!("download: {e}")))?
        .into_reader();
    let mut bytes = Vec::with_capacity(4 * 1024 * 1024);
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| darkos_core::Error::Other(format!("read body: {e}")))?;

    let mut h = Sha256::new();
    h.update(&bytes);
    let got = format!("{:x}", h.finalize());
    if !got.eq_ignore_ascii_case(m.sha256.trim()) {
        return Err(darkos_core::Error::Other(format!(
            "sha256 mismatch: got {got}, expected {}",
            m.sha256
        )));
    }

    let parent = install_path
        .parent()
        .ok_or_else(|| darkos_core::Error::Other("install path has no parent".into()))?;
    let tmp = parent.join(format!(".darkos.upd.{}", std::process::id()));
    std::fs::write(&tmp, &bytes)
        .map_err(|e| darkos_core::Error::Other(format!("write tmp: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&tmp)
            .map_err(|e| darkos_core::Error::Other(format!("stat tmp: {e}")))?
            .permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&tmp, perm)
            .map_err(|e| darkos_core::Error::Other(format!("chmod tmp: {e}")))?;
    }
    std::fs::rename(&tmp, &install_path)
        .map_err(|e| darkos_core::Error::Other(format!("atomic replace: {e}")))?;
    Ok(install_path)
}

pub fn run_self_update(
    current_version: &str,
    url: Option<&str>,
    install_path: Option<&Path>,
    force: bool,
) -> Result<SelfUpdateReport> {
    let remote = fetch_manifest(url)?;
    let upgraded = force || is_newer(current_version, &remote.version);
    let installed_path = if upgraded {
        apply(&remote, install_path)?
    } else {
        install_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_exe().unwrap_or_default())
    };
    Ok(SelfUpdateReport {
        current: current_version.to_string(),
        remote,
        upgraded,
        installed_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_semver() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("v0.1.0", "v0.1.2"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
    }

    #[test]
    fn newer_fallback_string() {
        assert!(is_newer("nightly-1", "nightly-2"));
        assert!(!is_newer("nightly-2", "nightly-2"));
    }
}
