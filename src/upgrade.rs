use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use flate2::read::GzDecoder;
use reqwest::Client;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tar::Archive;
use tempfile::{NamedTempFile, TempDir};

const API_BASE: &str = "https://api.github.com/repos/Pectics/mihoto";
const CHECKSUMS_NAME: &str = "SHA256SUMS";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
    draft: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Perform an upgrade from a verified stable GitHub release.
pub async fn run_upgrade(no_confirm: bool, target: Option<String>) -> Result<()> {
    let prefix = "mihoto:";
    println!("{} Checking for Mihoto updates...", prefix.cyan());

    let client = github_client()?;
    let releases = fetch_releases(&client).await?;
    let Some(release) = latest_stable(&releases) else {
        bail!("no stable Mihoto release is available");
    };
    let current = parse_release_version(CURRENT_VERSION)?;
    let available = parse_release_version(&release.tag_name)?;
    if available <= current {
        println!(
            "{} Already running the latest stable version ({})",
            prefix.green(),
            CURRENT_VERSION.bold()
        );
        return Ok(());
    }

    if !no_confirm
        && !dialoguer::Confirm::new()
            .with_prompt(format!("Install Mihoto {}?", release.tag_name))
            .default(false)
            .interact()?
    {
        println!("{} Upgrade cancelled", prefix.yellow());
        return Ok(());
    }

    let target = target.unwrap_or_else(|| self_update::get_target().to_string());
    install_release(&client, release, &target).await?;
    println!(
        "{} Updated to version {}",
        prefix.green().bold(),
        release.tag_name.bold().green()
    );
    println!(
        "{} Please restart Mihoto for the new version to take effect",
        prefix.yellow()
    );
    Ok(())
}

/// Check whether a newer stable release exists without touching the local binary.
pub async fn check_for_update() -> Result<Option<String>> {
    let prefix = "mihoto:";
    println!("{} Checking for available updates...", prefix.cyan());

    let client = github_client()?;
    let releases = fetch_releases(&client).await?;
    let Some(release) = latest_stable(&releases) else {
        return Ok(None);
    };
    let current = parse_release_version(CURRENT_VERSION)?;
    let available = parse_release_version(&release.tag_name)?;
    Ok((available > current).then(|| release.tag_name.clone()))
}

fn github_client() -> Result<Client> {
    Client::builder()
        .user_agent(format!("mihoto/{CURRENT_VERSION}"))
        .build()
        .context("failed to build GitHub client")
}

async fn fetch_releases(client: &Client) -> Result<Vec<Release>> {
    client
        .get(format!("{API_BASE}/releases"))
        .send()
        .await
        .context("failed to request GitHub releases")?
        .error_for_status()
        .context("GitHub releases request failed")?
        .json()
        .await
        .context("failed to parse GitHub releases")
}

fn latest_stable(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = parse_release_version(&release.tag_name).ok()?;
            version.pre.is_empty().then_some((version, release))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, release)| release)
}

fn parse_release_version(tag: &str) -> Result<Version> {
    Version::parse(tag.trim_start_matches('v'))
        .with_context(|| format!("invalid Mihoto release version: {tag}"))
}

async fn install_release(client: &Client, release: &Release, target: &str) -> Result<()> {
    let archive_name = format!("mihoto-{}-{target}.tar.gz", release.tag_name);
    let archive_asset = find_asset(release, &archive_name)?;
    let checksums_asset = find_asset(release, CHECKSUMS_NAME)?;

    let checksums = download_asset(client, checksums_asset).await?;
    let expected_checksum = checksum_for_asset(
        std::str::from_utf8(&checksums).context("SHA256SUMS is not UTF-8")?,
        &archive_name,
    )?;
    let archive = download_asset(client, archive_asset).await?;
    verify_checksum(&expected_checksum, &archive)?;

    let extracted = TempDir::new().context("failed to create upgrade staging directory")?;
    let candidate = extract_binary(&archive, extracted.path())?;
    atomic_replace(&candidate)?;
    Ok(())
}

fn find_asset<'a>(release: &'a Release, name: &str) -> Result<&'a ReleaseAsset> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .with_context(|| format!("release {} does not contain {name}", release.tag_name))
}

async fn download_asset(client: &Client, asset: &ReleaseAsset) -> Result<Vec<u8>> {
    client
        .get(&asset.browser_download_url)
        .send()
        .await
        .with_context(|| format!("failed to download {}", asset.name))?
        .error_for_status()
        .with_context(|| format!("download failed for {}", asset.name))?
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .with_context(|| format!("failed to read {}", asset.name))
}

fn checksum_for_asset(checksums: &str, asset_name: &str) -> Result<String> {
    let matches = checksums
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let checksum = fields.next()?;
            let name = fields.next()?.trim_start_matches('*');
            (name == asset_name).then_some(checksum)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        bail!("SHA256SUMS must contain exactly one checksum for {asset_name}");
    }
    let checksum = matches[0];
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("SHA256SUMS contains an invalid checksum for {asset_name}");
    }
    Ok(checksum.to_ascii_lowercase())
}

