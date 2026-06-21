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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_unknown_format() {
        let r = cmd("/tmp/missing.unknown", None, true);
        assert!(r.is_err());
    }
}
