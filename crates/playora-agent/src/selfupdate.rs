use anyhow::Result;

#[derive(Debug, Clone, Copy)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub fn from_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("beta") || s.eq_ignore_ascii_case("prerelease") {
            Self::Beta
        } else {
            Self::Stable
        }
    }
}

pub fn run(owner: &str, repo: &str) -> Result<String> {
    run_channel(owner, repo, Channel::Stable)
}

pub fn run_channel(owner: &str, repo: &str, channel: Channel) -> Result<String> {
    use crate::ttyui::{self, Status};
    ttyui::header("Update Agent");

    ttyui::section("Pre-flight");
    if !crate::sync::online() {
        ttyui::row("network", "no WiFi", Status::Fail);
        println!();
        println!("SUMMARY: Update skipped — connect WiFi and try again.");
        return Ok("offline".into());
    }
    ttyui::row("network", "connected", Status::Ok);
    ttyui::row("channel", &format!("{channel:?}"), Status::Info);
    ttyui::row("current version", env!("CARGO_PKG_VERSION"), Status::Info);

    ttyui::section("GitHub release lookup");
    let mut builder = self_update::backends::github::Update::configure();
    builder
        .repo_owner(owner)
        .repo_name(repo)
        .bin_name("playora-agent")
        .target("aarch64-unknown-linux-gnu")
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"));
    if matches!(channel, Channel::Beta) {
        builder.identifier("beta");
    }
    let updater = builder.build()?;
    let latest = updater.get_latest_release()?;
    ttyui::row("latest tag", &latest.version, Status::Info);
    if latest.version.trim_start_matches('v') == env!("CARGO_PKG_VERSION") {
        ttyui::ok("already up to date");
        println!();
        println!("SUMMARY: Update — already on {}", env!("CARGO_PKG_VERSION"));
        return Ok("up-to-date".into());
    }

    ttyui::section("Downloading + installing");
    let status = updater.update()?;
    ttyui::ok(&format!("installed {status:?}"));

    ttyui::section("Restarting agent service");
    let restart = std::process::Command::new("sudo")
        .args(["systemctl", "restart", "playora-agent.service"])
        .status();
    match restart {
        Ok(s) if s.success() => ttyui::ok("playora-agent.service restarted"),
        _ => ttyui::row(
            "systemctl restart",
            "skipped (not a systemd unit yet)",
            Status::Warn,
        ),
    }

    println!();
    println!("SUMMARY: Update channel={:?} result={status:?}", channel);
    Ok(format!("update channel={:?} result={status:?}", channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_parse() {
        assert!(matches!(Channel::from_str("stable"), Channel::Stable));
        assert!(matches!(Channel::from_str("beta"), Channel::Beta));
        assert!(matches!(Channel::from_str("PRERELEASE"), Channel::Beta));
        assert!(matches!(Channel::from_str("xyz"), Channel::Stable));
    }
}