fn verify_checksum(expected: &str, archive: &[u8]) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(archive));
    if actual != expected {
        bail!("release archive checksum verification failed");
    }
    Ok(())
}

fn extract_binary(archive: &[u8], destination: &Path) -> Result<PathBuf> {
    let decoder = GzDecoder::new(Cursor::new(archive));
    let mut archive = Archive::new(decoder);
    let mut binary = None;
    for entry in archive
        .entries()
        .context("failed to inspect release archive")?
    {
        let mut entry = entry.context("failed to read release archive entry")?;
        let path = entry.path().context("invalid release archive path")?;
        if path.as_ref() != Path::new("mihoto") || !entry.header().entry_type().is_file() {
            bail!("release archive must contain exactly one mihoto binary");
        }
        if binary.is_some() {
            bail!("release archive contains more than one mihoto binary");
        }
        let candidate = destination.join("mihoto");
        entry
            .unpack(&candidate)
            .context("failed to extract Mihoto binary")?;
        binary = Some(candidate);
    }
    binary.context("release archive did not contain Mihoto binary")
}

fn atomic_replace(candidate: &Path) -> Result<()> {
    let installed =
        std::env::current_exe().context("failed to locate the running Mihoto binary")?;
    let parent = installed
        .parent()
        .context("Mihoto binary has no parent directory")?;
    let staged = NamedTempFile::new_in(parent).context("failed to create atomic upgrade file")?;
    fs::copy(candidate, staged.path()).context("failed to stage upgraded Mihoto binary")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staged
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))
            .context("failed to set upgraded Mihoto permissions")?;
    }
    staged
        .as_file()
        .sync_all()
        .context("failed to sync upgraded Mihoto binary")?;

    let smoke = Command::new(staged.path())
        .arg("--version")
        .status()
        .context("failed to execute staged Mihoto binary")?;
    if !smoke.success() {
        bail!("staged Mihoto binary failed its version smoke test");
    }
    staged
        .persist(&installed)
        .map_err(|error| error.error)
        .context("failed to atomically replace Mihoto binary")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        assert_eq!(
            parse_release_version("v1.0.0").unwrap().to_string(),
            "1.0.0"
        );
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
            release("v9.0.0-rc.1", false, false),
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

    #[test]
    fn checksum_validation_handles_case_star_duplicates_and_bad_values() {
        let asset = "mihoto-v1.0.0-x86_64-unknown-linux-gnu.tar.gz";
        let upper = "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789";
        assert_eq!(
            checksum_for_asset(&format!("{upper}  *{asset}\n"), asset).unwrap(),
            upper.to_ascii_lowercase()
        );
        assert!(
            checksum_for_asset(&format!("{upper}  {asset}\n{upper}  {asset}\n"), asset).is_err()
        );
        assert!(checksum_for_asset(&format!("short  {asset}\n"), asset).is_err());
        assert!(checksum_for_asset(&format!("{}  {asset}\n", "z".repeat(64)), asset).is_err());
    }

    #[test]
    fn release_asset_lookup_and_archive_checksum_are_verified() {
        let asset = ReleaseAsset {
            name: "mihoto-v1.0.0-x86_64-unknown-linux-gnu.tar.gz".into(),
            browser_download_url: "https://example.test/mihoto.tar.gz".into(),
        };
        let release = Release {
            tag_name: "v1.0.0".into(),
            prerelease: false,
            draft: false,
            assets: vec![asset.clone()],
        };
        assert_eq!(find_asset(&release, &asset.name).unwrap().name, asset.name);
        assert!(find_asset(&release, CHECKSUMS_NAME).is_err());

        let archive = b"release archive";
        let expected = format!("{:x}", Sha256::digest(archive));
        assert!(verify_checksum(&expected, archive).is_ok());
        assert!(verify_checksum(&"0".repeat(64), archive).is_err());
    }

    fn gzip_archive(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, contents, directory) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(contents.len() as u64);
            if *directory {
                header.set_entry_type(tar::EntryType::Directory);
            }
            header.set_cksum();
            tar.append_data(&mut header, path, *contents).unwrap();
        }
        let tar_bytes = tar.into_inner().unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&tar_bytes).unwrap();
        gzip.finish().unwrap()
    }

    #[test]
    fn extract_binary_accepts_one_file_and_rejects_unsafe_archive_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let valid = gzip_archive(&[("mihoto", b"binary", false)]);
        let extracted = extract_binary(&valid, dir.path()).unwrap();
        assert_eq!(fs::read(extracted).unwrap(), b"binary");

        for archive in [
            gzip_archive(&[("other", b"binary", false)]),
            gzip_archive(&[("mihoto", b"", true)]),
            gzip_archive(&[("mihoto", b"one", false), ("mihoto", b"two", false)]),
            gzip_archive(&[]),
        ] {
            assert!(extract_binary(&archive, dir.path()).is_err());
        }
    }
}
