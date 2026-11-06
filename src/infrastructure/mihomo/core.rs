use crate::infrastructure::download::{download_file, DETAIL_PREFIX};
use crate::infrastructure::filesystem::{create_parent_dir, extract_gzip};
use crate::infrastructure::mihomo_release as resolve_mihomo_bin;
use crate::infrastructure::systemctl::Systemctl;

use std::fs;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use reqwest::Client;
use tempfile::NamedTempFile;

use super::{BinaryPlan, Mihoto, StageStatus, SERVICE_HEALTH_DELAY};

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

    /// Validate and replace the binary, recovering the old core and service on failure.
    /// Init preserves a stopped service until its service-setup stage.
    pub async fn install_binary(&self, temp_file: NamedTempFile) -> Result<StageStatus> {
        self.install_binary_with_program(
            temp_file,
            Path::new("systemctl"),
            SERVICE_HEALTH_DELAY,
            false,
        )
        .await
    }

    async fn install_binary_with_program(
        &self,
        temp_file: NamedTempFile,
        systemctl_program: &Path,
        health_delay: Duration,
        activate: bool,
    ) -> Result<StageStatus> {
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
        // Linux refuses to execute a file while a writable descriptor is still open.
        let staged = staged.into_temp_path();

        // Reject unusable cores before touching either the installed binary or service.
        installed_mihomo_version(staged.to_string_lossy().as_ref())?
            .context("staged Mihomo core did not report a version")?;
        let config_path = Path::new(&self.mihomo_target_config_path);
        if config_path.exists() || activate {
            self.validate_config_with_binary(&staged, config_path)?;
        }

        let backup = if target.exists() {
            let backup = NamedTempFile::new_in(parent)?;
            fs::copy(target, backup.path())?;
            backup.as_file().sync_all()?;
            Some(backup)
        } else {
            None
        };
        let was_active = backup.is_some()
            && Systemctl::is_active_with_program(systemctl_program, "mihomo.service");
        let outcome: Result<()> = async {
            if was_active || activate {
                Systemctl::with_program(systemctl_program)
                    .stop("mihomo.service")
                    .execute()?;
            }
            staged.persist(target).map_err(|error| error.error)?;
            if was_active || activate {
                self.restart_and_verify_with_program(systemctl_program, health_delay)
                    .await?;
            }
            Ok(())
        }
        .await;

        if let Err(install_error) = outcome {
            return match self
                .restore_binary(backup, systemctl_program, health_delay, was_active)
                .await
            {
                Ok(()) => Err(anyhow!(
                    "Mihomo core installation failed and was rolled back: {install_error:#}"
                )),
                Err(recovery_error) => Err(anyhow!(
                    "Mihomo core installation failed ({install_error:#}); rollback recovery also failed: {recovery_error:#}"
                )),
            };
        }
        Ok(StageStatus::Installed)
    }

    async fn restore_binary(
        &self,
        backup: Option<NamedTempFile>,
        systemctl_program: &Path,
        health_delay: Duration,
        was_active: bool,
    ) -> Result<()> {
        let stopped = Systemctl::with_program(systemctl_program)
            .stop("mihomo.service")
            .execute();
        let target = Path::new(&self.mihomo_target_binary_path);
        match backup {
            Some(backup) => {
                backup.persist(target).map_err(|error| {
                    let restore_error = error.error;
                    match error.file.keep() {
                        Ok((_file, path)) => anyhow!(
                            "failed to restore old core ({restore_error}); backup retained at {}",
                            path.display()
                        ),
                        Err(keep_error) => anyhow!(
                            "failed to restore old core ({restore_error}); failed to retain backup: {keep_error}"
                        ),
                    }
                })?;
            }
            None if target.exists() => fs::remove_file(target)?,
            None => {}
        }
        if was_active {
            Systemctl::with_program(systemctl_program)
                .reset_failed("mihomo.service")
                .execute()?;
            self.restart_and_verify_with_program(systemctl_program, health_delay)
                .await
        } else {
            stopped?;
            Ok(())
        }
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

        self.install_binary_with_program(
            temp_file,
            Path::new("systemctl"),
            SERVICE_HEALTH_DELAY,
            true,
        )
        .await
    }
}

