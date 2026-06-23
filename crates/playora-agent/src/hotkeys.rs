//! `hotkeys` — comprehensive shortcut cheatsheet across every emulator
//! installed on the device. For each emulator we detect:
//!   - whether the binary exists
//!   - which config file applies
//!   - the user's *current* keybinding for known actions (if cfg is
//!     parseable; otherwise the upstream default)
//!
//! The output is intentionally English so users coming from English-speaking
//! retro forums recognise the standard terminology (quit, save state, load
//! state, take screenshot, fast-forward, rewind, swap disc, ...).
//!
//! Three output modes:
//!   - TTY (default) — grouped, colored, paginated by section
//!   - `--json`      — full structured dump to stdout
//!   - `--system X`  — narrow to a single system / emulator

use anyhow::Result;
use playora_common::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Translate a RetroArch gamepad-button code to a human-readable button name.
/// Reference: libretro `RETRO_DEVICE_ID_JOYPAD_*` (well-known mapping).
fn ra_btn_name(code: &str) -> String {
    match code.trim() {
        "0" => "B",
        "1" => "Y",
        "2" => "Select",
        "3" => "Start",
        "4" => "D-pad Up",
        "5" => "D-pad Down",
        "6" => "D-pad Left",
        "7" => "D-pad Right",
        "8" => "A",
        "9" => "X",
        "10" => "L1",
        "11" => "R1",
        "12" => "L2",
        "13" => "R2",
        "14" => "L3",
        "15" => "R3",
        "nul" | "" => "(unset)",
        other => return format!("btn {other}"),
    }
    .to_string()
}

/// Translate a RetroArch `*_combo` code to a human-readable combo (0–6).
fn ra_quit_combo(code: &str) -> String {
    match code.trim() {
        "0" => "(disabled)",
        "1" => "Down + Y + L1 + R1",
        "2" => "L3 + R3",
        "3" => "L1 + R1 + Start + Select",
        "4" => "L1 + R1 + Select + Start",
        "5" => "Hold Start (2s)",
        "6" => "Hold Select (2s)",
        other => return format!("combo {other}"),
    }
    .to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct Shortcut {
    pub action: &'static str,
    /// Plain English description — what this shortcut does.
    pub description: &'static str,
    /// Default gamepad combo (R36S/dArkOSRE conventions).
    pub default_gamepad: &'static str,
    /// Default keyboard binding (RetroArch / standalone convention).
    pub default_keyboard: &'static str,
    /// Current binding read from cfg (falls back to default if unparsed).
    pub current: Option<String>,
    /// "core" = always available; "core+game" = needs both core + content;
    /// "menu" = only inside the in-game menu; "save-state" = save-state only.
    pub scope: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmulatorEntry {
    pub id: &'static str,
    pub display_name: &'static str,
    pub binary: &'static str,
    pub detected: bool,
    pub config_path: Option<String>,
    pub how_to_exit: &'static str,
    pub how_to_open_menu: &'static str,
    pub notes: Vec<&'static str>,
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Debug, Serialize)]
pub struct Cheatsheet {
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub emulators: Vec<EmulatorEntry>,
}

pub fn cmd_hotkeys(_cfg: AgentConfig, system: Option<String>, json: bool) -> Result<()> {
    let mut sheet = build_cheatsheet();
    if let Some(filter) = system.as_deref() {
        let f = filter.to_ascii_lowercase();
        sheet
            .emulators
            .retain(|e| e.id == f || e.display_name.to_ascii_lowercase().contains(&f));
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&sheet)?);
        return Ok(());
    }

    use crate::ttyui::{self, Status};
    ttyui::header("Hotkeys — Cheatsheet");

    for e in &sheet.emulators {
        ttyui::section(&format!(
            "{} {}",
            e.display_name,
            if e.detected {
                "[installed]"
            } else {
                "[not installed]"
            }
        ));
        ttyui::row(
            "binary",
            e.binary,
            if e.detected { Status::Ok } else { Status::Warn },
        );
        if let Some(p) = &e.config_path {
            ttyui::row("config", p, Status::Info);
        }
        ttyui::row("how to exit", e.how_to_exit, Status::Info);
        ttyui::row("how to open menu", e.how_to_open_menu, Status::Info);
        for n in &e.notes {
            ttyui::note(n);
        }
        for s in &e.shortcuts {
            let current = s.current.as_deref().unwrap_or(s.default_gamepad);
            ttyui::row(
                s.action,
                &format!("{} · keyboard: {}", current, s.default_keyboard),
                Status::Info,
            );
            ttyui::note(s.description);
        }
    }

    // Persist JSON report so dashboard + user can re-read offline.
    let reports = PathBuf::from("/roms/.playora/reports");
    let _ = std::fs::create_dir_all(&reports);
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let path = reports.join(format!("hotkeys-{stamp}.json"));
    if let Ok(j) = serde_json::to_string_pretty(&sheet) {
        let tmp = path.with_extension("tmp.playora");
        let _ = std::fs::write(&tmp, j.as_bytes());
        let _ = std::fs::rename(&tmp, &path);
        println!();
        println!("  saved: {}", path.display());
    }
    Ok(())
}

