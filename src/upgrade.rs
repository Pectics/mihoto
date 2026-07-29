use anyhow::Result;
use colored::Colorize;
use self_update::cargo_crate_version;

/// Perform the upgrade to the latest version
pub async fn run_upgrade(no_confirm: bool, target: Option<String>) -> Result<()> {
    let prefix = "mihoto:";

    println!("{} Checking for mihoto updates...", prefix.cyan());

    let result = tokio::task::spawn_blocking(move || {
        let mut builder = self_update::backends::github::Update::configure();
        builder
            .repo_owner("Pectics")
            .repo_name("mihoto")
            .bin_name("mihoto")
            .show_download_progress(true)
            .show_output(true)
            .no_confirm(no_confirm)
            .current_version(cargo_crate_version!());

        // Override target if provided
        if let Some(target) = target {
            builder.target(&target);
        }

        builder.build()?.update()
    })
    .await?;

    match result {
        Ok(status) => {
            // Add newline to separate from self_update output
            println!();
            if status.updated() {
                println!(
                    "{} Updated to version {}",
                    prefix.green().bold(),
                    status.version().to_string().underline().green()
                );
                println!(
                    "{} Please restart mihoto for the new version to take effect",
                    prefix.yellow()
                );
            } else {
                println!(
                    "{} Already running the latest version ({})",
                    prefix.green(),
                    status.version().to_string().bold()
                );
            }
        }
        Err(e) if e.to_string().contains("permission") => {
            anyhow::bail!(
                "Permission denied. Ensure you have write access to the mihoto binary location."
            );
        }
        Err(e) if e.to_string().contains("network") || e.to_string().contains("connection") => {
            anyhow::bail!("Network error. Please check your internet connection and try again.");
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

/// Check if a new version is available without installing
pub async fn check_for_update() -> Result<Option<String>> {
    let prefix = "mihoto:";

    println!("{} Checking for available updates...", prefix.cyan());

    let result = tokio::task::spawn_blocking(move || {
        let releases = self_update::backends::github::ReleaseList::configure()
            .repo_owner("Pectics")
            .repo_name("mihoto")
            .build()?
            .fetch()?;

        if let Some(latest) = releases.first() {
            let current = cargo_crate_version!();
            if latest.version != current {
                return Ok(Some(latest.version.clone()));
            }
        }
        Ok(None)
    })
    .await?;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag_name: &str, prerelease: bool, draft: bool) -> Release {
        Release {
            tag_name: tag_name.to_string(),
            prerelease,
            draft,
            assets: Vec::new(),
        }
    }

    #[test]
    fn release_versions_normalize_v_prefix_and_preserve_prereleases() {
        assert_eq!(parse_release_version("v1.0.0").unwrap().to_string(), "1.0.0");
        assert_eq!(
            parse_release_version("1.0.0-rc.1").unwrap().to_string(),
            "1.0.0-rc.1"
        );
        assert!(parse_release_version("latest").is_err());
    }

    #[test]
    fn latest_stable_ignores_drafts_and_release_candidates() {
        let releases = vec![
            release("v1.1.0-rc.1", true, false),
            release("v1.0.1", false, false),
            release("v1.2.0", false, true),
            release("v1.0.0", false, false),
        ];

        assert_eq!(latest_stable(&releases).unwrap().tag_name, "v1.0.1");
    }

    #[test]
    fn checksum_lookup_requires_one_exact_asset_match() {
        let sums = concat!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  mihoto-v1.0.0-x86_64-unknown-linux-gnu.tar.gz\n",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  mihoto-v1.0.0-aarch64-unknown-linux-gnu.tar.gz\n",
        );
        assert_eq!(
            checksum_for_asset(sums, "mihoto-v1.0.0-x86_64-unknown-linux-gnu.tar.gz").unwrap(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(checksum_for_asset(sums, "mihoto-v1.0.0").is_err());
    }
}
