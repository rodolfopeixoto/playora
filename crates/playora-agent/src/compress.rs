//! Multi-system ROM compressor.
//!
//! Per-system best-tool:
//!   PS1/Saturn/Dreamcast/PS2/Naomi → chdman (MAME tools)
//!   PSP                            → maxcso (.iso -> .cso)
//!   GameCube/Wii                   → dolphin-tool (.iso -> .rvz)
//!
//! Skips already-compressed files. Skips a system entirely if its tool
//! is missing — log shows install hint.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy)]
enum Tool {
    Chdman,
    Maxcso,
    DolphinTool,
}

impl Tool {
    fn cmd(self) -> &'static str {
        match self {
            Tool::Chdman => "chdman",
            Tool::Maxcso => "maxcso",
            Tool::DolphinTool => "dolphin-tool",
        }
    }
    fn install_hint(self) -> &'static str {
        match self {
            Tool::Chdman => "sudo apt-get install mame-tools",
            Tool::Maxcso => "https://github.com/unknownbrackets/maxcso/releases",
            Tool::DolphinTool => {
                "sudo apt-get install dolphin-emu  (or copy dolphin-tool into /roms/.playora/bin)"
            }
        }
    }
}

struct SystemSpec {
    folders: &'static [&'static str],
    /// Source extensions (lowercase) to convert.
    src_exts: &'static [&'static str],
    target_ext: &'static str,
    tool: Tool,
}

const SPECS: &[SystemSpec] = &[
    SystemSpec {
        folders: &["psx", "ps1", "playstation"],
        src_exts: &["cue", "iso"],
        target_ext: "chd",
        tool: Tool::Chdman,
    },
    SystemSpec {
        folders: &["saturn"],
        src_exts: &["cue", "iso"],
        target_ext: "chd",
        tool: Tool::Chdman,
    },
    SystemSpec {
        folders: &["dreamcast", "dc"],
        src_exts: &["cue", "gdi"],
        target_ext: "chd",
        tool: Tool::Chdman,
    },
    SystemSpec {
        folders: &["ps2"],
        src_exts: &["cue", "iso"],
        target_ext: "chd",
        tool: Tool::Chdman,
    },
    SystemSpec {
        folders: &["naomi", "atomiswave"],
        src_exts: &["cue", "gdi"],
        target_ext: "chd",
        tool: Tool::Chdman,
    },
    SystemSpec {
        folders: &["psp"],
        src_exts: &["iso"],
        target_ext: "cso",
        tool: Tool::Maxcso,
    },
    SystemSpec {
        folders: &["gc", "gamecube", "wii"],
        src_exts: &["iso"],
        target_ext: "rvz",
        tool: Tool::DolphinTool,
    },
];

pub fn cmd_compress_roms(roms_root: &str, dry_run: bool) -> Result<()> {
    let _lock = crate::lockfile::acquire("compress-roms")?;
    let cfg = crate::cfg::load(None).ok();

    let root = Path::new(roms_root);
    if !root.is_dir() {
        return Err(anyhow!("roms root not found: {roms_root}"));
    }

    let mut total_found = 0u32;
    let mut total_converted = 0u32;
    let mut total_errors = 0u32;
    let mut total_saved: i64 = 0;
    let mut tools_missing: Vec<Tool> = Vec::new();

    for spec in SPECS {
        for folder in spec.folders {
            let sys_dir = root.join(folder);
            if !sys_dir.is_dir() {
                continue;
            }
            // Tool gate (per-system).
            if which(spec.tool.cmd()).is_none() {
                println!(
                    "skip system {folder}: {} not found (install: {})",
                    spec.tool.cmd(),
                    spec.tool.install_hint()
                );
                if !tools_missing.iter().any(|t| t.cmd() == spec.tool.cmd()) {
                    tools_missing.push(spec.tool);
                }
                continue;
            }
            println!(
                "== {} ({}) -> .{} ==",
                sys_dir.display(),
                spec.tool.cmd(),
                spec.target_ext
            );
            let entries: Vec<_> = walkdir::WalkDir::new(&sys_dir)
                .max_depth(3)
                .into_iter()
                .flatten()
                .filter(|e| e.file_type().is_file())
                .collect();
            for entry in entries {
                let p = entry.path();
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !spec.src_exts.iter().any(|e| *e == ext) {
                    continue;
                }
                let target = p.with_extension(spec.target_ext);
                if target.exists() {
                    continue;
                }
                total_found += 1;
                let in_size = sources_total_size(p);
                println!(
                    "  -> {} ({:.1} MB)",
                    p.display(),
                    in_size as f64 / 1024.0 / 1024.0
                );
                if dry_run {
                    continue;
                }
                if let Some(c) = &cfg {
                    let _ = crate::activity::progress(
                        c,
                        "Compress ROMs",
                        &format!("converting {} -> {}", p.display(), spec.target_ext),
                    );
                }
                let out_tmp = target.with_extension(format!("{}.part", spec.target_ext));
                let status = run_tool(spec.tool, p, &out_tmp);
                match status {
                    Ok(true) => {
                        std::fs::rename(&out_tmp, &target)?;
                        let out_size = std::fs::metadata(&target)
                            .map(|m| m.len() as i64)
                            .unwrap_or(0);
                        let saved = in_size as i64 - out_size;
                        total_saved += saved;
                        delete_sources(p);
                        total_converted += 1;
                        println!(
                            "     ok ({:.1} MB -> {:.1} MB, saved {:.1} MB)",
                            in_size as f64 / 1024.0 / 1024.0,
                            out_size as f64 / 1024.0 / 1024.0,
                            saved as f64 / 1024.0 / 1024.0
                        );
                    }
                    Ok(false) => {
                        std::fs::remove_file(&out_tmp).ok();
                        total_errors += 1;
                    }
                    Err(e) => {
                        eprintln!("     spawn fail: {e}");
                        total_errors += 1;
                    }
                }
            }
        }
    }

    println!();
    println!("== Compress ROMs detail ==");
    println!("candidates: {total_found}");
    println!("converted:  {total_converted}");
    println!("errors:     {total_errors}");
    println!(
        "space saved: {:.1} MB",
        total_saved as f64 / 1024.0 / 1024.0
    );
    if !tools_missing.is_empty() {
        println!();
        println!("missing tools:");
        for t in &tools_missing {
            println!("  - {}: {}", t.cmd(), t.install_hint());
        }
    }
    println!();
    println!(
        "SUMMARY: {total_converted} ROMs compressed, saved {:.1} MB, {total_errors} errors, {} systems",
        total_saved as f64 / 1024.0 / 1024.0,
        SPECS.len()
    );
    if total_errors > 0 {
        return Err(anyhow!("{total_errors} errors"));
    }
    Ok(())
}

