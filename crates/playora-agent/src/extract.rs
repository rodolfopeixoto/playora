use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn cmd(file: &str, dest: Option<&str>, keep: bool) -> Result<()> {
    let src = Path::new(file)
        .canonicalize()
        .with_context(|| format!("source: {file}"))?;
    let dest_dir: PathBuf = match dest {
        Some(d) => Path::new(d).to_path_buf(),
        None => src
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    std::fs::create_dir_all(&dest_dir)?;

    let name = src.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let lower = name.to_lowercase();
    println!("source: {}", src.display());
    println!("dest:   {}", dest_dir.display());
    println!();

    let cmd_args: Vec<&str> = if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        vec!["tar", "-xzf", "-C"]
    } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        vec!["tar", "-xJf", "-C"]
    } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
        vec!["tar", "-xjf", "-C"]
    } else if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
        vec!["tar", "--zstd", "-xf", "-C"]
    } else if lower.ends_with(".tar") {
        vec!["tar", "-xf", "-C"]
    } else if lower.ends_with(".zip") {
        vec!["unzip", "-o", "-d"]
    } else if lower.ends_with(".7z") {
        vec!["7z", "x", "-y", "-o"]
    } else if lower.ends_with(".rar") {
        vec!["unrar", "x", "-y", "-o+"]
    } else if lower.ends_with(".gz") {
        vec!["gunzip", "-k"]
    } else if lower.ends_with(".xz") {
        vec!["unxz", "-k"]
    } else if lower.ends_with(".bz2") {
        vec!["bunzip2", "-k"]
    } else {
        return Err(anyhow!("unsupported format: {}", name));
    };

    let tool = cmd_args[0];
    if which(tool).is_none() {
        return Err(anyhow!(
            "missing tool '{tool}'. install via apt/portmaster."
        ));
    }

    let status = match tool {
        "tar" => Command::new("tar")
            .args(&cmd_args[1..cmd_args.len() - 1])
            .arg(&src)
            .arg(cmd_args[cmd_args.len() - 1])
            .arg(&dest_dir)
            .status()?,
        "unzip" => Command::new("unzip")
            .arg("-o")
            .arg(&src)
            .arg("-d")
            .arg(&dest_dir)
            .status()?,
        "7z" => Command::new("7z")
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", dest_dir.display()))
            .arg(&src)
            .status()?,
        "unrar" => Command::new("unrar")
            .arg("x")
            .arg("-y")
            .arg(&src)
            .arg(&dest_dir)
            .status()?,
        "gunzip" | "unxz" | "bunzip2" => Command::new(tool).arg("-k").arg(&src).status()?,
        _ => unreachable!(),
    };

    if !status.success() {
        return Err(anyhow!("{tool} exited with code {:?}", status.code()));
    }

    if !keep {
        std::fs::remove_file(&src).ok();
        println!("removed source archive");
    }
    println!("done.");
    Ok(())
}