fn build_cheatsheet() -> Cheatsheet {
    let mut emulators = Vec::new();
    emulators.push(retroarch_entry(
        "retroarch",
        "RetroArch (libretro)",
        &retroarch_cfg_paths(),
    ));
    emulators.push(retroarch_entry(
        "retroarch32",
        "RetroArch32 (libretro, 32-bit cores)",
        &retroarch32_cfg_paths(),
    ));
    emulators.push(ppsspp_entry());
    emulators.push(drastic_entry());
    emulators.push(mupen64plus_entry());
    emulators.push(dosbox_entry());
    emulators.push(scummvm_entry());
    emulators.push(dolphin_entry());
    emulators.push(redream_entry());
    emulators.push(kodi_entry());
    emulators.push(emulationstation_entry());
    Cheatsheet {
        generated_at: chrono::Utc::now(),
        emulators,
    }
}

// ---------------------------------------------------------------
// Per-emulator builders
// ---------------------------------------------------------------

fn retroarch_entry(id: &'static str, display: &'static str, cfg_paths: &[&str]) -> EmulatorEntry {
    let binary = if id == "retroarch32" {
        "retroarch32"
    } else {
        "retroarch"
    };
    let detected = binary_exists(binary);
    let cfg_path = cfg_paths
        .iter()
        .map(|p| Path::new(*p))
        .find(|p| p.is_file())
        .map(|p| p.display().to_string());
    let cfg = cfg_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|c| crate::fix_exit::parse_settings(&c))
        .unwrap_or_default();

    // Helpers to resolve current bindings from cfg.
    let btn = |key: &str| cfg.get(key).cloned().map(|v| ra_btn_name(&v));
    let raw = |key: &str| cfg.get(key).cloned();

    // The "enable hotkey" button is the modifier you hold before every combo.
    let hotkey_btn =
        btn("input_enable_hotkey_btn").unwrap_or_else(|| "Select (default)".to_string());
    let press_twice = raw("quit_press_twice")
        .map(|v| v == "true")
        .unwrap_or(false);

    let shortcuts = vec![
        Shortcut {
            action: "Hotkey modifier (hold before any combo)",
            description: "Hold this button first; every combo below is \"Hotkey + X\". On R36S/dArkOSRE this is Select by default.",
            default_gamepad: "Select",
            default_keyboard: "—",
            current: Some(hotkey_btn.clone()),
            scope: "core+game",
        },
        Shortcut {
            action: "Quit RetroArch",
            description: "Exit the emulator and return to EmulationStation. If \"quit_press_twice\" is true you must press the combo twice.",
            default_gamepad: "Select + Start (R36S) — RetroArch default combo: L1 + R1 + Select + Start",
            default_keyboard: "F4 (or ESC)",
            current: raw("input_quit_gamepad_combo").map(|v| {
                let combo = ra_quit_combo(&v);
                if press_twice {
                    format!("{combo} (press twice)")
                } else {
                    combo
                }
            }),
            scope: "core+game",
        },
        Shortcut {
            action: "Open RetroArch menu (Quick Menu)",
            description: "Open the in-game menu where you can save/load state, change cores, configure controls, etc.",
            default_gamepad: "Hotkey + B (hotkey usually = Select)",
            default_keyboard: "F1",
            current: btn("input_menu_toggle_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Save state",
            description: "Snapshot the current game state to the active save-state slot. Reload anytime with Load State.",
            default_gamepad: "Hotkey + R1 (Right Shoulder)",
            default_keyboard: "F2",
            current: btn("input_save_state_btn"),
            scope: "save-state",
        },
        Shortcut {
            action: "Load state",
            description: "Restore the most recently saved state for this game.",
            default_gamepad: "Hotkey + L1 (Left Shoulder)",
            default_keyboard: "F4",
            current: btn("input_load_state_btn"),
            scope: "save-state",
        },
        Shortcut {
            action: "Next save-state slot",
            description: "Cycle to the next save-state slot so Save State writes to a fresh slot.",
            default_gamepad: "Hotkey + Right",
            default_keyboard: "F7",
            current: btn("input_state_slot_increase_btn"),
            scope: "save-state",
        },
        Shortcut {
            action: "Previous save-state slot",
            description: "Cycle back to the previous save-state slot.",
            default_gamepad: "Hotkey + Left",
            default_keyboard: "F6",
            current: btn("input_state_slot_decrease_btn"),
            scope: "save-state",
        },
        Shortcut {
            action: "Fast-forward",
            description: "Run the game at maximum speed (typically 2–4x). Useful for grinding or skipping cutscenes.",
            default_gamepad: "Hotkey + R2",
            default_keyboard: "Space",
            current: btn("input_toggle_fast_forward_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Rewind",
            description: "Rewind the game in real-time (must be enabled in Settings > Rewind first).",
            default_gamepad: "Hotkey + L2",
            default_keyboard: "R",
            current: btn("input_rewind_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Reset game",
            description: "Soft-reset the running core (equivalent to pressing the console's reset button).",
            default_gamepad: "Hotkey + X",
            default_keyboard: "H",
            current: btn("input_reset_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Take screenshot",
            description: "Save a PNG screenshot to RetroArch's screenshots folder.",
            default_gamepad: "Hotkey + Y",
            default_keyboard: "F8",
            current: btn("input_screenshot_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Toggle FPS counter",
            description: "Show or hide the FPS counter overlay.",
            default_gamepad: "—",
            default_keyboard: "F3",
            current: None,
            scope: "core",
        },
        Shortcut {
            action: "Toggle pause",
            description: "Pause or resume the running core.",
            default_gamepad: "Hotkey + Start",
            default_keyboard: "P",
            current: btn("input_pause_toggle_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Hold fast-forward",
            description: "Hold to fast-forward; release to return to normal speed.",
            default_gamepad: "Hotkey + R2 (hold)",
            default_keyboard: "L (hold)",
            current: btn("input_hold_fast_forward_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Toggle slow-motion",
            description: "Slow the game down to ~1/3 speed.",
            default_gamepad: "Hotkey + Y (hold)",
            default_keyboard: "E",
            current: btn("input_slowmotion_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Toggle Cheats",
            description: "Enable or disable currently loaded cheat codes.",
            default_gamepad: "—",
            default_keyboard: "(menu only)",
            current: btn("input_cheat_toggle_btn"),
            scope: "menu",
        },
        Shortcut {
            action: "Next cheat index",
            description: "Step forward through loaded cheat codes.",
            default_gamepad: "—",
            default_keyboard: "Y",
            current: btn("input_cheat_index_plus_btn"),
            scope: "menu",
        },
        Shortcut {
            action: "Previous cheat index",
            description: "Step backward through loaded cheat codes.",
            default_gamepad: "—",
            default_keyboard: "T",
            current: btn("input_cheat_index_minus_btn"),
            scope: "menu",
        },
        Shortcut {
            action: "Volume up",
            description: "Increase RetroArch master volume.",
            default_gamepad: "—",
            default_keyboard: "F5",
            current: btn("input_volume_up_btn"),
            scope: "core",
        },
        Shortcut {
            action: "Volume down",
            description: "Decrease RetroArch master volume.",
            default_gamepad: "—",
            default_keyboard: "F6",
            current: btn("input_volume_down_btn"),
            scope: "core",
        },
        Shortcut {
            action: "Toggle audio mute",
            description: "Mute / unmute all audio.",
            default_gamepad: "—",
            default_keyboard: "F9",
            current: btn("input_audio_mute_btn"),
            scope: "core",
        },
        Shortcut {
            action: "Toggle fullscreen",
            description: "Switch between fullscreen and windowed.",
            default_gamepad: "—",
            default_keyboard: "F11 / Alt+Enter",
            current: btn("input_fullscreen_toggle_btn"),
            scope: "core",
        },
        Shortcut {
            action: "Eject / insert disc tray",
            description: "Open or close the virtual disc tray (multi-disc PSX, Saturn, etc.).",
            default_gamepad: "—",
            default_keyboard: "Insert",
            current: btn("input_disk_eject_toggle_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Next disc",
            description: "Cycle to the next disc image after ejecting the tray.",
            default_gamepad: "—",
            default_keyboard: "Page Up",
            current: btn("input_disk_next_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Previous disc",
            description: "Cycle to the previous disc image.",
            default_gamepad: "—",
            default_keyboard: "Page Down",
            current: btn("input_disk_prev_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Next shader preset",
            description: "Cycle to the next shader preset in the same folder.",
            default_gamepad: "—",
            default_keyboard: "N",
            current: btn("input_shader_next_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Previous shader preset",
            description: "Cycle back to the previous shader preset.",
            default_gamepad: "—",
            default_keyboard: "M",
            current: btn("input_shader_prev_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Recording / streaming toggle",
            description: "Toggle local recording / streaming (when configured).",
            default_gamepad: "—",
            default_keyboard: "O",
            current: btn("input_recording_toggle_btn"),
            scope: "core+game",
        },
        Shortcut {
            action: "Toggle Run-Ahead",
            description: "Toggle latency-reducing run-ahead frames if enabled in Settings > Latency.",
            default_gamepad: "—",
            default_keyboard: "(menu only)",
            current: btn("input_run_ahead_toggle_btn"),
            scope: "menu",
        },
        Shortcut {
            action: "AI Service",
            description: "Trigger the configured AI service (e.g. live translation).",
            default_gamepad: "—",
            default_keyboard: "(unset)",
            current: btn("input_ai_service_btn"),
            scope: "core+game",
        },
    ];

    EmulatorEntry {
        id,
        display_name: display,
        binary,
        detected,
        config_path: cfg_path,
        how_to_exit:
            "Press Select+Start (R36S default) — if `quit_press_twice = true` press twice. Always returns to EmulationStation when the exit-game fix is applied.",
        how_to_open_menu:
            "Press Hotkey + B (hotkey is usually Select on R36S) to open the Quick Menu.",
        notes: vec![
            "On the R36S/dArkOSRE the \"hotkey\" button is Select by default. Combos shown as \"Hotkey + X\" mean hold Select then press X.",
            "Per-core or per-game overrides in `<config>/<core>/<game>.cfg` can override any of the above.",
            "If a shortcut does nothing, run `playora-agent doctor --deep` — `input_keyboard_gamepad_enable` may be off.",
        ],
        shortcuts,
    }
}

fn ppsspp_entry() -> EmulatorEntry {
    let detected =
        binary_exists("PPSSPPSDL") || binary_exists("ppsspp") || binary_exists("ppsspp-sdl");
    let cfg_path = [
        "/home/ark/.config/ppsspp/PSP/SYSTEM/controls.ini",
        "/home/ark/.config/ppsspp/PSP/SYSTEM/ppsspp.ini",
    ]
    .into_iter()
    .find(|p| Path::new(p).is_file())
    .map(|s| s.to_string());
    EmulatorEntry {
        id: "ppsspp",
        display_name: "PPSSPP (PSP)",
        binary: "PPSSPPSDL",
        detected,
        config_path: cfg_path,
        how_to_exit:
            "Press Select + Start (PPSSPP \"Quit emulator\" combo on R36S). PPSSPP does NOT honour the RetroArch quit combo.",
        how_to_open_menu:
            "Press Select alone — opens the pause / save-state menu.",
        notes: vec![
            "PPSSPP is standalone — RetroArch settings (quit_press_twice etc.) do not apply.",
            "If exit freezes, run `playora-agent recover` from SSH.",
        ],
        shortcuts: vec![
            Shortcut { action: "Quit emulator", description: "Exit PPSSPP and return to EmulationStation.", default_gamepad: "Select + Start", default_keyboard: "ESC", current: None, scope: "core+game" },
            Shortcut { action: "Open pause menu", description: "Open the in-game menu (save/load state, settings, etc.).", default_gamepad: "Select", default_keyboard: "ESC", current: None, scope: "core+game" },
            Shortcut { action: "Save state", description: "Save current state to the active slot.", default_gamepad: "Pause menu > Save State", default_keyboard: "F2", current: None, scope: "save-state" },
            Shortcut { action: "Load state", description: "Load the active save-state slot.", default_gamepad: "Pause menu > Load State", default_keyboard: "F4", current: None, scope: "save-state" },
            Shortcut { action: "Toggle fast-forward", description: "Run the PSP at maximum speed.", default_gamepad: "Hold R2 (mapped)", default_keyboard: "Tab", current: None, scope: "core+game" },
            Shortcut { action: "Take screenshot", description: "Save a PNG to PPSSPP's screenshots folder.", default_gamepad: "—", default_keyboard: "F12", current: None, scope: "core" },
        ],
    }
}

fn drastic_entry() -> EmulatorEntry {
    let detected = binary_exists("drastic") || Path::new("/opt/drastic/drastic").is_file();
    let cfg_path = [
        "/home/ark/.config/drastic/config/drastic.cfg",
        "/opt/drastic/config/drastic.cfg",
    ]
    .into_iter()
    .find(|p| Path::new(p).is_file())
    .map(|s| s.to_string());
    EmulatorEntry {
        id: "drastic",
        display_name: "DraStic (Nintendo DS)",
        binary: "drastic",
        detected,
        config_path: cfg_path,
        how_to_exit: "Press Select + Start to open menu, then choose \"Exit DraStic\". DraStic does NOT honour the RetroArch quit combo.",
        how_to_open_menu: "Select + Start — opens DraStic's main menu.",
        notes: vec![
            "DraStic is standalone closed-source. No cfg parsing.",
            "Touch is mapped to right analog by default.",
        ],
        shortcuts: vec![
            Shortcut { action: "Open menu", description: "Open DraStic's main menu (states, settings, exit).", default_gamepad: "Select + Start", default_keyboard: "—", current: None, scope: "core+game" },
            Shortcut { action: "Swap screens", description: "Swap top/bottom DS screens layout.", default_gamepad: "L1 + R1 + Select", default_keyboard: "—", current: None, scope: "core+game" },
            Shortcut { action: "Save state", description: "Quick save the current state.", default_gamepad: "Menu > Save State", default_keyboard: "—", current: None, scope: "save-state" },
            Shortcut { action: "Load state", description: "Quick load the most recent state.", default_gamepad: "Menu > Load State", default_keyboard: "—", current: None, scope: "save-state" },
            Shortcut { action: "Toggle fast-forward", description: "Maximum-speed mode.", default_gamepad: "R2", default_keyboard: "—", current: None, scope: "core+game" },
        ],
    }
}

fn mupen64plus_entry() -> EmulatorEntry {
    let detected = binary_exists("mupen64plus");
    let cfg_path = ["/home/ark/.config/mupen64plus/mupen64plus.cfg"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .map(|s| s.to_string());
    EmulatorEntry {
        id: "mupen64plus",
        display_name: "Mupen64Plus (N64 standalone)",
        binary: "mupen64plus",
        detected,
        config_path: cfg_path,
        how_to_exit: "Press ESC on keyboard, or Select + Start on gamepad if mapped via gptokeyb.",
        how_to_open_menu: "No in-game menu — quit and relaunch to change settings.",
        notes: vec![
            "RetroArch's mupen64plus_next core is usually a better choice on R36S.",
            "If exit freezes, run `playora-agent recover`.",
        ],
        shortcuts: vec![
            Shortcut {
                action: "Quit emulator",
                description: "Exit the emulator.",
                default_gamepad: "Select + Start (via gptokeyb)",
                default_keyboard: "ESC",
                current: None,
                scope: "core+game",
            },
            Shortcut {
                action: "Save state",
                description: "Quick save state.",
                default_gamepad: "—",
                default_keyboard: "F5",
                current: None,
                scope: "save-state",
            },
            Shortcut {
                action: "Load state",
                description: "Quick load state.",
                default_gamepad: "—",
                default_keyboard: "F7",
                current: None,
                scope: "save-state",
            },
            Shortcut {
                action: "Take screenshot",
                description: "Save screenshot.",
                default_gamepad: "—",
                default_keyboard: "F12",
                current: None,
                scope: "core+game",
            },
            Shortcut {
                action: "Toggle fullscreen",
                description: "Switch fullscreen.",
                default_gamepad: "—",
                default_keyboard: "Alt+Enter",
                current: None,
                scope: "core",
            },
        ],
    }
}

fn dosbox_entry() -> EmulatorEntry {
    let detected = binary_exists("dosbox") || binary_exists("dosbox-staging");
    EmulatorEntry {
        id: "dosbox",
        display_name: "DOSBox (DOS)",
        binary: "dosbox",
        detected,
        config_path: None,
        how_to_exit: "Type `exit` at DOS prompt or press Ctrl+F9 on keyboard.",
        how_to_open_menu: "DOSBox has no in-game menu; use the keyboard mapper (Ctrl+F1).",
        notes: vec!["DOSBox bindings are keyboard-first. R36S maps gamepad to keys via gptokeyb."],
        shortcuts: vec![
            Shortcut {
                action: "Quit emulator",
                description: "Exit DOSBox.",
                default_gamepad: "Select + Start",
                default_keyboard: "Ctrl+F9",
                current: None,
                scope: "core+game",
            },
            Shortcut {
                action: "Open keyboard mapper",
                description: "Remap keys / gamepad buttons.",
                default_gamepad: "—",
                default_keyboard: "Ctrl+F1",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Toggle fullscreen",
                description: "Switch fullscreen.",
                default_gamepad: "—",
                default_keyboard: "Alt+Enter",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Speed up (cycles)",
                description: "Increase CPU cycles.",
                default_gamepad: "—",
                default_keyboard: "Ctrl+F12",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Slow down (cycles)",
                description: "Decrease CPU cycles.",
                default_gamepad: "—",
                default_keyboard: "Ctrl+F11",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Take screenshot",
                description: "Save screenshot.",
                default_gamepad: "—",
                default_keyboard: "Ctrl+F5",
                current: None,
                scope: "core+game",
            },
        ],
    }
}

fn scummvm_entry() -> EmulatorEntry {
    let detected = binary_exists("scummvm");
    EmulatorEntry {
        id: "scummvm",
        display_name: "ScummVM (point-and-click adventures)",
        binary: "scummvm",
        detected,
        config_path: ["/home/ark/.config/scummvm/scummvm.ini"].into_iter().find(|p| Path::new(p).is_file()).map(|s| s.to_string()),
        how_to_exit: "Press Ctrl+Q on keyboard, or use the in-game GMM (global main menu) > Return to Launcher.",
        how_to_open_menu: "Press Ctrl+F5 to open the global main menu (GMM).",
        notes: vec![
            "Mouse is mapped to right analog by default.",
        ],
        shortcuts: vec![
            Shortcut { action: "Quit emulator", description: "Exit ScummVM and return to the launcher.", default_gamepad: "Select + Start", default_keyboard: "Ctrl+Q", current: None, scope: "core+game" },
            Shortcut { action: "Open GMM", description: "Global main menu — save, load, options.", default_gamepad: "Hotkey + Start", default_keyboard: "Ctrl+F5", current: None, scope: "core+game" },
            Shortcut { action: "Save state", description: "Save state via the GMM.", default_gamepad: "Menu > Save", default_keyboard: "Ctrl+F5 > Save", current: None, scope: "save-state" },
            Shortcut { action: "Load state", description: "Load state via the GMM.", default_gamepad: "Menu > Load", default_keyboard: "Ctrl+F5 > Load", current: None, scope: "save-state" },
            Shortcut { action: "Toggle fast-forward", description: "Speed up cutscenes / dialog.", default_gamepad: "Hold R2", default_keyboard: "Ctrl+F", current: None, scope: "core+game" },
        ],
    }
}

fn dolphin_entry() -> EmulatorEntry {
    let detected = binary_exists("dolphin-emu") || binary_exists("dolphin");
    EmulatorEntry {
        id: "dolphin",
        display_name: "Dolphin (GameCube / Wii) — RK3326 cannot run this well",
        binary: "dolphin-emu",
        detected,
        config_path: None,
        how_to_exit: "Alt+F4 on keyboard. Standalone Dolphin does not honour the RetroArch combo.",
        how_to_open_menu: "Right-click the window (Esc shows menu in some builds).",
        notes: vec!["R36S/RK3326 is far too slow for GC/Wii — listed for completeness."],
        shortcuts: vec![
            Shortcut {
                action: "Quit emulator",
                description: "Close Dolphin window.",
                default_gamepad: "Select + Start",
                default_keyboard: "Alt+F4",
                current: None,
                scope: "core+game",
            },
            Shortcut {
                action: "Save state",
                description: "Quick save.",
                default_gamepad: "—",
                default_keyboard: "Shift+F1..F8",
                current: None,
                scope: "save-state",
            },
            Shortcut {
                action: "Load state",
                description: "Quick load.",
                default_gamepad: "—",
                default_keyboard: "F1..F8",
                current: None,
                scope: "save-state",
            },
        ],
    }
}

fn redream_entry() -> EmulatorEntry {
    let detected = binary_exists("redream");
    EmulatorEntry {
        id: "redream",
        display_name: "Redream (Dreamcast standalone)",
        binary: "redream",
        detected,
        config_path: ["/home/ark/.config/redream/redream.cfg"]
            .into_iter()
            .find(|p| Path::new(p).is_file())
            .map(|s| s.to_string()),
        how_to_exit: "Press Select + Start to open the menu, then choose Exit.",
        how_to_open_menu: "Select + Start.",
        notes: vec!["RetroArch's Flycast core is usually faster on R36S."],
        shortcuts: vec![
            Shortcut {
                action: "Open menu",
                description: "Open Redream menu.",
                default_gamepad: "Select + Start",
                default_keyboard: "ESC",
                current: None,
                scope: "core+game",
            },
            Shortcut {
                action: "Save state",
                description: "Save state via menu.",
                default_gamepad: "Menu > Save",
                default_keyboard: "—",
                current: None,
                scope: "save-state",
            },
            Shortcut {
                action: "Load state",
                description: "Load state via menu.",
                default_gamepad: "Menu > Load",
                default_keyboard: "—",
                current: None,
                scope: "save-state",
            },
        ],
    }
}

fn kodi_entry() -> EmulatorEntry {
    let detected = binary_exists("kodi") || binary_exists("kodi-standalone");
    EmulatorEntry {
        id: "kodi",
        display_name: "Kodi (media center)",
        binary: "kodi",
        detected,
        config_path: None,
        how_to_exit: "From Kodi home screen, hold Select then Start, or open menu > Power > Exit.",
        how_to_open_menu: "Press the C key (context menu) on keyboard, or Y on gamepad.",
        notes: vec!["Kodi is not an emulator but is bundled in dArkOSRE."],
        shortcuts: vec![
            Shortcut {
                action: "Quit Kodi",
                description: "Exit Kodi to EmulationStation.",
                default_gamepad: "Select + Start (hold)",
                default_keyboard: "ESC",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Context menu",
                description: "Open context menu for the selected item.",
                default_gamepad: "Y",
                default_keyboard: "C",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Play / Pause",
                description: "Toggle playback.",
                default_gamepad: "A",
                default_keyboard: "Space",
                current: None,
                scope: "core+game",
            },
            Shortcut {
                action: "Fullscreen video",
                description: "Switch to full video output.",
                default_gamepad: "—",
                default_keyboard: "Tab",
                current: None,
                scope: "core",
            },
        ],
    }
}

fn emulationstation_entry() -> EmulatorEntry {
    let detected = binary_exists("emulationstation");
    EmulatorEntry {
        id: "emulationstation",
        display_name: "EmulationStation (front-end)",
        binary: "emulationstation",
        detected,
        config_path: ["/home/ark/.emulationstation/es_input.cfg"]
            .into_iter()
            .find(|p| Path::new(p).is_file())
            .map(|s| s.to_string()),
        how_to_exit: "Press Start > Quit > Shutdown System. Never pull the power.",
        how_to_open_menu: "Press Start to open the main menu.",
        notes: vec!["ES navigation is consistent across all systems."],
        shortcuts: vec![
            Shortcut {
                action: "Open main menu",
                description: "Settings, UI mode, scrape, quit, etc.",
                default_gamepad: "Start",
                default_keyboard: "Enter",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Open Game Options",
                description: "Per-game options on a selected ROM.",
                default_gamepad: "Select",
                default_keyboard: "Shift",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Launch game",
                description: "Start the highlighted ROM.",
                default_gamepad: "A",
                default_keyboard: "Enter",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Back",
                description: "Go back one screen.",
                default_gamepad: "B",
                default_keyboard: "Backspace",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Mark as favorite",
                description: "Toggle favorite flag on a ROM.",
                default_gamepad: "Y",
                default_keyboard: "F",
                current: None,
                scope: "core",
            },
            Shortcut {
                action: "Random game",
                description: "Jump to a random game in the current system.",
                default_gamepad: "X",
                default_keyboard: "R",
                current: None,
                scope: "core",
            },
        ],
    }
}

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

fn retroarch_cfg_paths() -> Vec<&'static str> {
    vec![
        "/home/ark/.config/retroarch/retroarch.cfg",
        "/root/.config/retroarch/retroarch.cfg",
        "/opt/retroarch/.config/retroarch/retroarch.cfg",
        "/userdata/system/configs/retroarch/retroarch.cfg",
    ]
}

fn retroarch32_cfg_paths() -> Vec<&'static str> {
    vec![
        "/home/ark/.config/retroarch32/retroarch.cfg",
        "/opt/retroarch32/.config/retroarch/retroarch.cfg",
    ]
}

fn binary_exists(name: &str) -> bool {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        if Path::new(dir).join(name).is_file() {
            return true;
        }
    }
    // Also check bundled location used by dArkOSRE forks.
    for prefix in ["/opt", "/usr/local/bin", "/usr/bin"] {
        if Path::new(prefix).join(name).is_file() {
            return true;
        }
    }
    false
}

#[allow(dead_code)]
pub fn cheatsheet_for_tests() -> Cheatsheet {
    build_cheatsheet()
}

#[allow(dead_code)]
fn _unused() -> BTreeMap<String, String> {
    BTreeMap::new()
}
