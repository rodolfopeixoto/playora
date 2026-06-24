use darkos_core::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmwareEntry {
    pub name: String,
    pub vendor: String,
    pub variant_url: String,
    pub download_urls: Vec<String>,
    pub sha1_or_sha256: Option<String>,
    pub notes: Option<String>,
}

pub fn known_firmwares() -> Vec<FirmwareEntry> {
    vec![
        FirmwareEntry {
            name: "dArkOSRE-R36".into(),
            vendor: "southoz".into(),
            variant_url: "https://github.com/southoz/dArkOSRE-R36".into(),
            download_urls: vec![
                "https://drive.google.com/file/d/1ONnNxR3cpGAC0d5YefS-xE-Hp1ph7Hm-/view?usp=sharing".into(),
                "https://mega.nz/file/k6AgTSTS#RrMGot_xVXyzAr5h_7RDNKFIv2GaKniLYliLSPA3UWc".into(),
            ],
            sha1_or_sha256: Some("a4858eee2f1eced10d3ce90c911d89450eea700f".into()),
            notes: Some("Best for R36S clones with sitronix,st7703 panel. Has built-in panel selector.".into()),
        },
        FirmwareEntry {
            name: "dArkOS".into(),
            vendor: "christianhaitian".into(),
            variant_url: "https://github.com/christianhaitian/dArkOS".into(),
            download_urls: vec![],
            sha1_or_sha256: None,
            notes: Some("Upstream Debian-based ArkOS. RG351MP variant did NOT boot on our clone hw (U-Boot SPL incompatibility).".into()),
        },
    ]
}

pub fn current_firmware_string() -> Result<String> {
    for f in [
        "/etc/os-release",
        "/usr/share/plymouth/themes/text.plymouth",
    ] {
        if let Ok(s) = std::fs::read_to_string(f) {
            for line in s.lines() {
                if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
                    return Ok(v.trim_matches('"').to_string());
                }
                if let Some(v) = line.strip_prefix("title=") {
                    return Ok(v.to_string());
                }
            }
        }
    }
    Err(Error::NotFound("firmware identifier".into()))
}

pub fn lookup(name: &str) -> Option<FirmwareEntry> {
    known_firmwares().into_iter().find(|f| f.name == name)
}

pub fn default_stage_dir() -> PathBuf {
    let home = std::env::var("DARKOS_HOME").unwrap_or_else(|_| "/roms/.darkOs".into());
    PathBuf::from(home).join("firmware")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchReport {
    pub name: String,
    pub source_url: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
    pub sha256_verified: bool,
}

pub fn resolve_url(entry: &FirmwareEntry) -> Result<String> {
    if let Ok(base) = std::env::var("DARKOS_FIRMWARE_BASE_URL") {
        return Ok(format!(
            "{}/{}.img.gz",
            base.trim_end_matches('/'),
            entry.name
        ));
    }
    entry
        .download_urls
        .iter()
        .find(|u| !(u.contains("drive.google.com") || u.contains("mega.nz")))
        .cloned()
        .ok_or_else(|| {
            Error::Other(format!(
                "no direct download URL for {} (Google Drive / MEGA cannot be fetched headlessly). \
                 Set DARKOS_FIRMWARE_BASE_URL=https://your.mirror to override.",
                entry.name
            ))
        })
}

pub fn fetch(name: &str, dest_dir: Option<&Path>) -> Result<FetchReport> {
    let entry = lookup(name).ok_or_else(|| Error::NotFound(format!("firmware {name}")))?;
    let url = resolve_url(&entry)?;

    let dest_dir = match dest_dir {
        Some(p) => p.to_path_buf(),
        None => default_stage_dir(),
    };
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| Error::Other(format!("mkdir {}: {e}", dest_dir.display())))?;

    let file_name = url.rsplit('/').next().unwrap_or("firmware.img.gz");
    let path = dest_dir.join(file_name);

    let mut reader = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(900))
        .call()
        .map_err(|e| Error::Other(format!("download {url}: {e}")))?
        .into_reader();

    let tmp = path.with_extension("partial");
    let mut f = std::fs::File::create(&tmp)
        .map_err(|e| Error::Other(format!("open {}: {e}", tmp.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::Other(format!("read body: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        std::io::Write::write_all(&mut f, &buf[..n])
            .map_err(|e| Error::Other(format!("write tmp: {e}")))?;
        total += n as u64;
    }
    drop(f);
    std::fs::rename(&tmp, &path)
        .map_err(|e| Error::Other(format!("rename to {}: {e}", path.display())))?;

    let sha256 = format!("{:x}", hasher.finalize());
    let verified = match entry.sha1_or_sha256.as_deref() {
        Some(expected) if expected.len() == 64 => sha256.eq_ignore_ascii_case(expected),
        _ => false,
    };

    Ok(FetchReport {
        name: entry.name,
        source_url: url,
        path,
        bytes: total,
        sha256,
        sha256_verified: verified,
    })
}

pub fn check(name: &str) -> Result<(String, FirmwareEntry, bool)> {
    let current = current_firmware_string().unwrap_or_else(|_| "<unknown>".into());
    let entry = lookup(name).ok_or_else(|| Error::NotFound(format!("firmware {name}")))?;
    let installed = current.to_lowercase().contains(&entry.name.to_lowercase());
    Ok((current, entry, installed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn lookup_returns_known() {
        assert!(lookup("dArkOSRE-R36").is_some());
        assert!(lookup("nope").is_none());
    }

    #[test]
    fn resolve_url_paths() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("DARKOS_FIRMWARE_BASE_URL");
        let entry = lookup("dArkOSRE-R36").unwrap();
        assert!(resolve_url(&entry).is_err());

        std::env::set_var("DARKOS_FIRMWARE_BASE_URL", "https://example.com/fw");
        let url = resolve_url(&entry).unwrap();
        assert!(url.starts_with("https://example.com/fw/dArkOSRE-R36"));
        std::env::remove_var("DARKOS_FIRMWARE_BASE_URL");
    }
}
