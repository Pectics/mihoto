use crate::infrastructure::download::{download_file, DETAIL_PREFIX};
use crate::infrastructure::filesystem::{create_parent_dir, extract_gzip};
use crate::infrastructure::mihomo_release as resolve_mihomo_bin;
use crate::infrastructure::systemctl::Systemctl;

use std::fs;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Result};
use colored::Colorize;
use reqwest::Client;
use tempfile::NamedTempFile;

use super::{BinaryPlan, Mihoto, StageStatus};

impl Mihoto {
    /// Stage 1 of the binary install: resolve the URL and download to a temp file.
    ///
    /// Skips if the binary exists and `force` is false. The returned [`BinaryPlan`] is
    /// handed to [`Mihoto::install_binary`] *after* every other download stage so that
    /// stopping the running mihomo service does not break the user's `https_proxy`
    /// while we still need to reach the network.
    pub async fn prepare_binary(
        &self,
        client: &Client,
        force: bool,
        arch_override: Option<&str>,
    ) -> Result<BinaryPlan> {
        let binary_exists = fs::metadata(&self.mihomo_target_binary_path).is_ok();
        if binary_exists && !force {
            return Ok(BinaryPlan::Skip(format!(
                "binary exists at {}",
                self.mihomo_target_binary_path
            )));
        }
        let binary_url = resolve_mihomo_bin::resolve_binary_url(
            client,
            &self.config,
            arch_override,
            DETAIL_PREFIX,
        )
        .await?;

        let temp_file = NamedTempFile::new()?;
        download_file(
            client,
            &binary_url,
            temp_file.path(),
            &self.config.mihoto_user_agent,
        )
        .await?;
        Ok(BinaryPlan::Install(temp_file))
    }

    /// Stage 2 of the binary install: stop the running service if any, then extract the
    /// downloaded binary into place and set its executable bit.
    ///
    /// Must run *after* every other network-dependent stage; see [`BinaryPlan`].
    pub async fn install_binary(&self, temp_file: NamedTempFile) -> Result<StageStatus> {
        let target = Path::new(&self.mihomo_target_binary_path);
        create_parent_dir(target)?;
        let parent = target.parent().ok_or_else(|| {
            anyhow!(
                "binary path has no parent: {}",
                self.mihomo_target_binary_path
            )
        })?;
        let staged = NamedTempFile::new_in(parent)?;
        extract_gzip(
            temp_file.path(),
            staged.path().to_string_lossy().as_ref(),
            DETAIL_PREFIX.cyan(),
        )?;
        staged
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
        staged.as_file().sync_all()?;

        // Stop mihomo.service before overwriting to avoid "Text file busy".
        let binary_exists = target.exists();
        if binary_exists {
            println!(
                "{} Stopping mihomo.service before overwriting binary...",
                DETAIL_PREFIX.cyan()
            );
            Systemctl::new().stop("mihomo.service").execute()?;
        }
        staged.persist(target).map_err(|error| error.error)?;
        Ok(StageStatus::Installed)
    }

    pub async fn update_core(
        &self,
        client: &Client,
        arch_override: Option<&str>,
    ) -> Result<StageStatus> {
        // Check if binary exists
        let binary_exists = fs::metadata(&self.mihomo_target_binary_path).is_ok();
        if !binary_exists {
            return Err(anyhow!(
                "Mihomo binary not found at {}. Run `mihoto init` first.",
                self.mihomo_target_binary_path
            ));
        }

        // Resolve binary URL (auto-detect from GitHub or use configured URL)
        let resolved_binary =
            resolve_mihomo_bin::resolve_binary(client, &self.config, arch_override, DETAIL_PREFIX)
                .await?;
        if let Some(latest_version) = resolved_binary.version.as_deref() {
            match installed_mihomo_version(&self.mihomo_target_binary_path) {
                Ok(Some(installed_version)) if installed_version == latest_version => {
                    println!(
                        "{} Mihomo core is already up to date ({})",
                        DETAIL_PREFIX.green(),
                        installed_version.bold()
                    );
                    return Ok(StageStatus::Skipped(format!(
                        "already at {installed_version}"
                    )));
                }
                Ok(Some(installed_version)) => {
                    println!(
                        "{} Updating mihomo core: {} -> {}",
                        DETAIL_PREFIX.cyan(),
                        installed_version.bold(),
                        latest_version.bold()
                    );
                }
                Ok(None) => {
                    println!(
                        "{} Could not detect installed mihomo version; downloading latest ({})",
                        DETAIL_PREFIX.yellow(),
                        latest_version.bold()
                    );
                }
                Err(err) => {
                    println!(
                        "{} Could not check installed mihomo version: {:#}",
                        DETAIL_PREFIX.yellow(),
                        err
                    );
                    println!(
                        "{} Downloading latest mihomo core ({})",
                        DETAIL_PREFIX.cyan(),
                        latest_version.bold()
                    );
                }
            }
        }

        // Create a temporary file for downloading
        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path();

        // Download mihomo binary first (before stopping service)
        download_file(
            client,
            &resolved_binary.url,
            temp_path,
            &self.config.mihoto_user_agent,
        )
        .await?;

        self.install_binary(temp_file).await
    }
}

