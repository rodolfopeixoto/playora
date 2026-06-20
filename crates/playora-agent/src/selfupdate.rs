use anyhow::Result;

pub fn run(repo_owner: &str, repo_name: &str) -> Result<String> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner(repo_owner)
        .repo_name(repo_name)
        .bin_name("playora-agent")
        .target("aarch64-unknown-linux-gnu")
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()?
        .update()?;
    Ok(format!("update result: {status:?}"))
}
