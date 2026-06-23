//! Storage analysis: disk space, partition info, cleanup candidates.

use darkos_core::Result;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct DiskUsage {
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub used_pct: f32,
}

/// Wraps statvfs (Unix). On macOS dev box, returns volume stats. Same syscall.
pub fn disk_usage(path: impl AsRef<Path>) -> Result<DiskUsage> {
    use std::ffi::CString;
    use std::os::raw::c_char;

    #[repr(C)]
    #[derive(Default)]
    struct StatVfs {
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
        fn statvfs(path: *const c_char, buf: *mut StatVfs) -> i32;
    }

    let p = path.as_ref().to_string_lossy().into_owned();
    let c = CString::new(p.clone()).map_err(|e| darkos_core::Error::Other(e.to_string()))?;
    let mut s = StatVfs::default();
    let rc = unsafe { statvfs(c.as_ptr(), &mut s as *mut _) };
    if rc != 0 {
        return Err(darkos_core::Error::Io(std::io::Error::last_os_error()));
    }
    let total = s.f_blocks * s.f_frsize;
    let free = s.f_bavail * s.f_frsize;
    let used = total.saturating_sub(free);
    let used_pct = if total > 0 {
        (used as f32 / total as f32) * 100.0
    } else {
        0.0
    };
    Ok(DiskUsage {
        path: p,
        total_bytes: total,
        free_bytes: free,
        used_bytes: used,
        used_pct,
    })
}

/// Suggest cleanup candidates (logs, caches, .DS_Store, ._* macOS metadata).
pub fn cleanup_candidates(root: impl AsRef<Path>) -> Vec<String> {
    let root = root.as_ref();
    let mut hits = vec![];
    let patterns = [".DS_Store", "Thumbs.db"];
    if let Ok(rd) = walkdir_safe(root) {
        for path in rd {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if patterns.iter().any(|p| name == *p) || name.starts_with("._") {
                hits.push(path.display().to_string());
            }
        }
    }
    hits
}

// Tiny non-recursive directory walk (no extra dep here).
fn walkdir_safe(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut out = vec![];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}
