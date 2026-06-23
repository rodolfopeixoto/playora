//! Hardware probe — runtime facts about the R36S clone.
//!
//! Lê dados de /proc, /sys e sysinfo. No macOS (dev host) cai em stubs.

use darkos_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub soc: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub mem_total_mb: u64,
    pub mem_free_mb: u64,
    pub kernel: String,
    pub uptime_s: u64,
    pub temp_c: Option<f32>,
    pub cpu_freq_mhz: Option<u32>,
    pub gpu_freq_mhz: Option<u32>,
    pub gov: Option<String>,
    pub panel_compat: Option<String>,
    pub panel_res: Option<(u32, u32)>,
    pub dtb_path: Option<String>,
    pub firmware: Option<String>,
}

/// Read /proc/cpuinfo "Hardware:" — this is the string dArkOSRE uses to pick DTB.
pub fn hardware_string() -> Result<String> {
    let p = Path::new("/proc/cpuinfo");
    if !p.exists() {
        return Err(Error::Hw("/proc/cpuinfo not present (dev host?)".into()));
    }
    let s = std::fs::read_to_string(p)?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("Hardware") {
            if let Some(v) = rest.split(':').nth(1) {
                return Ok(v.trim().to_string());
            }
        }
    }
    Err(Error::Hw("Hardware: line not found".into()))
}

pub fn read_temp_c() -> Option<f32> {
    let zone = Path::new("/sys/class/thermal/thermal_zone0/temp");
    if !zone.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(zone).ok()?;
    raw.trim().parse::<f32>().ok().map(|v| v / 1000.0)
}

pub fn read_cpu_freq_mhz() -> Option<u32> {
    let f = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq";
    let raw = std::fs::read_to_string(f).ok()?;
    raw.trim().parse::<u32>().ok().map(|v| v / 1000)
}

pub fn read_gpu_freq_mhz() -> Option<u32> {
    for c in [
        "/sys/devices/platform/ff400000.gpu/devfreq/ff400000.gpu/cur_freq",
        "/sys/class/devfreq/ff400000.gpu/cur_freq",
    ] {
        if let Ok(s) = std::fs::read_to_string(c) {
            if let Ok(v) = s.trim().parse::<u32>() {
                return Some(v / 1_000_000);
            }
        }
    }
    None
}

pub fn read_governor() -> Option<String> {
    let f = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";
    std::fs::read_to_string(f)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn read_panel_compat() -> Option<String> {
    let f = "/proc/device-tree/dsi@ff450000/panel@0/compatible";
    let raw = std::fs::read(f).ok()?;
    // device-tree strings are NUL-separated, first one is the panel ID
    let first = raw.split(|&b| b == 0).next()?;
    Some(String::from_utf8_lossy(first).into_owned())
}

pub fn read_panel_resolution() -> Option<(u32, u32)> {
    // panel timing 0 — uses BE-32 ints
    let base = Path::new("/proc/device-tree/dsi@ff450000/panel@0/display-timings/timing0");
    let h = std::fs::read(base.join("hactive")).ok()?;
    let v = std::fs::read(base.join("vactive")).ok()?;
    let p = |b: Vec<u8>| -> Option<u32> {
        if b.len() < 4 {
            return None;
        }
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    Some((p(h)?, p(v)?))
}

pub fn read_firmware_string() -> Option<String> {
    for f in [
        "/etc/os-release",
        "/usr/share/plymouth/themes/text.plymouth",
    ] {
        if let Ok(s) = std::fs::read_to_string(f) {
            for line in s.lines() {
                if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
                    return Some(v.trim_matches('"').to_string());
                }
                if let Some(v) = line.strip_prefix("title=") {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

pub fn snapshot() -> Result<HardwareSnapshot> {
    let mut s = sysinfo::System::new_all();
    s.refresh_all();
    let cpu = s.cpus().first();
    let snap = HardwareSnapshot {
        timestamp: chrono::Utc::now(),
        soc: "RK3326".to_string(),
        cpu_model: cpu
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "unknown".into()),
        cpu_cores: s.cpus().len() as u32,
        mem_total_mb: s.total_memory() / 1024 / 1024,
        mem_free_mb: s.free_memory() / 1024 / 1024,
        kernel: sysinfo::System::kernel_version().unwrap_or_default(),
        uptime_s: sysinfo::System::uptime(),
        temp_c: read_temp_c(),
        cpu_freq_mhz: read_cpu_freq_mhz(),
        gpu_freq_mhz: read_gpu_freq_mhz(),
        gov: read_governor(),
        panel_compat: read_panel_compat(),
        panel_res: read_panel_resolution(),
        dtb_path: hardware_string().ok(),
        firmware: read_firmware_string(),
    };
    Ok(snap)
}