fn which(tool: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let p = Path::new(dir).join(tool);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Walk inbox for archives, extract to temp, route every ROM into /roms/<system>/.
/// Routing by extension. Ambiguous (zip/7z) → stays in temp + warning.
pub fn cmd_extract_roms(inbox: &str, roms_root: &str, keep: bool) -> Result<()> {
    use playora_common::systems::SYSTEMS;
    use std::collections::HashMap;

    let inbox_p = Path::new(inbox);
    if !inbox_p.exists() {
        std::fs::create_dir_all(inbox_p).ok();
        println!("inbox empty: {inbox} (created)");
        return Ok(());
    }
    let mut ext_to_folder: HashMap<&str, &str> = HashMap::new();
    for sys in SYSTEMS {
        for ext in sys.extensions {
            if *ext == "zip" || *ext == "7z" {
                continue;
            }
            ext_to_folder.entry(ext).or_insert(sys.folder);
        }
    }

    let mut archive_count = 0;
    let mut routed = 0;
    let mut unrouted = 0;
    let mut errors = 0;

    let entries: Vec<_> = std::fs::read_dir(inbox_p)
        .with_context(|| format!("read {inbox}"))?
        .filter_map(|e| e.ok())
        .collect();

    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let lower = name.to_lowercase();
        let is_archive = [
            ".zip", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".7z",
            ".rar", ".gz", ".xz", ".bz2",
        ]
        .iter()
        .any(|s| lower.ends_with(s));

        let mut ext = lower.rsplit('.').next().unwrap_or("").to_string();
        if !is_archive {
            if let Some(folder) = ext_to_folder.get(ext.as_str()) {
                let dest = Path::new(roms_root).join(folder);
                std::fs::create_dir_all(&dest).ok();
                let target = dest.join(&name);
                if let Err(e) = std::fs::rename(&path, &target) {
                    eprintln!("route fail {name}: {e}");
                    errors += 1;
                } else {
                    println!("routed {name} -> {}/", folder);
                    routed += 1;
                }
            } else {
                println!("skip (unknown ext .{ext}): {name}");
                unrouted += 1;
            }
            continue;
        }

        archive_count += 1;
        let tmp = inbox_p.join(format!(".extract_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).ok();
        println!("==> extracting {name}");
        let r = cmd(path.to_str().unwrap(), Some(tmp.to_str().unwrap()), true);
        if let Err(e) = r {
            eprintln!("extract fail {name}: {e}");
            errors += 1;
            std::fs::remove_dir_all(&tmp).ok();
            continue;
        }
        // Walk tmp and route each file
        walk_and_route(&tmp, roms_root, &ext_to_folder, &mut routed, &mut unrouted);
        std::fs::remove_dir_all(&tmp).ok();
        if !keep {
            std::fs::remove_file(&path).ok();
        }
        let _ = &mut ext;
    }

    println!();
    println!("== ExtractRoms summary ==");
    println!("archives: {archive_count}");
    println!("routed:   {routed}");
    println!("unrouted: {unrouted}");
    println!("errors:   {errors}");
    println!("inbox:    {inbox}");
    println!("roms:     {roms_root}");
    if errors > 0 {
        return Err(anyhow!("{errors} errors during extraction"));
    }
    Ok(())
}

fn walk_and_route(
    dir: &Path,
    roms_root: &str,
    ext_to_folder: &std::collections::HashMap<&str, &str>,
    routed: &mut u32,
    unrouted: &mut u32,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_and_route(&p, roms_root, ext_to_folder, routed, unrouted);
            continue;
        }
        let fname = match p.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if fname.starts_with("._") || fname == ".DS_Store" {
            std::fs::remove_file(&p).ok();
            continue;
        }
        let ext = fname.rsplit('.').next().unwrap_or("").to_lowercase();
        if let Some(folder) = ext_to_folder.get(ext.as_str()) {
            let dest_dir = Path::new(roms_root).join(folder);
            std::fs::create_dir_all(&dest_dir).ok();
            let dest = dest_dir.join(&fname);
            if let Err(err) = std::fs::rename(&p, &dest) {
                let _ = std::fs::copy(&p, &dest).map_err(|e| {
                    eprintln!("copy fail {fname}: {e} (rename: {err})");
                });
                std::fs::remove_file(&p).ok();
            }
            println!("  -> {}/{fname}", folder);
            *routed += 1;
        } else {
            println!("  (skip {fname}: unknown ext .{ext})");
            *unrouted += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_unknown_format() {
        let r = cmd("/tmp/missing.unknown", None, true);
        assert!(r.is_err());
    }

    #[test]
    fn empty_inbox_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        let roms = tmp.path().join("roms");
        let r = cmd_extract_roms(inbox.to_str().unwrap(), roms.to_str().unwrap(), false);
        assert!(r.is_ok());
    }

    #[test]
    fn routes_loose_rom_by_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        let roms = tmp.path().join("roms");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("game.gba"), b"x").unwrap();
        let r = cmd_extract_roms(inbox.to_str().unwrap(), roms.to_str().unwrap(), false);
        assert!(r.is_ok(), "{:?}", r);
        assert!(roms.join("gba").join("game.gba").exists());
    }
}
