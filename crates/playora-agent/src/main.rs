//! playora-agent — runs on the R36S, captures runtime data, syncs to server.

mod activity;
mod catalog;
mod cfg;
mod coolrom;
mod db;
mod download;
mod extract;
mod features;
mod hw;
mod kodi;
mod launcher;
mod myrient;
mod portmaster;
mod resources;
mod restore;
mod saves;
mod scanner;
mod selfupdate;
mod showlog;
mod sync;
mod tests;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use playora_common::AgentConfig;

pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "playora-agent", version, about = "Playora device agent")]
struct Cli {
    /// Path to agent.toml (overrides default discovery)
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create config + DB + register device
    Init {
        #[arg(long)]
        server_url: Option<String>,
        #[arg(long)]
        device_name: Option<String>,
    },
    /// Long-running loop: heartbeat + sync (foreground)
    Run,
    /// Diagnostic checks
    Doctor {
        #[arg(long)]
        interactive: bool,
    },
    /// Print current status (JSON)
    Status,
    /// Interactive TUI menu
    Tui {
        #[arg(long)]
        screen: Option<String>,
    },
    /// Hardware: snapshot | test | watch
    #[command(subcommand)]
    Hardware(HardwareCmd),
    /// Resource sampling
    #[command(subcommand)]
    Resources(ResourcesCmd),
    /// Scan roms paths
    Scan,
    /// Send pending events
    Sync,
    /// Send a single heartbeat
    Heartbeat,
    /// Fake game session (for QA)
    TestSession {
        #[arg(long)]
        system: String,
        #[arg(long)]
        game: String,
        #[arg(long, default_value_t = 5)]
        duration: u64,
    },
    /// Wrap an emulator command to record a real session
    Launcher {
        #[arg(long)]
        system: String,
        #[arg(long)]
        core: Option<String>,
        #[arg(long)]
        rom: String,
        /// Original emulator command (after `--`)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Catalog (legal only)
    #[command(subcommand)]
    Catalog(CatalogCmd),
    /// Feature flags
    #[command(subcommand)]
    Features(FeaturesCmd),
    /// Log viewer
    #[command(subcommand)]
    Logs(LogsCmd),
    /// Direct ROM download to a system folder
    Download {
        #[arg(long)]
        url: String,
        #[arg(long)]
        system: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        sha256: Option<String>,
        #[arg(long)]
        overwrite: bool,
    },
    /// List built-in ROM sources
    Sources,
    /// List supported systems with emulator + extensions
    Systems,
    /// CoolROM-style downloader (Rust port by Rodolfo Peixoto, 2026)
    #[command(subcommand)]
    Coolrom(CoolromCmd),
    /// Myrient directory crawler
    #[command(subcommand)]
    Myrient(MyrientCmd),
    /// Save files: pack tarball or upload to server
    #[command(subcommand)]
    Saves(SavesCmd),
    /// Self-update from GitHub release
    SelfUpdate {
        #[arg(long, default_value = "ropeixoto")]
        owner: String,
        #[arg(long, default_value = "playora")]
        repo: String,
    },
    /// PortMaster: list/search/install ports (Counter-Strike-like, Half-Life, etc.)
    #[command(subcommand)]
    Portmaster(PortmasterCmd),
    /// Extract `playora_restore.tar` from /roms/ into the system (one-time bootstrap)
    RestoreTar {
        #[arg(long)]
        keep_tar: bool,
    },
    /// Show a log file in a scrollable TUI (use as: ports script post-step)
    ShowLog { file: String },
    /// Kodi setup helper (list addons, recommend, install via JSON-RPC)
    #[command(subcommand)]
    Kodi(KodiCmd),
    /// Extract any archive (.zip .tar .tar.gz .tar.xz .tar.bz2 .7z .rar .gz .xz .bz2)
    Extract {
        file: String,
        #[arg(long)]
        dest: Option<String>,
        #[arg(long)]
        keep: bool,
    },
    /// Run a quick diagnostic + hardware snapshot + sync in one shot (background)
    QuickSync,
    /// Mark a script as started (POSTs an Activity event to the server right away)
    ActivityBegin { script: String },
    /// Mark a script as finished, with exit code
    ActivityEnd { script: String, exit_code: i32 },
}

#[derive(Subcommand)]
enum KodiCmd {
    /// Probe Kodi, list installed video add-ons, print recommendations
    Setup,
    /// Trigger add-on install in Kodi (must be in enabled repository)
    Install { addon_id: String },
}

#[derive(Subcommand)]
enum PortmasterCmd {
    /// Fetch live catalog and list titles
    List {
        #[arg(long)]
        ready_to_run_only: bool,
    },
    /// Search the catalog by title/genre
    Search { query: String },
    /// Install a port by its catalog name (e.g. `vvvvvv`)
    Install { name: String },
    /// Show already-installed ports
    Installed,
}

#[derive(Subcommand)]
enum CoolromCmd {
    /// List supported consoles
    Consoles,
    /// List ROMs for a console / starting letter
    Roms { console: String, letter: char },
    /// Download a ROM by its CoolROM url-path
    Download {
        url_path: String,
        #[arg(long)]
        dest: String,
    },
}

#[derive(Subcommand)]
enum MyrientCmd {
    Index { url: String },
    Search { url: String, query: String },
}

#[derive(Subcommand)]
enum SavesCmd {
    Pack {
        #[arg(long)]
        dest: Option<String>,
    },
    Upload,
}

#[derive(Subcommand)]
enum HardwareCmd {
    Snapshot {
        #[arg(long)]
        save: bool,
    },
    Test {
        #[arg(long, default_value = "quick")]
        mode: String,
        #[arg(long)]
        interactive: bool,
    },
    Watch {
        #[arg(long, default_value_t = 2)]
        interval_secs: u64,
    },
}

#[derive(Subcommand)]
enum ResourcesCmd {
    Sample,
    Watch {
        #[arg(long, default_value_t = 5)]
        interval_secs: u64,
    },
}

#[derive(Subcommand)]
enum CatalogCmd {
    List {
        #[arg(long)]
        interactive: bool,
    },
    Search {
        term: String,
    },
    Download {
        id: String,
    },
}

#[derive(Subcommand)]
enum FeaturesCmd {
    Fetch,
    Show,
}

#[derive(Subcommand)]
enum LogsCmd {
    Tail {
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let env_filter = std::env::var("PLAYORA_LOG").unwrap_or_else(|_| "playora_agent=info".into());
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init()
        .ok();

    match cli.cmd {
        Cmd::Init {
            server_url,
            device_name,
        } => cfg::cmd_init(cli.config.as_deref(), server_url, device_name),
        Cmd::Run => sync::cmd_run(load_cfg(cli.config.as_deref())?),
        Cmd::Doctor { interactive } => {
            tests::cmd_doctor(load_cfg(cli.config.as_deref())?, interactive)
        }
        Cmd::Status => sync::cmd_status(load_cfg(cli.config.as_deref())?),
        Cmd::Tui { screen } => tui::cmd_tui(load_cfg(cli.config.as_deref())?, screen),
        Cmd::Hardware(c) => match c {
            HardwareCmd::Snapshot { save } => {
                hw::cmd_snapshot(load_cfg(cli.config.as_deref())?, save)
            }
            HardwareCmd::Test { mode, interactive } => {
                tests::cmd_hardware_test(load_cfg(cli.config.as_deref())?, &mode, interactive)
            }
            HardwareCmd::Watch { interval_secs } => hw::cmd_watch(interval_secs),
        },
        Cmd::Resources(c) => match c {
            ResourcesCmd::Sample => resources::cmd_sample(load_cfg(cli.config.as_deref())?),
            ResourcesCmd::Watch { interval_secs } => resources::cmd_watch(interval_secs),
        },
        Cmd::Scan => scanner::cmd_scan(load_cfg(cli.config.as_deref())?),
        Cmd::Sync => sync::cmd_sync_once(load_cfg(cli.config.as_deref())?),
        Cmd::Heartbeat => sync::cmd_heartbeat(load_cfg(cli.config.as_deref())?),
        Cmd::TestSession {
            system,
            game,
            duration,
        } => tests::cmd_test_session(load_cfg(cli.config.as_deref())?, &system, &game, duration),
        Cmd::Launcher {
            system,
            core,
            rom,
            command,
        } => launcher::cmd_launch(
            load_cfg(cli.config.as_deref())?,
            &system,
            core.as_deref(),
            &rom,
            &command,
        ),
        Cmd::Catalog(c) => match c {
            CatalogCmd::List { interactive } => {
                catalog::cmd_list(load_cfg(cli.config.as_deref())?, interactive)
            }
            CatalogCmd::Search { term } => {
                catalog::cmd_search(load_cfg(cli.config.as_deref())?, &term)
            }
            CatalogCmd::Download { id } => {
                catalog::cmd_download(load_cfg(cli.config.as_deref())?, &id)
            }
        },
        Cmd::Features(c) => match c {
            FeaturesCmd::Fetch => features::cmd_fetch(load_cfg(cli.config.as_deref())?),
            FeaturesCmd::Show => features::cmd_show(load_cfg(cli.config.as_deref())?),
        },
        Cmd::Logs(c) => match c {
            LogsCmd::Tail { lines } => {
                let path = cfg::log_path();
                if !path.exists() {
                    println!("(no logs yet at {})", path.display());
                    return Ok(());
                }
                let txt = std::fs::read_to_string(&path)?;
                let v: Vec<&str> = txt.lines().collect();
                let start = v.len().saturating_sub(lines);
                for l in &v[start..] {
                    println!("{l}");
                }
                Ok(())
            }
        },
        Cmd::Download {
            url,
            system,
            name,
            sha256,
            overwrite,
        } => {
            let cfg = load_cfg(cli.config.as_deref())?;
            let req = download::DownloadRequest {
                url: &url,
                system_folder: &system,
                filename: name.as_deref(),
                expected_sha256: sha256.as_deref(),
                overwrite,
            };
            let out = download::fetch(&cfg, &req)?;
            println!(
                "saved {} ({} bytes) sha256={}",
                out.path.display(),
                out.bytes,
                out.sha256
            );
            Ok(())
        }
        Cmd::Sources => {
            for s in playora_common::sources::built_in() {
                println!("{:<14} {:<35} {}", s.id, s.name, s.base_url);
            }
            Ok(())
        }
        Cmd::Systems => {
            println!(
                "{:<14} {:<35} {:<14} {}",
                "FOLDER", "NAME", "EMULATOR", "EXTENSIONS"
            );
            for s in playora_common::systems::SYSTEMS {
                println!(
                    "{:<14} {:<35} {:<14} {}",
                    s.folder,
                    s.display_name,
                    s.default_emulator,
                    s.extensions.join(",")
                );
            }
            Ok(())
        }
        Cmd::Coolrom(c) => match c {
            CoolromCmd::Consoles => {
                for c in coolrom::list_consoles()? {
                    println!("{c}");
                }
                Ok(())
            }
            CoolromCmd::Roms { console, letter } => {
                for r in coolrom::list_roms(&console, letter)? {
                    println!("{:<60} {}", r.name, r.url_path);
                }
                Ok(())
            }
            CoolromCmd::Download { url_path, dest } => {
                let rom = coolrom::RomEntry {
                    name: "rom".into(),
                    url_path,
                };
                let path = coolrom::download(&rom, std::path::Path::new(&dest), |dl, total| {
                    let pct = total.map(|t| (dl as f64 / t as f64) * 100.0);
                    if let Some(p) = pct {
                        print!("\r{p:.1}% ({} bytes)", dl);
                    } else {
                        print!("\r{dl} bytes");
                    }
                    use std::io::Write as _;
                    std::io::stdout().flush().ok();
                })?;
                println!("\nsaved {}", path.display());
                Ok(())
            }
        },
        Cmd::Myrient(c) => match c {
            MyrientCmd::Index { url } => {
                for e in myrient::list_index(&url)? {
                    let kind = if e.is_dir { "dir" } else { "file" };
                    println!("{:<5} {}", kind, e.url);
                }
                Ok(())
            }
            MyrientCmd::Search { url, query } => {
                for e in myrient::search(&url, &query)? {
                    println!("{}", e.url);
                }
                Ok(())
            }
        },
        Cmd::Saves(c) => match c {
            SavesCmd::Pack { dest } => saves::cmd_pack(load_cfg(cli.config.as_deref())?, dest),
            SavesCmd::Upload => saves::cmd_upload(load_cfg(cli.config.as_deref())?),
        },
        Cmd::SelfUpdate { owner, repo } => {
            let s = selfupdate::run(&owner, &repo)?;
            println!("{s}");
            Ok(())
        }
        Cmd::Portmaster(c) => match c {
            PortmasterCmd::List { ready_to_run_only } => {
                let cat = portmaster::fetch_catalog()?;
                for p in &cat.ports {
                    if ready_to_run_only && !p.attr.rtr {
                        continue;
                    }
                    let req = if p.items.is_empty() { "" } else { " *" };
                    println!(
                        "{:<40} {:<28} {}{}",
                        truncate(&p.attr.title, 40),
                        p.name,
                        p.attr.genres.join(","),
                        req
                    );
                }
                println!("\n* requires game data files (place in /roms/ports/<name>/)");
                Ok(())
            }
            PortmasterCmd::Search { query } => {
                let cat = portmaster::fetch_catalog()?;
                for p in portmaster::search(&cat, &query) {
                    println!("{:<40} {}", truncate(&p.attr.title, 40), p.name);
                }
                Ok(())
            }
            PortmasterCmd::Install { name } => {
                let cat = portmaster::fetch_catalog()?;
                let entry = cat
                    .ports
                    .iter()
                    .find(|p| p.name == name || p.attr.title.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| anyhow::anyhow!("not found: {name}"))?;
                let report = portmaster::install(entry, |dl, total| {
                    let pct = total.map(|t| (dl as f64 / t as f64) * 100.0);
                    if let Some(p) = pct {
                        print!("\r{p:.1}% ({dl} bytes)");
                    } else {
                        print!("\r{dl} bytes");
                    }
                    use std::io::Write as _;
                    std::io::stdout().flush().ok();
                })?;
                println!(
                    "\ninstalled {} ({} files){}",
                    report.port_name,
                    report.installed_files,
                    if report.requires_data {
                        " — needs game data files"
                    } else {
                        ""
                    }
                );
                Ok(())
            }
            PortmasterCmd::Installed => {
                for p in portmaster::list_installed() {
                    println!("{p}");
                }
                Ok(())
            }
        },
        Cmd::RestoreTar { keep_tar } => restore::cmd(keep_tar),
        Cmd::ShowLog { file } => showlog::cmd(std::path::Path::new(&file)),
        Cmd::Kodi(c) => match c {
            KodiCmd::Setup => kodi::cmd_setup(),
            KodiCmd::Install { addon_id } => kodi::cmd_install(&addon_id),
        },
        Cmd::Extract { file, dest, keep } => extract::cmd(&file, dest.as_deref(), keep),
        Cmd::QuickSync => {
            let cfg = load_cfg(cli.config.as_deref())?;
            let _ = tests::cmd_doctor(cfg.clone(), false);
            let _ = hw::cmd_snapshot(cfg.clone(), true);
            let _ = sync::cmd_heartbeat(cfg.clone());
            let _ = sync::cmd_sync_once(cfg);
            Ok(())
        }
        Cmd::ActivityBegin { script } => {
            let cfg = load_cfg(cli.config.as_deref())?;
            activity::begin(&cfg, &script)
        }
        Cmd::ActivityEnd { script, exit_code } => {
            let cfg = load_cfg(cli.config.as_deref())?;
            activity::end(&cfg, &script, exit_code)
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

fn load_cfg(path: Option<&str>) -> Result<AgentConfig> {
    cfg::load(path)
}
