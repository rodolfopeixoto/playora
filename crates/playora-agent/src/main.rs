//! playora-agent — runs on the R36S, captures runtime data, syncs to server.

mod catalog;
mod cfg;
mod coolrom;
mod db;
mod download;
mod features;
mod hw;
mod launcher;
mod myrient;
mod resources;
mod saves;
mod scanner;
mod selfupdate;
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
    }
}

fn load_cfg(path: Option<&str>) -> Result<AgentConfig> {
    cfg::load(path)
}
