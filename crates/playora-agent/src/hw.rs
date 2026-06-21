use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use sysinfo::System;

pub fn snapshot() -> HardwareSnapshot {
    let mut s = System::new_all();
    s.refresh_all();
    HardwareSnapshot {
        cpu_model: s
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default(),
        cpu_arch: std::env::consts::ARCH.to_string(),
        cpu_cores: s.cpus().len() as u32,
        mem_total_mb: s.total_memory() / 1024 / 1024,
        mem_available_mb: s.available_memory() / 1024 / 1024,
        swap_total_mb: s.total_swap() / 1024 / 1024,
        kernel: System::kernel_version().unwrap_or_default(),
        uptime_s: System::uptime(),
        load_avg: read_loadavg(),
        temps_c: read_temps(),
        freqs_mhz: read_freqs(),
        governors: read_govs(),
        disks: read_disks(),
        batteries: read_batteries(),
        net_ifs: read_net_ifs(),
        panel_compatible: read_panel_compat(),
        panel_resolution: read_panel_res(),
        framebuffer: read_fb(),
        audio_cards: read_audio_cards(),
        input_devices: read_input(),
        usb_devices: read_usb(),
        retroarch_detected: detect_retroarch(),
        retroarch_version: retroarch_version(),
        hardware_string: read_hardware_string(),
        captured_at: Utc::now(),
    }
}

fn read_loadavg() -> Option<(f32, f32, f32)> {
    let s = std::fs::read_to_string("/proc/loadavg").ok()?;
    let parts: Vec<&str> = s.split_whitespace().collect();
    Some((
        parts.first()?.parse().ok()?,
        parts.get(1)?.parse().ok()?,
        parts.get(2)?.parse().ok()?,
    ))
}

fn read_temps() -> BTreeMap<String, f32> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/thermal") {
        for e in rd.flatten() {
            let zone = e.path();
            let name = zone
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            if !name.starts_with("thermal_zone") {
                continue;
            }
            let label = std::fs::read_to_string(zone.join("type"))
                .ok()
                .map(|x| x.trim().to_string())
                .unwrap_or(name.clone());
            if let Ok(t) = std::fs::read_to_string(zone.join("temp")) {
                if let Ok(v) = t.trim().parse::<f32>() {
                    out.insert(label, v / 1000.0);
                }
            }
        }
    }
    out
}

fn read_freqs() -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir("/sys/devices/system/cpu") {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("cpu")
                || !name
                    .chars()
                    .nth(3)
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
            {
                continue;
            }
            let f = p.join("cpufreq/scaling_cur_freq");
            if let Ok(s) = std::fs::read_to_string(&f) {
                if let Ok(v) = s.trim().parse::<u32>() {
                    out.insert(name, v / 1000);
                }
            }
        }
    }
    out
}

fn read_govs() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir("/sys/devices/system/cpu") {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("cpu")
                || !name
                    .chars()
                    .nth(3)
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
            {
                continue;
            }
            if let Ok(s) = std::fs::read_to_string(p.join("cpufreq/scaling_governor")) {
                out.insert(name, s.trim().to_string());
            }
        }
    }
    out
}

fn read_disks() -> Vec<DiskInfo> {
    let mut out = vec![];
    let txt = match std::fs::read_to_string("/proc/mounts") {
        Ok(s) => s,
        Err(_) => return out,
    };
    for line in txt.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let mount = parts[1];
        let fstype = parts[2];
        // skip pseudo
        if matches!(
            fstype,
            "proc"
                | "sysfs"
                | "cgroup"
                | "cgroup2"
                | "devpts"
                | "tmpfs"
                | "devtmpfs"
                | "squashfs"
                | "overlay"
                | "autofs"
                | "nsfs"
                | "pstore"
                | "bpf"
                | "securityfs"
                | "debugfs"
                | "tracefs"
                | "mqueue"
                | "hugetlbfs"
                | "configfs"
                | "fusectl"
                | "fuse.gvfsd-fuse"
                | "rpc_pipefs"
        ) {
            continue;
        }
        if let Some(d) = statvfs(mount) {
            out.push(DiskInfo {
                mount: mount.into(),
                fstype: fstype.into(),
                total_bytes: d.0,
                free_bytes: d.1,
                used_bytes: d.0.saturating_sub(d.1),
            });
        }
    }
    out
}

