//! Firmware metadata + (future) OTA install.
//! For now: lists known firmwares + can probe currently-installed one.

use darkos_core::{Error, Result};
use serde::{Deserialize, Serialize};

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
