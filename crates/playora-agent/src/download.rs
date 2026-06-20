use anyhow::{anyhow, Context, Result};
use playora_common::{systems::spec_by_folder, AgentConfig};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MIN_FREE_MARGIN: u64 = 64 * 1024 * 1024;

pub struct DownloadRequest<'a> {
    pub url: &'a str,
    pub system_folder: &'a str,
    pub filename: Option<&'a str>,
    pub expected_sha256: Option<&'a str>,
    pub overwrite: bool,
}

pub struct DownloadOutcome {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

pub fn fetch(cfg: &AgentConfig, req: &DownloadRequest<'_>) -> Result<DownloadOutcome> {
    let roms_root = cfg.rom_paths.first().context("no rom_paths configured")?;
    let sys_spec = spec_by_folder(req.system_folder)
        .ok_or_else(|| anyhow!("unknown system folder: {}", req.system_folder))?;
    let dest_dir = Path::new(roms_root).join(sys_spec.folder);
    std::fs::create_dir_all(&dest_dir)?;

    let filename = req
        .filename
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| infer_filename(req.url));
    if filename.contains('/') || filename.contains("..") {
        return Err(anyhow!("invalid filename: {}", filename));
    }
    let dest = dest_dir.join(&filename);
    if dest.exists() && !req.overwrite {
        return Err(anyhow!("file exists: {} (use --overwrite)", dest.display()));
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;
    let mut resp = client
        .get(req.url)
        .send()
        .with_context(|| format!("GET {}", req.url))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {}", resp.status()));
    }
    let content_len = resp.content_length();
    if let Some(len) = content_len {
        let free = free_bytes(&dest_dir).unwrap_or(u64::MAX);
        if free < len + MIN_FREE_MARGIN {
            return Err(anyhow!(
                "insufficient free space: have {}B, need >= {}B",
                free,
                len + MIN_FREE_MARGIN
            ));
        }
    }

    let tmp = dest.with_extension(format!("{}.part", filename));
    let mut out = std::fs::File::create(&tmp)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        total += n as u64;
    }
    out.sync_all()?;
    drop(out);

    let actual = hex::encode(hasher.finalize());
    if let Some(expected) = req.expected_sha256 {
        if !expected.eq_ignore_ascii_case(&actual) {
            let _ = std::fs::remove_file(&tmp);
            return Err(anyhow!(
                "sha256 mismatch: expected={} got={}",
                expected,
                actual
            ));
        }
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(DownloadOutcome {
        path: dest,
        bytes: total,
        sha256: actual,
    })
}

fn infer_filename(url: &str) -> String {
    let trimmed = url
        .split('?')
        .next()
        .unwrap_or(url)
        .split('#')
        .next()
        .unwrap_or(url);
    let last = trimmed.rsplit('/').next().unwrap_or("download.bin");
    if last.is_empty() {
        "download.bin".into()
    } else {
        last.into()
    }
}

fn free_bytes(p: &Path) -> Option<u64> {
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
    let cp = CString::new(p.to_string_lossy().to_string()).ok()?;
    let mut s = S::default();
    if unsafe { statvfs(cp.as_ptr(), &mut s) } != 0 {
        return None;
    }
    Some(s.f_bavail * s.f_frsize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_basic() {
        assert_eq!(infer_filename("https://x/y/Pokemon.zip"), "Pokemon.zip");
        assert_eq!(
            infer_filename("https://x/y/Pokemon.zip?token=abc"),
            "Pokemon.zip"
        );
        assert_eq!(infer_filename("https://x/"), "download.bin");
    }
}