fn read_batteries() -> Vec<BatteryInfo> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let ty = std::fs::read_to_string(p.join("type"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if ty != "Battery" {
                continue;
            }
            out.push(BatteryInfo {
                name,
                status: std::fs::read_to_string(p.join("status"))
                    .ok()
                    .map(|s| s.trim().to_string()),
                capacity_pct: std::fs::read_to_string(p.join("capacity"))
                    .ok()
                    .and_then(|s| s.trim().parse().ok()),
                voltage_uv: std::fs::read_to_string(p.join("voltage_now"))
                    .ok()
                    .and_then(|s| s.trim().parse().ok()),
                current_ua: std::fs::read_to_string(p.join("current_now"))
                    .ok()
                    .and_then(|s| s.trim().parse().ok()),
            });
        }
    }
    out
}

fn read_net_ifs() -> Vec<NetIfInfo> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir("/sys/class/net") {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "lo" {
                continue;
            }
            let up = std::fs::read_to_string(p.join("operstate"))
                .map(|s| s.trim() == "up")
                .unwrap_or(false);
            let mac_raw = std::fs::read_to_string(p.join("address"))
                .ok()
                .map(|s| s.trim().to_string());
            let mac_hash = mac_raw
                .as_ref()
                .map(|m| hex::encode(Sha256::digest(m.as_bytes())));
            let is_wireless = p.join("wireless").exists() || p.join("phy80211").exists();
            out.push(NetIfInfo {
                name,
                up,
                ipv4: None, // could parse via getifaddrs but keep MVP small
                mac_hash,
                is_wireless,
            });
        }
    }
    out
}

fn read_panel_compat() -> Option<String> {
    let raw = std::fs::read("/proc/device-tree/dsi@ff450000/panel@0/compatible").ok()?;
    Some(String::from_utf8_lossy(raw.split(|&b| b == 0).next()?).into_owned())
}
fn read_panel_res() -> Option<(u32, u32)> {
    let base = Path::new("/proc/device-tree/dsi@ff450000/panel@0/display-timings/timing0");
    let r = |n: &str| -> Option<u32> {
        let b = std::fs::read(base.join(n)).ok()?;
        if b.len() < 4 {
            return None;
        }
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    Some((r("hactive")?, r("vactive")?))
}
fn read_fb() -> Option<String> {
    std::fs::read_to_string("/sys/class/graphics/fb0/name")
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_audio_cards() -> Vec<String> {
    std::fs::read_to_string("/proc/asound/cards")
        .map(|t| t.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}
fn read_input() -> Vec<String> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir("/dev/input") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("event") {
                v.push(format!("/dev/input/{n}"));
            }
        }
    }
    v
}
fn read_usb() -> Vec<UsbDevice> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir("/sys/bus/usb/devices") {
        for e in rd.flatten() {
            let p = e.path();
            let v = std::fs::read_to_string(p.join("idVendor"))
                .ok()
                .map(|s| s.trim().to_string());
            let pi = std::fs::read_to_string(p.join("idProduct"))
                .ok()
                .map(|s| s.trim().to_string());
            if v.is_some() && pi.is_some() {
                let product = std::fs::read_to_string(p.join("product"))
                    .ok()
                    .map(|s| s.trim().to_string());
                out.push(UsbDevice {
                    vendor_id: v,
                    product_id: pi,
                    product,
                });
            }
        }
    }
    out
}
fn detect_retroarch() -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg("command -v retroarch >/dev/null 2>&1")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
fn retroarch_version() -> Option<String> {
    let out = std::process::Command::new("retroarch")
        .arg("--version")
        .output()
        .ok()?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
    )
}
fn read_hardware_string() -> Option<String> {
    let s = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    for line in s.lines() {
        if let Some(r) = line.strip_prefix("Hardware") {
            return Some(r.split(':').nth(1)?.trim().to_string());
        }
    }
    None
}

