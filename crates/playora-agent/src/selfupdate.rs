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
    let status = builder.build()?.update()?;
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
