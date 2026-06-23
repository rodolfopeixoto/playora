//! Doctor `--deep` — comprehensive health check for dArkOSRE/Playora.
//!
//! Produces a structured JSON report in `/roms/.playora/reports/`, a tail-able
//! log at `/roms/.playora/logs/doctor-latest.log`, and emits a `DoctorReport`
//! event for the dashboard. Each check yields a CheckResult. The aggregated
//! score is OK / WARN / FAIL. Auto-fixable issues are listed separately from
//! manual ones so the UI can drive remediation.

use anyhow::Result;
use chrono::Utc;
use playora_common::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Severity {
    Ok,
    Warn,
    Fail,
    Info,
}

#[derive(Debug, Clone, Serialize)]
struct CheckResult {
    key: String,
    title: String,
    severity: Severity,
    detail: String,
    evidence: Option<String>,
    fix_code: Option<String>,
    auto_fixable: bool,
    suggested_fix: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeepReport {
    report_id: String,
    captured_at: chrono::DateTime<Utc>,
    device_id: String,
    device_name: String,
    agent_version: String,
    checks: Vec<CheckResult>,
    summary: Summary,
}

#[derive(Debug, Serialize)]
struct Summary {
    score: DoctorScore,
    total: u32,
    ok: u32,
    warn: u32,
    fail: u32,
    auto_fixes: Vec<String>,
    manual_fixes: Vec<String>,
}

pub fn cmd_doctor_deep(cfg: AgentConfig, json_only: bool) -> Result<()> {
    use crate::ttyui::{self, Status};

    let report_id = format!("doctor_{}", Uuid::new_v4().simple());
    let mut checks: Vec<CheckResult> = Vec::new();

    if !json_only {
        ttyui::header("Doctor — Deep Health Check");
        ttyui::section("Identity");
        ttyui::row("device_id", &cfg.device_id.0, Status::Info);
        ttyui::row("device_name", &cfg.device_name, Status::Info);
        ttyui::row("agent_version", env!("CARGO_PKG_VERSION"), Status::Info);
    }

    // --- 1. Hardware/system identity
    let snap = crate::hw::snapshot();
    push_info(&mut checks, "kernel", "Kernel", &snap.kernel);
    push_info(
        &mut checks,
        "cpu",
        "CPU",
        &format!("{} ({}c)", snap.cpu_model, snap.cpu_cores),
    );
    push_info(&mut checks, "arch", "Architecture", &snap.cpu_arch);
    push_info(
        &mut checks,
        "hw_string",
        "Hardware string",
        snap.hardware_string.as_deref().unwrap_or("?"),
    );
    push_info(
        &mut checks,
        "panel",
        "Panel",
        snap.panel_compatible.as_deref().unwrap_or("?"),
    );
    if let Some((w, h)) = snap.panel_resolution {
        push_info(
            &mut checks,
            "panel_res",
            "Panel resolution",
            &format!("{w}x{h}"),
        );
    }
    push_info(
        &mut checks,
        "framebuffer",
        "Framebuffer",
        snap.framebuffer.as_deref().unwrap_or("?"),
    );

    // --- 2. Device profile
    let profile = format!("{:?}", cfg.device_profile);
    push_info(&mut checks, "device_profile", "Device profile", &profile);

    // --- 3. TTY available
    let tty_ok = Path::new("/dev/tty1").exists() || Path::new("/dev/tty0").exists();
    push(
        &mut checks,
        "tty",
        "TTY framebuffer console",
        if tty_ok { Severity::Ok } else { Severity::Fail },
        if tty_ok {
            "tty1/tty0 present"
        } else {
            "no /dev/tty1 or /dev/tty0"
        }
        .into(),
        None,
        false,
        if tty_ok {
            None
        } else {
            Some("Reboot device; check that getty/console is enabled.".into())
        },
        "tty_missing",
    );

    // --- 4. /roms mount + free space + fstype
    let roms_w = is_writeable("/roms");
    let free_mb = snap
        .disks
        .iter()
        .find(|d| d.mount == "/roms")
        .map(|d| d.free_bytes / 1024 / 1024)
        .unwrap_or(0);
    let fstype = snap
        .disks
        .iter()
        .find(|d| d.mount == "/roms")
        .map(|d| d.fstype.clone())
        .unwrap_or_else(|| "unknown".into());
    push(
        &mut checks,
        "roms_writable",
        "/roms writeable",
        if roms_w { Severity::Ok } else { Severity::Fail },
        if roms_w {
            "OK".into()
        } else {
            "READ-ONLY (SD corrupt or remounted ro)".into()
        },
        None,
        false,
        if roms_w {
            None
        } else {
            Some(
                "Power-off cleanly. Check dmesg for mmc/I/O errors. Re-flash SD if persistent."
                    .into(),
            )
        },
        "roms_readonly",
    );
    push(
        &mut checks,
        "roms_free",
        "/roms free space",
        if free_mb >= 1024 {
            Severity::Ok
        } else if free_mb > 256 {
            Severity::Warn
        } else {
            Severity::Fail
        },
        format!("{free_mb} MB free ({fstype})"),
        None,
        false,
        if free_mb < 1024 {
            Some("Run Cleanup, remove duplicates, or use Compress ROMs to free space.".into())
        } else {
            None
        },
        "roms_low_space",
    );

    // --- 5. dmesg storage errors (last 200 lines, best-effort)
    let dmesg_issues = scan_dmesg();
    push(
        &mut checks,
        "dmesg_storage",
        "dmesg storage errors",
        if dmesg_issues.is_empty() {
            Severity::Ok
        } else {
            Severity::Fail
        },
        if dmesg_issues.is_empty() {
            "no recent mmc/ext4/I/O errors".into()
        } else {
            format!(
                "{} suspect line(s) in last dmesg window",
                dmesg_issues.len()
            )
        },
        Some(dmesg_issues.join("\n")).filter(|s| !s.is_empty()),
        false,
        if dmesg_issues.is_empty() {
            None
        } else {
            Some("Back up /roms now; SD may be failing.".into())
        },
        "sd_failing",
    );

    // --- 6. macOS junk files (from TAR transfer)
    let macos_junk = count_macos_junk("/roms");
    push(
        &mut checks,
        "macos_junk",
        "macOS junk files in /roms",
        if macos_junk == 0 {
            Severity::Ok
        } else if macos_junk < 50 {
            Severity::Warn
        } else {
            Severity::Fail
        },
        format!("{macos_junk} junk file(s)"),
        None,
        macos_junk > 0,
        if macos_junk > 0 {
            Some(
                "Run `playora-agent clean-roms --apply` to remove .DS_Store / ._* / __MACOSX/."
                    .into(),
            )
        } else {
            None
        },
        "macos_junk",
    );

    // --- 7. RetroArch + RetroArch32 install + cfg discovery
    let ra_cfgs = find_all_retroarch_cfgs();
    let ra_detected = snap.retroarch_detected;
    push(
        &mut checks,
        "retroarch_present",
        "RetroArch binary",
        if ra_detected {
            Severity::Ok
        } else {
            Severity::Fail
        },
        if ra_detected {
            "detected".into()
        } else {
            "not found".into()
        },
        snap.retroarch_version.clone(),
        false,
        if ra_detected {
            None
        } else {
            Some("Reinstall dArkOSRE base packages.".into())
        },
        "retroarch_missing",
    );
    push(
        &mut checks,
        "retroarch_cfg",
        "retroarch.cfg paths",
        if ra_cfgs.is_empty() {
            Severity::Fail
        } else {
            Severity::Ok
        },
        format!("{} cfg path(s) found", ra_cfgs.len()),
        Some(
            ra_cfgs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .filter(|s| !s.is_empty()),
        false,
        if ra_cfgs.is_empty() {
            Some("Launch RetroArch once so it writes a default cfg.".into())
        } else {
            None
        },
        "retroarch_cfg_missing",
    );

    // --- 8. Hotkey + exit-game settings audit (across all found cfgs)
    let mut hotkey_warns: Vec<String> = Vec::new();
    for cfg_path in &ra_cfgs {
        if let Ok(content) = std::fs::read_to_string(cfg_path) {
            let map = crate::fix_exit::parse_settings(&content);
            for (k, want) in EXIT_SAFE_KEYS {
                match map.get(*k) {
                    Some(v) if v == want => {}
                    Some(v) => {
                        hotkey_warns.push(format!("{}: {k}={v} (want {want})", cfg_path.display()))
                    }
                    None => hotkey_warns
                        .push(format!("{}: {k} unset (want {want})", cfg_path.display())),
                }
            }
        }
    }
    push(
        &mut checks,
        "exit_game_cfg",
        "RetroArch exit-game / hotkey config",
        if hotkey_warns.is_empty() {
            Severity::Ok
        } else {
            Severity::Warn
        },
        if hotkey_warns.is_empty() {
            "exit-safe keys aligned".into()
        } else {
            format!("{} key(s) need attention", hotkey_warns.len())
        },
        Some(hotkey_warns.join("\n")).filter(|s| !s.is_empty()),
        !hotkey_warns.is_empty(),
        if hotkey_warns.is_empty() {
            None
        } else {
            Some("Run `playora-agent fix-exit-game --dry-run` then `--apply`.".into())
        },
        "exit_game_misconfigured",
    );

    // --- 9. RetroArch overrides per-core / per-game
    let overrides = find_retroarch_overrides();
    push(
        &mut checks,
        "retroarch_overrides",
        "RetroArch overrides (core/game)",
        if overrides.is_empty() {
            Severity::Ok
        } else {
            Severity::Warn
        },
        format!("{} override file(s)", overrides.len()),
        Some(
            overrides
                .iter()
                .take(20)
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .filter(|s| !s.is_empty()),
        false,
        if overrides.is_empty() {
            None
        } else {
            Some("Per-core/per-game overrides can mask global fixes; audit each.".into())
        },
        "retroarch_overrides_present",
    );

    // --- 10. Required tools
    for (tool, hint) in REQUIRED_TOOLS {
        let found = which_any(tool).is_some();
        push(
            &mut checks,
            &format!("tool_{tool}"),
            &format!("tool: {tool}"),
            if found { Severity::Ok } else { Severity::Warn },
            if found {
                "found".into()
            } else {
                "missing".into()
            },
            None,
            false,
            if found { None } else { Some((*hint).into()) },
            "tool_missing",
        );
    }

    // --- 11. gptokeyb processes (zombies after game quit)
    let gptokeyb_alive = process_alive("gptokeyb") || process_alive("gptokeyb2");
    push(
        &mut checks,
        "gptokeyb_zombie",
        "gptokeyb running outside a port",
        if gptokeyb_alive {
            Severity::Warn
        } else {
            Severity::Ok
        },
        if gptokeyb_alive {
            "instance alive — possible input capture".into()
        } else {
            "no rogue gptokeyb".into()
        },
        None,
        gptokeyb_alive,
        if gptokeyb_alive {
            Some("`playora-agent recover` will kill it.".into())
        } else {
            None
        },
        "gptokeyb_zombie",
    );

    // --- 12. EmulationStation service detected
    let es = detect_es_service();
    push(
        &mut checks,
        "es_service",
        "EmulationStation service",
        match &es {
            Some(_) => Severity::Ok,
            None => Severity::Warn,
        },
        es.clone()
            .unwrap_or_else(|| "no systemd unit detected — fallback exec on recover".into()),
        None,
        false,
        if es.is_none() {
            Some("Recover will spawn ES via exec fallback.".into())
        } else {
            None
        },
        "es_service_missing",
    );

    // --- 13. Gamelist validity (count of malformed xml)
    let bad_gamelists = scan_gamelists("/roms");
    push(
        &mut checks,
        "gamelists",
        "gamelist.xml integrity",
        if bad_gamelists.is_empty() {
            Severity::Ok
        } else {
            Severity::Warn
        },
        format!("{} invalid gamelist(s)", bad_gamelists.len()),
        Some(bad_gamelists.join("\n")).filter(|s| !s.is_empty()),
        false,
        if bad_gamelists.is_empty() {
            None
        } else {
            Some("Open each in a text editor or let Scan ROMs regenerate them.".into())
        },
        "gamelist_invalid",
    );

    // --- 14. BIOS folder presence
    let bios_dir = Path::new("/roms/bios");
    let bios_count = bios_dir
        .read_dir()
        .ok()
        .map(|it| it.filter_map(|e| e.ok()).count() as u32)
        .unwrap_or(0);
    push(
        &mut checks,
        "bios_present",
        "/roms/bios folder",
        if bios_dir.is_dir() {
            Severity::Ok
        } else {
            Severity::Warn
        },
        if bios_dir.is_dir() {
            format!("{bios_count} file(s)")
        } else {
            "missing".into()
        },
        None,
        false,
        if bios_dir.is_dir() {
            None
        } else {
            Some("Create /roms/bios and drop required BIOS there for PSX/PSP/etc.".into())
        },
        "bios_missing",
    );

    // --- 15. Broken CUE references (cue points to .bin that does not exist)
    let bad_cue = scan_broken_cues("/roms");
    push(
        &mut checks,
        "cue_integrity",
        "CUE/BIN integrity",
        if bad_cue.is_empty() {
            Severity::Ok
        } else {
            Severity::Warn
        },
        format!("{} broken cue(s)", bad_cue.len()),
        Some(
            bad_cue
                .iter()
                .take(20)
                .map(|s| s.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .filter(|s| !s.is_empty()),
        false,
        if bad_cue.is_empty() {
            None
        } else {
            Some("Re-copy the matching .bin or rename inside the .cue.".into())
        },
        "cue_broken",
    );

    // --- 16. Broken M3U references
    let bad_m3u = scan_broken_m3us("/roms");
    push(
        &mut checks,
        "m3u_integrity",
        "M3U integrity",
        if bad_m3u.is_empty() {
            Severity::Ok
        } else {
            Severity::Warn
        },
        format!("{} broken m3u(s)", bad_m3u.len()),
        Some(
            bad_m3u
                .iter()
                .take(20)
                .map(|s| s.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .filter(|s| !s.is_empty()),
        false,
        if bad_m3u.is_empty() {
            None
        } else {
            Some("Open each .m3u — paths must be relative + match real filenames.".into())
        },
        "m3u_broken",
    );

    // --- 17. Local SQLite + pending events
    let db_path = crate::cfg::db_path();
    let db = crate::db::open(&db_path);
    let pending = db
        .as_ref()
        .ok()
        .and_then(|c| crate::db::count_pending(c).ok())
        .unwrap_or(0);
    push(
        &mut checks,
        "agent_db",
        "Agent SQLite DB",
        if db.is_ok() {
            Severity::Ok
        } else {
            Severity::Fail
        },
        format!("{} ({} pending event(s))", db_path.display(), pending),
        None,
        false,
        if db.is_err() {
            Some("Re-run `playora-agent init` or delete the DB and re-init.".into())
        } else {
            None
        },
        "agent_db_broken",
    );

    // --- 18. Server reachable
    let server_ok = ping_server(&cfg.server_url);
    push(
        &mut checks,
        "server_reachable",
        "Dashboard reachable",
        if server_ok {
            Severity::Ok
        } else {
            Severity::Warn
        },
        format!(
            "{} -> {}",
            cfg.server_url,
            if server_ok { "ok" } else { "offline" }
        ),
        None,
        false,
        if server_ok {
            None
        } else {
            Some("Offline-first: events queue locally until reachable.".into())
        },
        "server_offline",
    );

    // --- 19. Autosync service status
    let autosync_status = autosync_status();
    push(
        &mut checks,
        "autosync_service",
        "Autosync systemd service",
        match autosync_status.as_str() {
            "active" => Severity::Ok,
            "inactive" => Severity::Warn,
            _ => Severity::Info,
        },
        autosync_status.clone(),
        None,
        autosync_status == "inactive",
        if autosync_status == "inactive" {
            Some("Run port `Playora Autosync Enable`.".into())
        } else {
            None
        },
        "autosync_inactive",
    );

    // --- 20. Render TTY summary
    let mut totals = (0u32, 0u32, 0u32, 0u32); // ok warn fail info
    if !json_only {
        let by_section = group_by_section(&checks);
        for (section, rows) in &by_section {
            ttyui::section(section);
            for r in rows {
                let s = match r.severity {
                    Severity::Ok => Status::Ok,
                    Severity::Warn => Status::Warn,
                    Severity::Fail => Status::Fail,
                    Severity::Info => Status::Info,
                };
                ttyui::row(&r.title, &r.detail, s);
                if let Some(fix) = &r.suggested_fix {
                    ttyui::note(fix);
                }
            }
        }
    }
    for r in &checks {
        match r.severity {
            Severity::Ok => totals.0 += 1,
            Severity::Warn => totals.1 += 1,
            Severity::Fail => totals.2 += 1,
            Severity::Info => totals.3 += 1,
        }
    }
    let score = if totals.2 > 0 {
        DoctorScore::Fail
    } else if totals.1 > 0 {
        DoctorScore::Warn
    } else {
        DoctorScore::Ok
    };
    let auto_fixes: Vec<String> = checks
        .iter()
        .filter(|c| c.auto_fixable)
        .map(|c| {
            format!(
                "{}: {}",
                c.title,
                c.suggested_fix.clone().unwrap_or_default()
            )
        })
        .collect();
    let manual_fixes: Vec<String> = checks
        .iter()
        .filter(|c| !c.auto_fixable && c.severity != Severity::Ok && c.severity != Severity::Info)
        .filter_map(|c| c.suggested_fix.clone().map(|f| format!("{}: {f}", c.title)))
        .collect();

    // --- 21. Persist JSON report + log + emit event
    let captured_at = Utc::now();
    let stamp = captured_at.format("%Y%m%d-%H%M%S").to_string();
    let reports_dir = PathBuf::from("/roms/.playora/reports");
    let logs_dir = PathBuf::from("/roms/.playora/logs");
    let _ = std::fs::create_dir_all(&reports_dir);
    let _ = std::fs::create_dir_all(&logs_dir);
    let report_path = reports_dir.join(format!("doctor-{stamp}.json"));
    let log_latest = logs_dir.join("doctor-latest.log");

    let report = DeepReport {
        report_id: report_id.clone(),
        captured_at,
        device_id: cfg.device_id.0.clone(),
        device_name: cfg.device_name.clone(),
        agent_version: env!("CARGO_PKG_VERSION").into(),
        checks: checks.clone(),
        summary: Summary {
            score,
            total: checks.len() as u32,
            ok: totals.0,
            warn: totals.1,
            fail: totals.2,
            auto_fixes: auto_fixes.clone(),
            manual_fixes: manual_fixes.clone(),
        },
    };
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        let _ = atomic_write(&report_path, json.as_bytes());
    }
    let mut tail = format!(
        "doctor {stamp} — score={:?} ok={} warn={} fail={}\nreport: {}\n",
        score,
        totals.0,
        totals.1,
        totals.2,
        report_path.display()
    );
    for c in &checks {
        if c.severity == Severity::Warn || c.severity == Severity::Fail {
            tail.push_str(&format!("[{:?}] {} — {}\n", c.severity, c.title, c.detail));
        }
    }
    let _ = atomic_write(&log_latest, tail.as_bytes());

    // Emit events: SystemIssueDetected per warn/fail + DoctorReport summary
    if let Ok(conn) = db {
        for c in &checks {
            let sev = match c.severity {
                Severity::Fail => IssueSeverity::Critical,
                Severity::Warn => IssueSeverity::Warning,
                _ => continue,
            };
            let issue = SystemIssueDetected {
                code: c.fix_code.clone().unwrap_or_else(|| c.key.clone()),
                severity: sev,
                title: c.title.clone(),
                evidence: c.evidence.clone(),
                suggested_fix: c.suggested_fix.clone(),
                auto_fixable: c.auto_fixable,
                detected_at: captured_at,
            };
            let _ = crate::db::enqueue(
                &conn,
                &Event {
                    event_id: EventId::new(),
                    device_id: cfg.device_id.clone(),
                    created_at: captured_at,
                    payload: EventPayload::SystemIssueDetected(issue),
                },
            );
        }
        let dr = DoctorReport {
            report_id: report_id.clone(),
            score,
            checks_total: checks.len() as u32,
            checks_ok: totals.0,
            checks_warn: totals.1,
            checks_fail: totals.2,
            issues: vec![],
            auto_fixes,
            manual_fixes,
            report_path: Some(report_path.display().to_string()),
            captured_at,
        };
        let _ = crate::db::enqueue(
            &conn,
            &Event {
                event_id: EventId::new(),
                device_id: cfg.device_id.clone(),
                created_at: captured_at,
                payload: EventPayload::DoctorReport(dr),
            },
        );
    }

    if json_only {
        let body = std::fs::read_to_string(&report_path).unwrap_or_default();
        println!("{body}");
    } else {
        println!();
        println!(
            "  Doctor complete — score={:?} (ok={} warn={} fail={}).",
            score, totals.0, totals.1, totals.2
        );
        println!("  report: {}", report_path.display());
    }
    Ok(())
}

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

const EXIT_SAFE_KEYS: &[(&str, &str)] = &[
    ("video_threaded", "false"),
    ("pause_nonactive", "false"),
    ("quit_press_twice", "true"),
    ("input_quit_gamepad_combo", "4"),
    ("video_fullscreen", "true"),
    ("video_disable_composition", "true"),
    ("input_keyboard_gamepad_enable", "true"),
];

const REQUIRED_TOOLS: &[(&str, &str)] = &[
    ("retroarch", "Reinstall dArkOSRE base packages."),
    ("systemctl", "Needed for autosync + ES restart."),
    ("pgrep", "Used by recover + session tracker."),
    ("timeout", "Coreutils — used by port-runner."),
    ("find", "Coreutils — used by scanners."),
    (
        "dialog",
        "Optional: needed for the port-runner review menu.",
    ),
    ("qrencode", "Optional: Cloud Setup QR code."),
    ("fbv", "Optional: framebuffer image viewer."),
    ("chdman", "Optional: Compress ROMs (PSX/Saturn)."),
    (
        "rclone",
        "Cloud Backup/Restore — bundled in /roms/.playora/bin/.",
    ),
];

fn push(
    out: &mut Vec<CheckResult>,
    key: &str,
    title: &str,
    sev: Severity,
    detail: String,
    evidence: Option<String>,
    auto_fixable: bool,
    suggested_fix: Option<String>,
    fix_code: &str,
) {
    out.push(CheckResult {
        key: key.into(),
        title: title.into(),
        severity: sev,
        detail,
        evidence,
        fix_code: Some(fix_code.into()),
        auto_fixable,
        suggested_fix,
    });
}

fn push_info(out: &mut Vec<CheckResult>, key: &str, title: &str, val: &str) {
    push(
        out,
        key,
        title,
        Severity::Info,
        val.into(),
        None,
        false,
        None,
        "info",
    );
}

fn group_by_section(checks: &[CheckResult]) -> BTreeMap<&'static str, Vec<&CheckResult>> {
    let mut map: BTreeMap<&'static str, Vec<&CheckResult>> = BTreeMap::new();
    for c in checks {
        let s = match c.key.as_str() {
            "kernel" | "cpu" | "arch" | "hw_string" | "panel" | "panel_res" | "framebuffer"
            | "device_profile" => "1. Identity",
            "tty" | "es_service" => "2. Console",
            "roms_writable" | "roms_free" | "dmesg_storage" | "macos_junk" | "bios_present" => {
                "3. Storage"
            }
            "retroarch_present" | "retroarch_cfg" | "exit_game_cfg" | "retroarch_overrides" => {
                "4. RetroArch"
            }
            "gamelists" | "cue_integrity" | "m3u_integrity" => "5. ROM Layout",
            "gptokeyb_zombie" => "6. Runtime",
            "agent_db" | "server_reachable" | "autosync_service" => "7. Playora",
            _ if c.key.starts_with("tool_") => "8. Tools",
            _ => "9. Other",
        };
        map.entry(s).or_default().push(c);
    }
    map
}

fn is_writeable(path: &str) -> bool {
    let test = format!("{path}/.playora-doctor-write-test");
    let r = std::fs::write(&test, b"x").is_ok();
    let _ = std::fs::remove_file(&test);
    r
}

fn which_any(tool: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let p = Path::new(dir).join(tool);
        if p.is_file() {
            return Some(p);
        }
    }
    let bundled = Path::new("/roms/.playora/bin").join(tool);
    if bundled.is_file() {
        return Some(bundled);
    }
    None
}

fn process_alive(name: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ping_server(url: &str) -> bool {
    if url == "auto" || url.is_empty() {
        return false;
    }
    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    else {
        return false;
    };
    let u = format!("{}/health", url.trim_end_matches('/'));
    client
        .get(&u)
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

fn detect_es_service() -> Option<String> {
    let out = Command::new("systemctl")
        .args(["list-unit-files", "--no-legend"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for unit in ["emulationstation", "emustation", "oga_es"] {
        if text
            .lines()
            .any(|l| l.starts_with(&format!("{unit}.service")))
        {
            return Some(format!("{unit}.service"));
        }
    }
    None
}

fn autosync_status() -> String {
    Command::new("systemctl")
        .args(["is-active", "playora-agent.service"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn scan_dmesg() -> Vec<String> {
    let out = Command::new("dmesg").arg("-T").output();
    let Ok(o) = out else { return vec![] };
    let text = String::from_utf8_lossy(&o.stdout);
    let mut suspects = Vec::new();
    for l in text.lines().rev().take(500) {
        let lc = l.to_lowercase();
        if lc.contains("i/o error")
            || lc.contains("mmc") && (lc.contains("error") || lc.contains("timeout"))
            || lc.contains("ext4-fs error")
            || lc.contains("read-only")
            || lc.contains("remount-ro")
        {
            suspects.push(l.to_string());
            if suspects.len() > 30 {
                break;
            }
        }
    }
    suspects
}

fn count_macos_junk(root: &str) -> u32 {
    let out = Command::new("find")
        .args([
            root,
            "-type",
            "f",
            "(",
            "-name",
            ".DS_Store",
            "-o",
            "-name",
            "._*",
            "-o",
            "-path",
            "*/__MACOSX/*",
            ")",
            "-print",
        ])
        .output();
    let Ok(o) = out else { return 0 };
    String::from_utf8_lossy(&o.stdout).lines().count() as u32
}

fn find_all_retroarch_cfgs() -> Vec<PathBuf> {
    let mut all = Vec::new();
    let candidates = [
        "/home/ark/.config/retroarch/retroarch.cfg",
        "/home/ark/.config/retroarch32/retroarch.cfg",
        "/root/.config/retroarch/retroarch.cfg",
        "/opt/retroarch/.config/retroarch/retroarch.cfg",
        "/userdata/system/configs/retroarch/retroarch.cfg",
    ];
    for c in &candidates {
        let p = Path::new(c);
        if p.is_file() {
            all.push(p.to_path_buf());
        }
    }
    if all.is_empty() {
        if let Ok(out) = Command::new("find")
            .args([
                "/home",
                "/root",
                "/opt",
                "/userdata",
                "-maxdepth",
                "6",
                "-name",
                "retroarch.cfg",
                "-type",
                "f",
                "-not",
                "-path",
                "*/cores/*",
            ])
            .output()
        {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let p = PathBuf::from(line.trim());
                if p.is_file() {
                    all.push(p);
                }
            }
        }
    }
    all
}

fn find_retroarch_overrides() -> Vec<PathBuf> {
    let mut all = Vec::new();
    for base in [
        "/home/ark/.config/retroarch/config",
        "/home/ark/.config/retroarch32/config",
    ] {
        if let Ok(out) = Command::new("find")
            .args([base, "-maxdepth", "6", "-type", "f", "-name", "*.cfg"])
            .output()
        {
            for l in String::from_utf8_lossy(&out.stdout).lines() {
                all.push(PathBuf::from(l.trim()));
            }
        }
    }
    all
}

fn scan_gamelists(root: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let Ok(out) = Command::new("find")
        .args([
            root,
            "-maxdepth",
            "3",
            "-name",
            "gamelist.xml",
            "-type",
            "f",
        ])
        .output()
    else {
        return bad;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let p = PathBuf::from(line.trim());
        if let Ok(content) = std::fs::read_to_string(&p) {
            let trimmed = content.trim_start();
            if !trimmed.starts_with("<?xml")
                || !content.contains("<gameList")
                || !content.contains("</gameList>")
            {
                bad.push(p.display().to_string());
            }
        } else {
            bad.push(p.display().to_string());
        }
    }
    bad
}

fn scan_broken_cues(root: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let Ok(out) = Command::new("find")
        .args([root, "-name", "*.cue", "-type", "f"])
        .output()
    else {
        return bad;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let cue = PathBuf::from(line.trim());
        let Ok(content) = std::fs::read_to_string(&cue) else {
            continue;
        };
        let parent = cue.parent().unwrap_or(Path::new("."));
        for raw in content.lines() {
            let l = raw.trim();
            if let Some(rest) = l.strip_prefix("FILE ") {
                if let Some(start) = rest.find('"') {
                    if let Some(end) = rest[start + 1..].find('"') {
                        let fname = &rest[start + 1..start + 1 + end];
                        let target = parent.join(fname);
                        if !target.exists() {
                            bad.push(format!("{} -> missing {}", cue.display(), fname));
                        }
                    }
                }
            }
        }
    }
    bad
}

fn scan_broken_m3us(root: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let Ok(out) = Command::new("find")
        .args([root, "-name", "*.m3u", "-type", "f"])
        .output()
    else {
        return bad;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let m = PathBuf::from(line.trim());
        let Ok(content) = std::fs::read_to_string(&m) else {
            continue;
        };
        let parent = m.parent().unwrap_or(Path::new("."));
        for raw in content.lines() {
            let l = raw.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let target = parent.join(l);
            if !target.exists() {
                bad.push(format!("{} -> missing {}", m.display(), l));
            }
        }
    }
    bad
}

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp.playora");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}