fn installed_mihomo_version(binary_path: &str) -> Result<Option<String>> {
    let output = Command::new(binary_path)
        .arg("-v")
        .output()
        .map_err(|err| anyhow!("failed to run `{binary_path} -v`: {err}"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "`{} -v` exited with {}",
            binary_path,
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(extract_mihomo_version(&format!("{stdout}\n{stderr}")))
}

fn extract_mihomo_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find_map(normalize_mihomo_version_token)
}

fn normalize_mihomo_version_token(token: &str) -> Option<String> {
    let token = token.trim_matches(|c: char| {
        c == ',' || c == ';' || c == ':' || c == '(' || c == ')' || c == '[' || c == ']'
    });

    let is_stable_version = token
        .strip_prefix('v')
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_digit());

    let is_bare_stable_version =
        token.chars().next().is_some_and(|c| c.is_ascii_digit()) && token.contains('.');

    if is_stable_version || token.starts_with("alpha-") {
        Some(token.to_string())
    } else if is_bare_stable_version {
        Some(format!("v{token}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::Config;
    use flate2::{write::GzEncoder, Compression};
    use std::{fs, io::Write, os::unix::fs::PermissionsExt};
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn executable(dir: &Path, body: &str) -> String {
        let path = dir.join("mihomo");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn version_tokens_accept_supported_formats_and_reject_noise() {
        for (token, expected) in [
            ("v1.19.0", Some("v1.19.0")),
            ("1.19.0", Some("v1.19.0")),
            ("(alpha-abc123),", Some("alpha-abc123")),
            ("version", None),
            ("vnext", None),
            ("123", None),
        ] {
            assert_eq!(normalize_mihomo_version_token(token).as_deref(), expected);
        }
        assert_eq!(
            extract_mihomo_version("Mihomo Meta v1.19.0 linux amd64").as_deref(),
            Some("v1.19.0")
        );
        assert_eq!(extract_mihomo_version("no version here"), None);
    }

    #[test]
    fn installed_version_reads_stdout_and_stderr_and_reports_process_errors() {
        let dir = tempfile::tempdir().unwrap();
        let binary = executable(dir.path(), "printf 'Mihomo Meta 1.2.3\\n'");
        assert_eq!(
            installed_mihomo_version(&binary).unwrap().as_deref(),
            Some("v1.2.3")
        );

        let binary = executable(dir.path(), "printf 'alpha-dev' >&2");
        assert_eq!(
            installed_mihomo_version(&binary).unwrap().as_deref(),
            Some("alpha-dev")
        );

        let binary = executable(dir.path(), "exit 9");
        assert!(installed_mihomo_version(&binary).is_err());
        assert!(installed_mihomo_version(dir.path().join("missing").to_str().unwrap()).is_err());
    }

    #[tokio::test]
    async fn prepare_and_install_downloaded_binary_then_skip_existing_target() {
        let server = MockServer::start().await;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"downloaded binary").unwrap();
        let body = encoder.finish().unwrap();
        Mock::given(method("GET"))
            .and(path("/mihomo.gz"))
            .and(header("user-agent", "mihoto-test"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("bin/mihomo");
        let config = Config {
            mihomo_binary_path: target.to_string_lossy().into_owned(),
            remote_mihomo_binary_url: Some(format!("{}/mihomo.gz", server.uri())),
            mihoto_user_agent: "mihoto-test".to_string(),
            ..Config::default()
        };
        let mihoto = Mihoto::from_config(config);
        let plan = mihoto
            .prepare_binary(&Client::new(), false, Some("invalid-but-unused"))
            .await
            .unwrap();
        let BinaryPlan::Install(handle) = plan else {
            panic!("missing target should schedule installation");
        };
        assert!(matches!(
            mihoto.install_binary(handle).await.unwrap(),
            StageStatus::Installed
        ));
        assert_eq!(fs::read(&target).unwrap(), b"downloaded binary");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o755
        );

        assert!(matches!(
            mihoto.prepare_binary(&Client::new(), false, None).await.unwrap(),
            BinaryPlan::Skip(reason) if reason.contains(target.to_str().unwrap())
        ));
    }

    #[tokio::test]
    async fn update_core_requires_an_installed_binary_before_network_access() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("missing-mihomo");
        let config = Config {
            mihomo_binary_path: target.to_string_lossy().into_owned(),
            ..Config::default()
        };
        let error = Mihoto::from_config(config)
            .update_core(&Client::new(), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Run `mihoto init` first"));
    }
}
