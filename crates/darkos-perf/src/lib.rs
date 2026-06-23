//! Performance & resource tuning for RK3326.

use darkos_core::{Error, Result};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Profile {
    PowerSave,
    Balanced,
    Performance,
}

pub fn apply_profile(p: Profile) -> Result<()> {
    let gov = match p {
        Profile::PowerSave => "powersave",
        Profile::Balanced => "schedutil",
        Profile::Performance => "performance",
    };
    write_to_all_cpus(gov)
}

fn write_to_all_cpus(gov: &str) -> Result<()> {
    let base = Path::new("/sys/devices/system/cpu");
    if !base.exists() {
        return Err(Error::Hw(
            "/sys/devices/system/cpu missing (dev host?)".into(),
        ));
    }
    let rd = fs::read_dir(base)?;
    for e in rd.flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if !n.starts_with("cpu")
            || !n
                .chars()
                .nth(3)
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
        {
            continue;
        }
        let p = e.path().join("cpufreq/scaling_governor");
        if p.exists() {
            if let Err(err) = fs::write(&p, gov) {
                eprintln!("warn: write {} = {gov}: {err}", p.display());
            }
        }
    }
    Ok(())
}

pub fn drop_caches() -> Result<()> {
    fs::write("/proc/sys/vm/drop_caches", "3")?;
    Ok(())
}
