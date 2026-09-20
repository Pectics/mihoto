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
    use flate2::{write::GzEncoder, Compression};
    use tar::{Builder, Header};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

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

    fn asset(name: &str, url: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            browser_download_url: url.to_string(),
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        for (path, body) in entries {
            let mut header = Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, *body).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn stable_selection_handles_empty_and_invalid_release_sets() {
        assert!(latest_stable(&[]).is_none());
        assert!(latest_stable(&[release("not-semver", false, false)]).is_none());
        assert!(latest_stable(&[
            release("v2.0.0", false, true),
            release("v1.0.0", true, false),
        ])
        .is_none());
    }

    #[test]
    fn asset_and_checksum_validation_cover_exact_matching_rules() {
        let mut release = release("v1.0.0", false, false);
        release.assets = vec![asset("binary", "https://example.com/binary")];
        assert_eq!(find_asset(&release, "binary").unwrap().name, "binary");
        assert!(find_asset(&release, "missing").is_err());

        let uppercase = format!("{} *binary\n", "A".repeat(64));
        assert_eq!(
            checksum_for_asset(&uppercase, "binary").unwrap(),
            "a".repeat(64)
        );
        for sums in [
            format!("{}  binary\n{}  binary\n", "a".repeat(64), "b".repeat(64)),
            "abc  binary\n".to_string(),
            format!("{}  binary\n", "z".repeat(64)),
        ] {
            assert!(checksum_for_asset(&sums, "binary").is_err());
        }
    }

    #[test]
    fn checksum_verification_accepts_only_the_exact_archive_digest() {
        let body = b"release archive";
        let checksum = format!("{:x}", Sha256::digest(body));
        verify_checksum(&checksum, body).unwrap();
        assert!(verify_checksum(&"0".repeat(64), body).is_err());
    }

    #[test]
    fn extraction_requires_exactly_one_root_mihoto_file() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = extract_binary(&archive(&[("mihoto", b"binary")]), dir.path()).unwrap();
        assert_eq!(fs::read(candidate).unwrap(), b"binary");

        let empty = archive(&[]);
        assert!(extract_binary(&empty, dir.path()).is_err());
        assert!(extract_binary(&archive(&[("nested/mihoto", b"binary")]), dir.path()).is_err());
        assert!(extract_binary(
            &archive(&[("mihoto", b"one"), ("mihoto", b"two")]),
            dir.path()
        )
        .is_err());
        assert!(extract_binary(b"not gzip", dir.path()).is_err());
    }

    #[tokio::test]
    async fn asset_download_uses_release_url_and_reports_http_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/asset"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"asset bytes"))
            .expect(1)
            .mount(&server)
            .await;
        let downloaded = download_asset(
            &Client::new(),
            &asset("binary", &format!("{}/asset", server.uri())),
        )
        .await
        .unwrap();
        assert_eq!(downloaded, b"asset bytes");

        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(download_asset(
            &Client::new(),
            &asset("missing", &format!("{}/missing", server.uri())),
        )
        .await
        .is_err());
    }

    #[test]
    fn github_client_has_a_valid_default_configuration() {
        github_client().unwrap();
    }
}