// statvfs returns (total_bytes, free_bytes)
fn statvfs(path: &str) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::raw::c_char;
    #[repr(C)]
    #[derive(Default)]
    struct S {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: u64,
        f_flag: u64,
        f_namemax: u64,
        f_pad: [u32; 32],
    }
    extern "C" {
        fn statvfs(p: *const c_char, b: *mut S) -> i32;
    }
    let c = CString::new(path).ok()?;
    let mut s = S::default();
    let rc = unsafe { statvfs(c.as_ptr(), &mut s) };
    if rc != 0 {
        return None;
    }
    Some((s.f_blocks * s.f_frsize, s.f_bavail * s.f_frsize))
}

pub fn cmd_snapshot(cfg: AgentConfig, save: bool, pretty: bool) -> Result<()> {
    let snap = snapshot();
    if pretty {
        print_pretty(&snap);
    } else {
        println!("{}", serde_json::to_string_pretty(&snap)?);
    }
    if save {
        let conn = crate::db::open(&crate::cfg::db_path())?;
        let ev = Event {
            event_id: EventId::new(),
            device_id: cfg.device_id,
            created_at: Utc::now(),
            payload: EventPayload::HardwareSnapshot(snap),
        };
        crate::db::enqueue(&conn, &ev)?;
        if !pretty {
            eprintln!("queued event {}", ev.event_id);
        }
    }
    Ok(())
}

fn print_pretty(s: &HardwareSnapshot) {
    use crate::ttyui::{self, Status};
    ttyui::header("Hardware");
    ttyui::section("CPU + Memory");
    ttyui::row(
        "cpu",
        &format!("{} ({}, {} cores)", s.cpu_model, s.cpu_arch, s.cpu_cores),
        Status::Info,
    );
    ttyui::row(
        "memory",
        &format!(
            "{} MB total / {} MB free",
            s.mem_total_mb, s.mem_available_mb
        ),
        Status::Info,
    );
    if !s.temps_c.is_empty() {
        ttyui::row("temperatures", &format!("{:?} °C", s.temps_c), Status::Info);
    }

    ttyui::section("OS + Panel");
    ttyui::row("kernel", &s.kernel, Status::Info);
    ttyui::row(
        "hardware string",
        s.hardware_string.as_deref().unwrap_or("?"),
        Status::Info,
    );
    ttyui::row(
        "panel",
        s.panel_compatible.as_deref().unwrap_or("?"),
        Status::Info,
    );
    ttyui::row(
        "retroarch",
        if s.retroarch_detected {
            "detected"
        } else {
            "absent"
        },
        if s.retroarch_detected {
            Status::Ok
        } else {
            Status::Warn
        },
    );

    ttyui::section("Storage");
    for d in &s.disks {
        ttyui::row(
            &d.mount,
            &format!(
                "{} GB free / {} GB total",
                d.free_bytes / 1024 / 1024 / 1024,
                d.total_bytes / 1024 / 1024 / 1024
            ),
            if d.free_bytes > 1024 * 1024 * 1024 {
                Status::Ok
            } else {
                Status::Warn
            },
        );
    }

    ttyui::section("Network");
    for n in &s.net_ifs {
        let label = if n.is_wireless { "wifi" } else { "wired" };
        ttyui::row(
            &format!("{} {}", label, n.name),
            &format!(
                "{} {}",
                if n.up { "up" } else { "down" },
                n.ipv4.as_deref().unwrap_or("(no ip)")
            ),
            if n.up { Status::Ok } else { Status::Warn },
        );
    }

    println!();
    println!("  Snapshot also synced to the dashboard.");
}

pub fn cmd_watch(interval: u64) -> Result<()> {
    loop {
        let s = snapshot();
        println!(
            "[{}] cpu={} mem={}/{}MB temps={:?}",
            s.captured_at,
            s.cpu_cores,
            s.mem_total_mb - s.mem_available_mb,
            s.mem_total_mb,
            s.temps_c
        );
        std::thread::sleep(std::time::Duration::from_secs(interval));
    }
}
