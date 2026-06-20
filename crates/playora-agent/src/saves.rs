use anyhow::{anyhow, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use playora_common::AgentConfig;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SAVE_EXTS: &[&str] = &[
    "srm", "sav", "state", "state1", "state2", "state3", "state4", "state5", "rtc", "mcr", "fla",
    "sa1", "sa2", "eep", "auto",
];
const SAVE_DIRS: &[&str] = &["savestates", "saves"];

pub fn collect(roms_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(roms_dir)
        .max_depth(5)
        .into_iter()
        .flatten()
    {
        if !e.file_type().is_file() {
            continue;
        }
        let p = e.path();
        if let Some(parent) = p.parent() {
            let pname = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if SAVE_DIRS.contains(&pname) {
                out.push(p.into());
                continue;
            }
        }
        if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
            if SAVE_EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
                out.push(p.into());
            }
        }
    }
    out
}

pub fn pack(roms_dir: &Path, dest_tgz: &Path) -> Result<u64> {
    let f = std::fs::File::create(dest_tgz)?;
    let enc = GzEncoder::new(f, Compression::fast());
    let mut tar = tar::Builder::new(enc);
    for p in collect(roms_dir) {
        let rel = p.strip_prefix(roms_dir).unwrap_or(&p);
        let mut file = std::fs::File::open(&p)?;
        let meta = file.metadata()?;
        let mut header = tar::Header::new_gnu();
        header.set_path(rel)?;
        header.set_size(meta.len());
        header.set_mode(0o644);
        header.set_cksum();
        let mut buf = Vec::with_capacity(meta.len() as usize);
        file.read_to_end(&mut buf)?;
        tar.append(&header, std::io::Cursor::new(buf))?;
    }
    let enc = tar.into_inner()?;
    let mut out = enc.finish()?;
    out.flush()?;
    let sz = std::fs::metadata(dest_tgz)?.len();
    Ok(sz)
}

pub fn upload(cfg: &AgentConfig, tgz: &Path) -> Result<String> {
    let url = format!(
        "{}/api/v1/saves/upload?device_id={}",
        cfg.server_url.trim_end_matches('/'),
        cfg.device_id
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let bytes = std::fs::read(tgz)?;
    let resp = client.post(&url).body(bytes).send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {}", resp.status()));
    }
    Ok(resp.text()?)
}

pub fn cmd_pack(cfg: AgentConfig, dest: Option<String>) -> Result<()> {
    let roms = cfg
        .rom_paths
        .first()
        .ok_or_else(|| anyhow!("rom_paths empty"))?;
    let out_path = match dest {
        Some(s) => PathBuf::from(s),
        None => std::env::temp_dir().join(format!(
            "playora_saves_{}.tar.gz",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        )),
    };
    let sz = pack(Path::new(roms), &out_path)?;
    println!("packed {} bytes -> {}", sz, out_path.display());
    Ok(())
}

pub fn cmd_upload(cfg: AgentConfig) -> Result<()> {
    let roms = cfg
        .rom_paths
        .first()
        .ok_or_else(|| anyhow!("rom_paths empty"))?
        .clone();
    let tgz = std::env::temp_dir().join(format!(
        "playora_saves_{}.tar.gz",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    ));
    let sz = pack(Path::new(&roms), &tgz)?;
    println!("packed {} bytes; uploading...", sz);
    let resp = upload(&cfg, &tgz)?;
    println!("server: {resp}");
    let _ = std::fs::remove_file(&tgz);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn collect_finds_save_extensions() {
        let dir = tempdir().unwrap();
        let snes = dir.path().join("snes");
        std::fs::create_dir_all(&snes).unwrap();
        std::fs::write(snes.join("game.sfc"), b"x").unwrap();
        std::fs::write(snes.join("game.srm"), b"y").unwrap();
        let saves = collect(dir.path());
        assert!(saves.iter().any(|p| p.ends_with("game.srm")));
        assert!(!saves.iter().any(|p| p.ends_with("game.sfc")));
    }

    #[test]
    fn pack_creates_nonempty_tgz() {
        let dir = tempdir().unwrap();
        let snes = dir.path().join("snes");
        std::fs::create_dir_all(&snes).unwrap();
        std::fs::write(snes.join("a.srm"), vec![0u8; 1024]).unwrap();
        let out = dir.path().join("saves.tar.gz");
        let sz = pack(dir.path(), &out).unwrap();
        assert!(sz > 0);
        assert!(out.exists());
    }
}
