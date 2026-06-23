//! darkos — single-binary CLI for the R36S clone.
//!
//! Subcommands wire to small crates. `darkos tui` runs the menu UI.

use anyhow::Result;
use clap::{Parser, Subcommand};
use darkos_core::Paths;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "darkos",
    version,
    about = "darkOs control panel for R36S clones"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show runtime hardware snapshot (JSON)
    Hw {
        /// Persist snapshot to db
        #[arg(long)]
        save: bool,
    },
    /// Show storage usage of common paths
    Storage,
    /// Scan roms_dir, populate DB
    Scan {
        /// Compute SHA256 for each ROM (slow)
        #[arg(long)]
        hash: bool,
    },
    /// Count roms per system
    Summary,
    /// Snapshot all save files to <DARKOS_HOME>/cache/saves_<ts>/
    Saves,
    /// List known firmwares + current
    Firmware,
    /// List installed themes + small catalog
    Themes,
    /// Probe APT updates count + (with --apply) run apt-get upgrade,
    /// or self-update the `darkos` binary from a release manifest.
    Update {
        /// apt-get upgrade after probing
        #[arg(long)]
        apply: bool,
        /// Self-update the darkos binary from DARKOS_RELEASE_URL (or default).
        #[arg(long = "self")]
        self_update: bool,
        /// Override manifest URL (default: env DARKOS_RELEASE_URL → built-in).
        #[arg(long)]
        url: Option<String>,
        /// Reinstall even if remote == current.
        #[arg(long)]
        force: bool,
        /// Only check; don't replace the binary.
        #[arg(long)]
        check: bool,
    },
    /// Set perf profile: powersave | balanced | performance
    Perf { profile: String },
    /// Run the TUI menu
    Tui,
    /// Print effective paths
    Paths,
    /// Drop kernel caches (free RAM)
    DropCaches,
    /// Print panel/DTB info recovered from /proc/device-tree
    Panel,
    /// Open the scrollable text overlay (file path or stdin)
    View {
        /// Path to file (omit to read stdin)
        file: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("darkos=info")
        .try_init()
        .ok();
    let cli = Cli::parse();
    let paths = Paths::default();
    paths.ensure().ok();

    match cli.cmd {
        Cmd::Hw { save } => {
            let snap = darkos_hw::snapshot()?;
            println!("{}", serde_json::to_string_pretty(&snap)?);
            if save {
                let db = darkos_db::Db::open(&paths.db_path)?;
                let id = db.record_hw_snapshot(&snap)?;
                eprintln!("snapshot persisted id={id}");
            }
        }
        Cmd::Storage => {
            for r in ["/roms", "/", "/boot"] {
                if let Ok(u) = darkos_storage::disk_usage(r) {
                    println!(
                        "{}: used {:.1}% ({} / {}) free {}",
                        u.path,
                        u.used_pct,
                        bytesize::ByteSize(u.used_bytes),
                        bytesize::ByteSize(u.total_bytes),
                        bytesize::ByteSize(u.free_bytes),
                    );
                }
            }
        }
        Cmd::Scan { hash } => {
            let db = darkos_db::Db::open(&paths.db_path)?;
            let n = darkos_roms::scan_into_db(PathBuf::from(&paths.roms_dir).as_path(), &db, hash)?;
            println!("indexed {n} roms");
        }
        Cmd::Summary => {
            let db = darkos_db::Db::open(&paths.db_path)?;
            for (sys, count, sz) in db.count_roms_by_system()? {
                println!("{sys:<20} {count:>6}  {}", bytesize::ByteSize(sz as u64));
            }
        }
        Cmd::Saves => {
            let p = darkos_saves::snapshot(
                PathBuf::from(&paths.roms_dir).as_path(),
                PathBuf::from(&paths.cache_dir).as_path(),
            )?;
            println!("snapshot at: {}", p.display());
        }
        Cmd::Firmware => {
            match darkos_firmware::current_firmware_string() {
                Ok(c) => println!("current: {c}"),
                Err(e) => println!("current: <unknown> ({e})"),
            }
            println!("known:");
            for fw in darkos_firmware::known_firmwares() {
                println!("- {} ({}) — {}", fw.name, fw.vendor, fw.variant_url);
            }
        }
        Cmd::Themes => {
            println!("installed:");
            for t in darkos_themes::list_installed()? {
                println!("  {t}");
            }
            println!("catalog:");
            for t in darkos_themes::catalog() {
                println!("  {} by {} — {}", t.name, t.author, t.source_url);
            }
        }
        Cmd::Update {
            apply,
            self_update,
            url,
            force,
            check,
        } => {
            if self_update || check {
                let current = env!("CARGO_PKG_VERSION");
                if check {
                    let m = darkos_update::fetch_manifest(url.as_deref())?;
                    let newer = darkos_update::is_newer(current, &m.version);
                    println!(
                        "current: {current}\nremote:  {} ({})\nupgrade: {}",
                        m.version,
                        m.binary_url,
                        if newer { "available" } else { "up to date" }
                    );
                } else {
                    let report =
                        darkos_update::run_self_update(current, url.as_deref(), None, force)?;
                    if report.upgraded {
                        println!(
                            "upgraded {} → {} at {}",
                            report.current,
                            report.remote.version,
                            report.installed_path.display()
                        );
                    } else {
                        println!(
                            "already up to date ({} == {})",
                            report.current, report.remote.version
                        );
                    }
                }
            } else {
                let n = darkos_update::apt_update_available()?;
                println!("{n} apt updates available");
                if apply {
                    let out = darkos_update::run_apt_upgrade(false)?;
                    println!("{out}");
                }
            }
        }
        Cmd::Perf { profile } => {
            let prof = match profile.as_str() {
                "powersave" => darkos_perf::Profile::PowerSave,
                "balanced" => darkos_perf::Profile::Balanced,
                "performance" => darkos_perf::Profile::Performance,
                other => anyhow::bail!("unknown profile: {other}"),
            };
            darkos_perf::apply_profile(prof)?;
            println!("perf profile applied: {profile}");
        }
        Cmd::Tui => darkos_tui::run()?,
        Cmd::Paths => println!("{}", serde_json::to_string_pretty(&paths)?),
        Cmd::DropCaches => {
            darkos_perf::drop_caches()?;
            println!("caches dropped");
        }
        Cmd::View { file } => {
            let src = match file {
                Some(p) => darkos_tui::viewer::ViewSource::from_file(&p)?,
                None => darkos_tui::viewer::ViewSource::from_stdin()?,
            };
            let snapshot_dir = PathBuf::from(&paths.cache_dir).join("snapshots");
            darkos_tui::viewer::view(src, Some(snapshot_dir))?;
        }
        Cmd::Panel => {
            let panel = darkos_hw::read_panel_compat().unwrap_or_else(|| "<unknown>".into());
            let res = darkos_hw::read_panel_resolution();
            let hw = darkos_hw::hardware_string().ok();
            println!("panel.compatible = {panel}");
            if let Some((w, h)) = res {
                println!("panel.resolution = {w}x{h}");
            }
            if let Some(h) = hw {
                println!("hardware_string  = {h}");
            }
        }
    }
    Ok(())
}
