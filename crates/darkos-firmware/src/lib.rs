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
    #[serde(default)]
    pub release_image: Option<ReleaseImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseImage {
    pub file_name: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub parts: Vec<String>,
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
            release_image: load_release_image_from_env("dArkOSRE-R36"),
        },
        FirmwareEntry {
            name: "dArkOS".into(),
            vendor: "christianhaitian".into(),
            variant_url: "https://github.com/christianhaitian/dArkOS".into(),
            download_urls: vec![],
            sha1_or_sha256: None,
            notes: Some("Upstream Debian-based ArkOS. RG351MP variant did NOT boot on our clone hw (U-Boot SPL incompatibility).".into()),
            release_image: load_release_image_from_env("dArkOS"),
        },
    ]
}

fn load_release_image_from_env(_name: &str) -> Option<ReleaseImage> {
    let manifest_path = std::env::var("DARKOS_FIRMWARE_MANIFEST").ok()?;
    let body = std::fs::read_to_string(manifest_path).ok()?;
    serde_json::from_str::<ReleaseImage>(&body).ok()
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

pub fn default_manifest_path(name: &str) -> PathBuf {
    let home = std::env::var("DARKOS_HOME").unwrap_or_else(|_| "/roms/.darkOs".into());
    PathBuf::from(home).join(format!("firmware-manifest-{name}.json"))
}

pub fn default_manifest_url(name: &str) -> String {
    std::env::var("DARKOS_FIRMWARE_MANIFEST_URL").unwrap_or_else(|_| {
        format!(
            "https://github.com/rodolfopeixoto/playora/releases/latest/download/firmware-manifest-{name}.json"
        )
    })
}

pub fn refresh_manifest(name: &str, dest: Option<&Path>) -> Result<(PathBuf, ReleaseImage)> {
    let url = default_manifest_url(name);
    let body = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| Error::Other(format!("fetch manifest {url}: {e}")))?
        .into_string()
        .map_err(|e| Error::Other(format!("read manifest body: {e}")))?;
    let parsed: ReleaseImage =
        serde_json::from_str(&body).map_err(|e| Error::Other(format!("parse manifest: {e}")))?;
    let path = match dest {
        Some(p) => p.to_path_buf(),
        None => default_manifest_path(name),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Other(format!("mkdir {}: {e}", parent.display())))?;
    }
    std::fs::write(&path, &body)
        .map_err(|e| Error::Other(format!("write {}: {e}", path.display())))?;
    Ok((path, parsed))
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
    let dest_dir = match dest_dir {
        Some(p) => p.to_path_buf(),
        None => default_stage_dir(),
    };
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| Error::Other(format!("mkdir {}: {e}", dest_dir.display())))?;

    if let Some(img) = &entry.release_image {
        return fetch_release_image(&entry, img, &dest_dir);
    }

    let url = resolve_url(&entry)?;
    let file_name = url.rsplit('/').next().unwrap_or("firmware.img.gz");
    let path = dest_dir.join(file_name);

    let mut f = std::fs::File::create(path.with_extension("partial"))
        .map_err(|e| Error::Other(format!("open tmp: {e}")))?;
    let mut hasher = Sha256::new();
    let total = stream_to(&url, &mut f, &mut hasher)?;
    drop(f);
    std::fs::rename(path.with_extension("partial"), &path)
        .map_err(|e| Error::Other(format!("rename to {}: {e}", path.display())))?;

    let sha256 = format!("{:x}", hasher.finalize());
    let verified = matches!(entry.sha1_or_sha256.as_deref(), Some(expected) if expected.len() == 64 && sha256.eq_ignore_ascii_case(expected));

    Ok(FetchReport {
        name: entry.name,
        source_url: url,
        path,
        bytes: total,
        sha256,
        sha256_verified: verified,
    })
}

fn fetch_release_image(
    entry: &FirmwareEntry,
    img: &ReleaseImage,
    dest_dir: &Path,
) -> Result<FetchReport> {
    let path = dest_dir.join(&img.file_name);
    let tmp = path.with_extension("partial");
    let mut f = std::fs::File::create(&tmp)
        .map_err(|e| Error::Other(format!("open {}: {e}", tmp.display())))?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let n_parts = img.parts.len();
    for (i, url) in img.parts.iter().enumerate() {
        eprintln!("[fetch] part {}/{}: {}", i + 1, n_parts, url);
        total += stream_to(url, &mut f, &mut hasher)?;
    }
    drop(f);
    let sha256 = format!("{:x}", hasher.finalize());
    if !sha256.eq_ignore_ascii_case(&img.sha256) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Other(format!(
            "sha256 mismatch after {n_parts} parts: got {sha256}, expected {}",
            img.sha256
        )));
    }
    if img.size_bytes != 0 && img.size_bytes != total {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Other(format!(
            "size mismatch: got {total}, expected {}",
            img.size_bytes
        )));
    }
    std::fs::rename(&tmp, &path)
        .map_err(|e| Error::Other(format!("rename to {}: {e}", path.display())))?;
    Ok(FetchReport {
        name: entry.name.clone(),
        source_url: img.parts.first().cloned().unwrap_or_default(),
        path,
        bytes: total,
        sha256,
        sha256_verified: true,
    })
}

fn stream_to<W: std::io::Write>(url: &str, out: &mut W, hasher: &mut Sha256) -> Result<u64> {
    let mut reader = ureq::get(url)
        .timeout(std::time::Duration::from_secs(1800))
        .call()
        .map_err(|e| Error::Other(format!("download {url}: {e}")))?
        .into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut total: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::Other(format!("read {url}: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| Error::Other(format!("write: {e}")))?;
        total += n as u64;
    }
    Ok(total)
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