fn installed_mihomo_version(binary_path: &str) -> Result<Option<String>> {
    let output = core_command_output(Command::new(binary_path).arg("-v"))
        .with_context(|| format!("failed to run `{binary_path} -v`"))?;

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

/// A concurrently forked process may briefly retain a writable descriptor until exec.
/// Retry only that transient Linux error; all other execution failures are immediate.
pub(super) fn core_command_output(command: &mut Command) -> std::io::Result<std::process::Output> {
    let mut retries = 4;
    loop {
        match command.output() {
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) && retries > 0 => {
                retries -= 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
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

        let mut busy = NamedTempFile::new_in(dir.path()).unwrap();
        busy.write_all(b"#!/bin/sh\necho 'Mihomo Meta v2.0.0'\n")
            .unwrap();
        busy.as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))
            .unwrap();
        let error = installed_mihomo_version(busy.path().to_str().unwrap()).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(libc::ETXTBSY)
        );
        let executable = busy.into_temp_path();
        assert_eq!(
            installed_mihomo_version(executable.to_str().unwrap())
                .unwrap()
                .as_deref(),
            Some("v2.0.0")
        );
    }

    #[tokio::test]
    async fn prepare_and_install_downloaded_binary_then_skip_existing_target() {
        let server = MockServer::start().await;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        let binary = b"#!/bin/sh\nprintf 'Mihomo Meta v2.0.0\\n'\n";
        encoder.write_all(binary).unwrap();
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
        assert_eq!(fs::read(&target).unwrap(), binary);
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

    fn archive(contents: &[u8]) -> NamedTempFile {
        let mut encoder = GzEncoder::new(NamedTempFile::new().unwrap(), Compression::default());
        encoder.write_all(contents).unwrap();
        encoder.finish().unwrap()
    }

    fn candidate(validation_exit: i32) -> Vec<u8> {
        format!(
            "#!/bin/sh\n# new core\ncase \"$1\" in\n-v) echo 'Mihomo Meta v2.0.0' ;;\n-t) test \"$2\" = -d && test \"$4\" = -f && test -f \"$5\" || exit 8; exit {validation_exit} ;;\n*) exit 9 ;;\nesac\n"
        )
        .into_bytes()
    }

    fn installation(dir: &Path, existing: bool) -> Mihoto {
        let config = Config {
            mihomo_binary_path: dir.join("mihomo").display().to_string(),
            mihomo_config_root: dir.display().to_string(),
            ..Config::default()
        };
        if existing {
            executable(dir, "# old core\necho 'Mihomo Meta v1.0.0'");
            fs::set_permissions(dir.join("mihomo"), fs::Permissions::from_mode(0o751)).unwrap();
        }
        fs::write(dir.join("config.yaml"), "rules: []\n").unwrap();
        Mihoto::from_config(config)
    }

    // State lives in each case's temp directory; no PATH or process-wide environment changes.
    fn service(dir: &Path, active: bool, failure: &str) -> std::path::PathBuf {
        let program = dir.join("systemctl");
        fs::write(
            &program,
            format!(
                r#"#!/bin/sh
cd '{}'
echo "$*" >> calls
count=$(cat restarts 2>/dev/null || echo 0)
new=false
grep -q 'new core' mihomo 2>/dev/null && new=true
case "$1" in
is-active)
    if [ "$count" = 0 ]; then {}; fi
    if $new && [ '{}' = health ]; then exit 3; fi
    exit 0 ;;
show)
    if $new && [ '{}' = loop ] && [ "$count" -gt 0 ]; then echo 1; else echo 0; fi ;;
stop)
    if [ '{}' = replace ]; then
        if [ -f mihomo ]; then rm mihomo; mkdir mihomo; fi
        exit 0
    fi
    [ '{}' != stop ] ;;
restart)
    echo $((count + 1)) > restarts
    if $new; then
        case '{}' in restart|recovery) exit 1 ;; esac
    fi ;;
reset-failed)
    [ '{}' != recovery ] ;;