fn run_tool(tool: Tool, src: &Path, dst: &Path) -> std::io::Result<bool> {
    let st = match tool {
        Tool::Chdman => Command::new("chdman")
            .args(["createcd", "-i"])
            .arg(src)
            .arg("-o")
            .arg(dst)
            .status()?,
        Tool::Maxcso => Command::new("maxcso")
            .arg(src)
            .arg("-o")
            .arg(dst)
            .status()?,
        Tool::DolphinTool => Command::new("dolphin-tool")
            .args(["convert", "-f", "rvz", "-i"])
            .arg(src)
            .arg("-o")
            .arg(dst)
            .status()?,
    };
    Ok(st.success())
}

fn sources_total_size(cue_or_iso: &Path) -> u64 {
    let mut total = std::fs::metadata(cue_or_iso).map(|m| m.len()).unwrap_or(0);
    match cue_or_iso.extension().and_then(|s| s.to_str()) {
        Some("cue") | Some("gdi") => {
            for bin in parse_track_files(cue_or_iso) {
                let p = cue_or_iso.parent().unwrap_or(Path::new(".")).join(bin);
                total += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            }
        }
        _ => {}
    }
    total
}

fn delete_sources(cue_or_iso: &Path) {
    match cue_or_iso.extension().and_then(|s| s.to_str()) {
        Some("cue") | Some("gdi") => {
            for bin in parse_track_files(cue_or_iso) {
                let p = cue_or_iso.parent().unwrap_or(Path::new(".")).join(bin);
                std::fs::remove_file(&p).ok();
            }
        }
        _ => {}
    }
    std::fs::remove_file(cue_or_iso).ok();
}

/// Parse referenced track files from a .cue or .gdi sheet.
fn parse_track_files(sheet: &Path) -> Vec<String> {
    let s = match std::fs::read_to_string(sheet) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let is_cue = sheet.extension().and_then(|x| x.to_str()) == Some("cue");
    if is_cue {
        s.lines()
            .filter_map(|l| {
                let t = l.trim();
                if !t.to_lowercase().starts_with("file ") {
                    return None;
                }
                let q = t.find('"')?;
                let rest = &t[q + 1..];
                let end = rest.find('"')?;
                Some(rest[..end].to_string())
            })
            .collect()
    } else {
        // GDI: lines like `1 0 4 2352 "track01.bin" 0`
        s.lines()
            .filter_map(|l| {
                let parts: Vec<&str> = l.trim().split_whitespace().collect();
                if parts.len() < 5 {
                    return None;
                }
                let name = parts[4].trim_matches('"');
                Some(name.to_string())
            })
            .collect()
    }
}

fn which(tool: &str) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cue_bin_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let cue = tmp.path().join("game.cue");
        std::fs::write(
            &cue,
            "FILE \"game.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        let bins = parse_track_files(&cue);
        assert_eq!(bins, vec!["game.bin"]);
    }

    #[test]
    fn parses_gdi_track_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let gdi = tmp.path().join("game.gdi");
        std::fs::write(
            &gdi,
            "3\n1 0 4 2352 track01.bin 0\n2 600 0 2352 track02.raw 0\n3 45000 4 2352 track03.bin 0\n",
        )
        .unwrap();
        let tracks = parse_track_files(&gdi);
        assert_eq!(tracks, vec!["track01.bin", "track02.raw", "track03.bin"]);
    }
}
