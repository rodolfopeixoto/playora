//! Tiny on-device terminal UI helpers.
//!
//! All output goes to stdout. The port-runner.sh wrapper redirects
//! stdout to /dev/tty1 when launched in --mode tty, so these helpers
//! end up rendering on the R36S framebuffer console. On the dev box
//! they degrade to plain stdout.

use std::io::{self, Write};
use std::process::Command;
use std::time::Duration;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const WHITE: &str = "\x1b[37m";
pub const CLEAR: &str = "\x1b[2J\x1b[H";

#[derive(Copy, Clone)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Info,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ok => " ok ",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Info => "info",
        }
    }
    fn color(self) -> &'static str {
        match self {
            Status::Ok => GREEN,
            Status::Warn => YELLOW,
            Status::Fail => RED,
            Status::Info => CYAN,
        }
    }
}

/// Clear screen + draw a centered title banner.
pub fn header(title: &str) {
    print!("{CLEAR}");
    println!("{BOLD}{MAGENTA}╔══════════════════════════════════════════════════════╗{RESET}");
    println!(
        "{BOLD}{MAGENTA}║{RESET} {BOLD}{WHITE}{:<52}{RESET} {BOLD}{MAGENTA}║{RESET}",
        format!("PLAYORA · {title}")
    );
    println!("{BOLD}{MAGENTA}╚══════════════════════════════════════════════════════╝{RESET}");
    println!();
    let _ = io::stdout().flush();
}

/// One labeled row with a colored status pill at the end.
pub fn row(label: &str, value: &str, status: Status) {
    println!(
        "  {:<32} {value:<30}  [{}{}{RESET}]",
        format!("{BOLD}{}{RESET}", label),
        status.color(),
        status.label(),
    );
}

pub fn note(msg: &str) {
    println!("  {DIM}{msg}{RESET}");
}

pub fn section(title: &str) {
    println!();
    println!("  {BOLD}{CYAN}── {title} ──{RESET}");
}

pub fn ok(msg: &str) {
    println!("  {GREEN}✓{RESET} {msg}");
}
pub fn warn(msg: &str) {
    println!("  {YELLOW}!{RESET} {msg}");
}
pub fn fail(msg: &str) {
    println!("  {RED}✗{RESET} {msg}");
}
pub fn info(msg: &str) {
    println!("  {CYAN}·{RESET} {msg}");
}

/// Centered prompt. On real tty1 the script's caller (port-runner.sh)
/// waits for input via `read` after we exit; we just print the hint.
pub fn wait_press(seconds: u32) {
    println!();
    println!(
        "  {DIM}Returning to EmulationStation in {seconds}s · press any button now to skip{RESET}"
    );
    let _ = io::stdout().flush();
    std::thread::sleep(Duration::from_secs(seconds as u64));
}

/// QR code as ANSIUTF8 (text-only). Uses the system `qrencode` binary
/// if available, otherwise falls back to the qrcode Rust crate.
pub fn qr_ansi(text: &str) -> String {
    // Prefer qrencode CLI — its ANSIUTF8 layout is more compact than ours.
    if let Ok(out) = Command::new("qrencode")
        .args(["-t", "ANSIUTF8", "-m", "1", text])
        .output()
    {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).into_owned();
        }
    }
    // Fallback: use the qrcode crate's Dense1x2 renderer.
    match qrcode::QrCode::new(text.as_bytes()) {
        Ok(qr) => qr
            .render::<qrcode::render::unicode::Dense1x2>()
            .dark_color(qrcode::render::unicode::Dense1x2::Light)
            .light_color(qrcode::render::unicode::Dense1x2::Dark)
            .build(),
        Err(_) => format!("(failed to render QR for: {text})"),
    }
}

/// Try to display a QR PNG on the framebuffer via fbv/fbi.
/// Returns Ok if any viewer launched. Caller may render ANSI as fallback.
pub fn qr_png_framebuffer(png_path: &std::path::Path) -> std::io::Result<()> {
    if Command::new("fbv")
        .args(["-d", "1", "-i", "-r", "1"])
        .arg(png_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }
    if Command::new("fbi")
        .args(["-d", "/dev/fb0", "-a", "-noverbose", "-T", "1"])
        .arg(png_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no framebuffer viewer (fbv or fbi)",
    ))
}

/// Whether stdout looks like a real terminal (heuristic).
pub fn is_tty() -> bool {
    // Cheap heuristic: TERM is set to something other than dumb.
    std::env::var("TERM")
        .map(|t| t != "dumb" && !t.is_empty())
        .unwrap_or(false)
}