*) exit 9 ;;
esac
"#,
                dir.display(),
                if active { "exit 0" } else { "exit 3" },
                failure,
                failure,
                failure,
                failure,
                failure,
                failure
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    #[tokio::test]
    async fn preflight_rejects_bad_archives_executables_versions_and_configs_without_mutation() {
        for (contents, compressed, has_config, expected) in [
            (b"invalid gzip".to_vec(), false, true, ""),
            (b"not executable".to_vec(), true, true, "failed to run"),
            (b"#!/bin/sh\nexit 4\n".to_vec(), true, true, "exited with"),
            (
                b"#!/bin/sh\necho unknown\n".to_vec(),
                true,
                true,
                "did not report a version",
            ),
            (candidate(5), true, true, "Mihomo rejected staged config"),
            (candidate(0), true, false, "Mihomo rejected staged config"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mihoto = installation(dir.path(), true);
            if !has_config {
                fs::remove_file(&mihoto.mihomo_target_config_path).unwrap();
            }
            let original = fs::read(&mihoto.mihomo_target_binary_path).unwrap();
            let program = service(dir.path(), true, "");
            let mut download = if compressed {
                archive(&contents)
            } else {
                NamedTempFile::new().unwrap()
            };
            if !compressed {
                download.write_all(&contents).unwrap();
            }
            let error = mihoto
                .install_binary_with_program(download, &program, Duration::ZERO, true)
                .await
                .unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
            assert_eq!(
                fs::read(&mihoto.mihomo_target_binary_path).unwrap(),
                original
            );
            assert!(!dir.path().join("calls").exists());
        }
    }

    #[tokio::test]
    async fn successful_install_respects_init_and_update_service_states() {
        for (existing, active, activate, restarted) in [
            (false, false, false, false),
            (true, false, false, false),
            (true, true, false, true),
            (true, false, true, true),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mihoto = installation(dir.path(), existing);
            let program = service(dir.path(), active, "");
            // A fresh init can stage a binary before its YAML exists.
            if !existing {
                fs::remove_file(&mihoto.mihomo_target_config_path).unwrap();
            }
            let bytes = candidate(0);
            mihoto
                .install_binary_with_program(archive(&bytes), &program, Duration::ZERO, activate)
                .await
                .unwrap();
            assert_eq!(fs::read(&mihoto.mihomo_target_binary_path).unwrap(), bytes);
            assert_eq!(
                fs::metadata(&mihoto.mihomo_target_binary_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
            let calls = fs::read_to_string(dir.path().join("calls")).unwrap_or_default();
            assert_eq!(calls.contains("restart mihomo.service"), restarted);
            assert!(!calls.contains("reset-failed"));
        }
    }

    #[tokio::test]
    async fn installation_failures_restore_binary_permissions_and_prior_service_state() {
        for (active, failure, recovery_failed) in [
            (true, "restart", false),
            (true, "health", false),
            (true, "loop", false),
            (false, "restart", false),
            (true, "stop", false),
            (false, "stop", true),
            (true, "recovery", true),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mihoto = installation(dir.path(), true);
            let original = fs::read(&mihoto.mihomo_target_binary_path).unwrap();
            let program = service(dir.path(), active, failure);
            let error = mihoto
                .install_binary_with_program(archive(&candidate(0)), &program, Duration::ZERO, true)
                .await
                .unwrap_err();
            assert_eq!(
                error.to_string().contains("recovery also failed"),
                recovery_failed,
                "{error:#}"
            );
            assert_eq!(
                fs::read(&mihoto.mihomo_target_binary_path).unwrap(),
                original
            );
            assert_eq!(
                fs::metadata(&mihoto.mihomo_target_binary_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o751
            );
            let calls = fs::read_to_string(dir.path().join("calls")).unwrap();
            assert_eq!(calls.contains("reset-failed mihomo.service"), active);
            if !active {
                assert_eq!(
                    calls.matches("restart mihomo.service").count(),
                    usize::from(failure != "stop")
                );
                assert!(calls.ends_with("stop mihomo.service\n"));
            }
        }
    }

    #[tokio::test]
    async fn failed_binary_restore_retains_old_core_for_manual_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let mihoto = installation(dir.path(), true);
        let original = fs::read(&mihoto.mihomo_target_binary_path).unwrap();
        let program = service(dir.path(), true, "replace");
        let error = mihoto
            .install_binary_with_program(archive(&candidate(0)), &program, Duration::ZERO, true)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("backup retained at"),
            "{error:#}"
        );
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(backups[0].path()).unwrap(), original);
    }

    #[tokio::test]
    async fn restoring_without_a_previous_binary_cleans_only_the_new_binary() {
        for exists in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mihoto = installation(dir.path(), exists);
            let program = service(dir.path(), false, "");
            mihoto
                .restore_binary(None, &program, Duration::ZERO, false)
                .await
                .unwrap();
            assert!(!Path::new(&mihoto.mihomo_target_binary_path).exists());
            assert_eq!(
                fs::read_to_string(&mihoto.mihomo_target_config_path).unwrap(),
                "rules: []\n"
            );
        }
    }
}
