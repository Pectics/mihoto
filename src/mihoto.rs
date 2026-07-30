use crate::config::{apply_mihomo_override, parse_config, Config};
use crate::resolve_mihomo_bin;
use crate::systemctl::Systemctl;
use crate::timer;
use crate::ui::{install_ui, resolve_external_ui_path};
use crate::utils::{
    create_parent_dir, delete_file, download_file, extract_gzip, systemd_escape_exec_arg,
    try_decode_base64_file_inplace, DETAIL_PREFIX,
};

use anyhow::Error;

use std::fs;
use std::io::Write;
use std::os::unix::prelude::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Result};
use colored::Colorize;
use reqwest::Client;
use tempfile::NamedTempFile;

#[derive(Debug)]
pub struct Mihoto {
    // global mihoto config
    pub prefix: String,
    pub config: Config,

    // mihomo global variables derived from mihoto config
    pub mihomo_target_binary_path: String,
    pub mihomo_target_config_root: String,
    pub mihomo_target_config_path: String,
    pub mihomo_target_service_path: String,
}

/// Outcome of a single setup stage, used by `mihoto init`.
pub enum StageStatus {
    Installed,
    Skipped(String),
    Failed(Error),
}

/// Plan returned by [`Mihoto::prepare_binary`]: either we already have the binary and
/// nothing needs swapping, or we downloaded a new one to a temp file that the install
/// step must consume.
///
/// The split exists so the network-killing `Systemctl::stop` happens only after every
/// other download stage has finished - otherwise the still-running mihomo proxy gets
/// torn down mid-init and subsequent reqwest calls hit `Connection refused` against
/// the configured `https_proxy`.
pub enum BinaryPlan {
    Skip(String),
    Install(NamedTempFile),
}

impl Mihoto {
    pub fn new(config_path: &str) -> Result<Mihoto> {
        let config = parse_config(config_path)?;
        Ok(Self::from_config(config))
    }

    /// Build a `Mihoto` from an already-validated `Config`.
    pub fn from_config(config: Config) -> Mihoto {
        Mihoto {
            prefix: String::from("mihoto:"),
            mihomo_target_binary_path: config.mihomo_binary_path.clone(),
            mihomo_target_config_root: config.mihomo_config_root.clone(),
            mihomo_target_config_path: format!("{}/config.yaml", config.mihomo_config_root),
            mihomo_target_service_path: String::from("/etc/systemd/system/mihomo.service"),
            config,
        }
    }

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

    fn ensure_config_root_secure(&self) -> Result<()> {
        fs::create_dir_all(&self.mihomo_target_config_root)?;
        fs::set_permissions(
            &self.mihomo_target_config_root,
            fs::Permissions::from_mode(0o750),
        )?;
        Ok(())
    }

    fn apply_existing_config_atomically(&self) -> Result<bool> {
        self.ensure_config_root_secure()?;
        let target = Path::new(&self.mihomo_target_config_path);
        let staged = NamedTempFile::new_in(&self.mihomo_target_config_root)?;
        fs::copy(target, staged.path())?;
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o600))?;
        let changed = apply_mihomo_override(
            staged.path().to_string_lossy().as_ref(),
            &self.config.mihomo_config,
        )?;
        if changed {
            staged.as_file().sync_all()?;
            staged.persist(target).map_err(|error| error.error)?;
        } else {
            fs::set_permissions(target, fs::Permissions::from_mode(0o600))?;
        }
        Ok(changed)
    }

    async fn download_and_install_config(&self, client: &Client) -> Result<()> {
        self.ensure_config_root_secure()?;
        let target = Path::new(&self.mihomo_target_config_path);
        let staged = NamedTempFile::new_in(&self.mihomo_target_config_root)?;
        download_file(
            client,
            &self.config.remote_config_url,
            staged.path(),
            &self.config.mihoto_user_agent,
        )
        .await?;
        let staged_path = staged.path().to_string_lossy();
        try_decode_base64_file_inplace(staged_path.as_ref())?;
        apply_mihomo_override(staged_path.as_ref(), &self.config.mihomo_config)?;
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o600))?;
        staged.as_file().sync_all()?;
        staged.persist(target).map_err(|error| error.error)?;
        Ok(())
    }

    /// Download remote config YAML and apply TOML overrides.
    /// If the config file already exists and `force` is false, only re-applies overrides.
    pub async fn ensure_remote_config(&self, client: &Client, force: bool) -> Result<StageStatus> {
        let config_path = Path::new(&self.mihomo_target_config_path);
        if !force && config_path.exists() {
            // Re-apply TOML overrides onto the cached YAML so user changes take effect.
            let changed = self.apply_existing_config_atomically()?;
            return if changed {
                Ok(StageStatus::Installed)
            } else {
                Ok(StageStatus::Skipped("config already current".to_string()))
            };
        }

        self.download_and_install_config(client).await?;
        Ok(StageStatus::Installed)
    }

    /// Download geodata.  Skips files that already exist (unless `force`).
    pub async fn ensure_geodata(&self, client: &Client, force: bool) -> Result<StageStatus> {
        let Some(ref geox_url) = self.config.mihomo_config.geox_url else {
            return Ok(StageStatus::Skipped("geox_url not configured".to_string()));
        };

        let geodata_mode = self.config.mihomo_config.geodata_mode.unwrap_or(false);
        let config_root = Path::new(&self.mihomo_target_config_root);

        if geodata_mode {
            let geoip_path = config_root.join("geoip.dat");
            let geosite_path = config_root.join("geosite.dat");
            if !force && geoip_path.exists() && geosite_path.exists() {
                return Ok(StageStatus::Skipped("geodata present".to_string()));
            }
            if force || !geoip_path.exists() {
                download_file(
                    client,
                    &geox_url.geoip,
                    &geoip_path,
                    &self.config.mihoto_user_agent,
                )
                .await?;
            }
            if force || !geosite_path.exists() {
                download_file(
                    client,
                    &geox_url.geosite,
                    &geosite_path,
                    &self.config.mihoto_user_agent,
                )
                .await?;
            }
        } else {
            let mmdb_path = config_root.join("country.mmdb");
            if !force && mmdb_path.exists() {
                return Ok(StageStatus::Skipped("geodata present".to_string()));
            }
            download_file(
                client,
                &geox_url.mmdb,
                &mmdb_path,
                &self.config.mihoto_user_agent,
            )
            .await?;
        }

        Ok(StageStatus::Installed)
    }

    /// Install the web dashboard.  Skips if the target directory already has an `index.html`
    /// (unless `force`).
    pub async fn ensure_ui(&self, client: &Client, force: bool) -> Result<StageStatus> {
        let Some(ui) = self.config.ui.as_ref() else {
            return Ok(StageStatus::Skipped("UI management disabled".to_string()));
        };
        let Some(target_dir) = self.external_ui_target_dir() else {
            return Ok(StageStatus::Skipped("`external_ui` path unset".to_string()));
        };
        if !force && target_dir.join("index.html").exists() {
            return Ok(StageStatus::Skipped(format!(
                "{} already installed",
                ui.as_config_value()
            )));
        }
        install_ui(
            client,
            ui,
            &target_dir,
            &self.config.mihoto_user_agent,
            DETAIL_PREFIX.cyan(),
        )
        .await?;
        Ok(StageStatus::Installed)
    }

    /// Write the systemd unit file.  Skips if the file already exists with identical content.
    pub async fn ensure_service(&self) -> Result<StageStatus> {
        let service_content = render_service_string(
            &self.mihomo_target_binary_path,
            &self.mihomo_target_config_root,
        );
        if let Ok(existing) = fs::read_to_string(&self.mihomo_target_service_path) {
            if existing == service_content {
                fs::set_permissions(
                    &self.mihomo_target_service_path,
                    fs::Permissions::from_mode(0o644),
                )?;
                return Ok(StageStatus::Skipped("service file unchanged".to_string()));
            }
        }
        let service_path = Path::new(&self.mihomo_target_service_path);
        create_parent_dir(service_path)?;
        let parent = service_path.parent().ok_or_else(|| {
            anyhow!(
                "service path has no parent: {}",
                self.mihomo_target_service_path
            )
        })?;
        let mut staged = NamedTempFile::new_in(parent)?;
        staged.write_all(service_content.as_bytes())?;
        staged
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))?;
        staged.as_file().sync_all()?;
        staged.persist(service_path).map_err(|error| error.error)?;
        Systemctl::new().daemon_reload().execute()?;
        println!(
            "{} Created mihomo.service at {}",
            DETAIL_PREFIX.cyan(),
            self.mihomo_target_service_path.underline().yellow()
        );
        Ok(StageStatus::Installed)
    }

    /// Enable and start mihomo.service, ensuring both autostart and current-session state.
    ///
    /// Always enables mihomo.service so it survives reboots, even if it was already running but
    /// not enabled (e.g. started manually after a previous failed init).
    pub async fn ensure_service_running(&self) -> Result<StageStatus> {
        let is_active = Systemctl::is_active("mihomo.service");
        let is_enabled = Systemctl::is_enabled("mihomo.service");

        if is_active && is_enabled {
            return Ok(StageStatus::Skipped(
                "already running and enabled".to_string(),
            ));
        }

        if !is_enabled {
            Systemctl::new().enable("mihomo.service").execute()?;
        }
        if !is_active {
            Systemctl::new().start("mihomo.service").execute()?;
        }
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

    pub async fn update_config(&self, client: &Client) -> Result<StageStatus> {
        self.download_and_install_config(client).await?;
        println!(
            "{} Updated and applied config overrides",
            DETAIL_PREFIX.cyan()
        );
        Ok(StageStatus::Installed)
    }

    pub async fn update_geodata(&self, client: &Client) -> Result<StageStatus> {
        if let Some(geox_url) = self.config.mihomo_config.geox_url.clone() {
            // Download geodata files based on `geodata_mode`
            let geodata_mode = self.config.mihomo_config.geodata_mode.unwrap_or(false);
            if geodata_mode {
                download_file(
                    client,
                    &geox_url.geoip,
                    &Path::new(&self.mihomo_target_config_root).join("geoip.dat"),
                    &self.config.mihoto_user_agent,
                )
                .await?;
                download_file(
                    client,
                    &geox_url.geosite,
                    &Path::new(&self.mihomo_target_config_root).join("geosite.dat"),
                    &self.config.mihoto_user_agent,
                )
                .await?;
            } else {
                download_file(
                    client,
                    &geox_url.mmdb,
                    &Path::new(&self.mihomo_target_config_root).join("country.mmdb"),
                    &self.config.mihoto_user_agent,
                )
                .await?;
            }

            println!("{} Downloaded and updated geodata", DETAIL_PREFIX.cyan());
        } else {
            return Ok(StageStatus::Skipped("`geox_url` undefined".to_string()));
        }
        Ok(StageStatus::Installed)
    }

    pub async fn update_ui(&self, client: &Client) -> Result<StageStatus> {
        let Some(ui) = self.config.ui.as_ref() else {
            return Ok(StageStatus::Skipped("UI management disabled".to_string()));
        };

        let Some(target_dir) = self.external_ui_target_dir() else {
            return Ok(StageStatus::Skipped("`external_ui` undefined".to_string()));
        };

        install_ui(
            client,
            ui,
            &target_dir,
            &self.config.mihoto_user_agent,
            DETAIL_PREFIX.cyan(),
        )
        .await?;
        Ok(StageStatus::Installed)
    }

    pub async fn restart_service(&self) -> Result<StageStatus> {
        println!("{} Restarting mihomo.service...", DETAIL_PREFIX.cyan());
        Systemctl::new().restart("mihomo.service").execute()?;
        Ok(StageStatus::Installed)
    }

    pub async fn apply(&self) -> Result<()> {
        // Apply mihomo config override
        self.apply_existing_config_atomically()?;
        println!(
            "{} Applied mihomo config overrides",
            self.prefix.green().bold()
        );

        // Restart mihomo systemd service
        Systemctl::new()
            .restart("mihomo.service")
            .execute()
            .map(|_| {
                println!("{} Restarted mihomo.service", self.prefix.green().bold());
            })?;
        Ok(())
    }

    pub fn uninstall(&self, purge: bool, manager_config_path: &Path) -> Result<()> {
        self.uninstall_with_paths(
            purge,
            manager_config_path,
            Path::new(timer::SERVICE_PATH),
            Path::new(timer::TIMER_PATH),
            Path::new("systemctl"),
        )
    }

    fn uninstall_with_paths(
        &self,
        purge: bool,
        manager_config_path: &Path,
        timer_service_path: &Path,
        timer_path: &Path,
        systemctl_program: &Path,
    ) -> Result<()> {
        if purge {
            for (label, path) in [
                ("manager config", manager_config_path),
                ("mihoto binary", Path::new(&self.config.mihoto_binary_path)),
                ("mihomo binary", Path::new(&self.mihomo_target_binary_path)),
                (
                    "mihomo config root",
                    Path::new(&self.mihomo_target_config_root),
                ),
            ] {
                validate_safe_purge_target(label, path)?;
            }
        }

        timer::disable_with_paths(timer_service_path, timer_path, systemctl_program)?;

        let service_path = Path::new(&self.mihomo_target_service_path);
        let had_service = service_path.exists();
        let service_known = Systemctl::is_active_with_program(systemctl_program, "mihomo.service")
            || Systemctl::is_enabled_with_program(systemctl_program, "mihomo.service");
        if service_known {
            Systemctl::with_program(systemctl_program)
                .disable_now("mihomo.service")
                .execute()?;
        }
        if had_service {
            delete_file(&self.mihomo_target_service_path, self.prefix.cyan())?;
        }
        if had_service || service_known {
            Systemctl::with_program(systemctl_program)
                .daemon_reload()
                .execute()?;
        }

        if purge {
            if Path::new(&self.mihomo_target_config_root).exists() {
                fs::remove_dir_all(&self.mihomo_target_config_root)?;
            }
            delete_file(&self.mihomo_target_binary_path, self.prefix.cyan())?;
            delete_file(&self.config.mihoto_binary_path, self.prefix.cyan())?;
            delete_file(
                manager_config_path.to_string_lossy().as_ref(),
                self.prefix.cyan(),
            )?;
        }

        println!(
            "{} Disabled and reloaded systemd services",
            self.prefix.green()
        );

        if !purge {
            println!(
                "{} Configuration and Mihomo binary retained; use --purge to remove them",
                self.prefix.yellow()
            );
        }
        Ok(())
    }

    fn external_ui_target_dir(&self) -> Option<PathBuf> {
        self.config
            .mihomo_config
            .external_ui
            .as_deref()
            .map(|external_ui| {
                resolve_external_ui_path(&self.mihomo_target_config_root, external_ui)
            })
    }
}

fn validate_safe_purge_target(label: &str, path: &Path) -> Result<()> {
    if !path.is_absolute() {
        anyhow::bail!("{label} purge target must be absolute: {}", path.display());
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        anyhow::bail!(
            "{label} purge target must not contain relative components: {}",
            path.display()
        );
    }
    for forbidden in ["/", "/etc", "/usr", "/usr/local", "/usr/local/bin"] {
        if path == Path::new(forbidden) {
            anyhow::bail!(
                "refusing dangerous {label} purge target: {}",
                path.display()
            );
        }
    }
    let expected_prefix = match label {
        "manager config" | "mihoto binary" => "mihoto",
        "mihomo binary" | "mihomo config root" => "mihomo",
        _ => anyhow::bail!("unknown purge target kind: {label}"),
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid {label} purge target: {}", path.display()))?;
    if !name.starts_with(expected_prefix) {
        anyhow::bail!(
            "refusing {label} purge target that does not look mihoto-managed: {}",
            path.display()
        );
    }
    Ok(())
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

/// Render the systemd unit file content for mihomo.service.
///
/// Reference: https://wiki.metacubex.one/startup/service/
fn render_service_string(binary_path: &str, config_root: &str) -> String {
    format!(
        "[Unit]
Description=mihomo Daemon, Another Clash Kernel.
Wants=network-online.target
After=network-online.target NetworkManager.service systemd-networkd.service iwd.service

[Service]
Type=simple
User=root
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW
LimitNPROC=4096
LimitNOFILE=65536
Restart=always
ExecStartPre=/usr/bin/sleep 1s
ExecStart={} -d {}
ExecReload=/bin/kill -HUP $MAINPID

[Install]
WantedBy=multi-user.target",
        systemd_escape_exec_arg(binary_path),
        systemd_escape_exec_arg(config_root)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn fake_systemctl(dir: &Path, query_exit: i32) -> PathBuf {
        let program = dir.join("systemctl");
        let log = dir.join("systemctl.log");
        fs::write(
			&program,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  is-active|is-enabled) exit {query_exit} ;;\n  *) exit 0 ;;\nesac\n",
				log.display()
			),
		)
		.unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    fn fake_mihomo(dir: &Path, validation_exit: i32) -> PathBuf {
        let program = dir.join("mihomo");
        let log = dir.join("mihomo.log");
        fs::write(
			&program,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  -t) exit {validation_exit} ;;\n  *) exit 0 ;;\nesac\n",
				log.display()
			),
		)
		.unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    #[test]
    fn system_service_has_tun_capabilities() {
        let unit = render_service_string("/usr/local/bin/mihomo", "/etc/mihomo");
        for value in [
            "User=root",
            "Wants=network-online.target",
            "After=network-online.target",
            "CapabilityBoundingSet=CAP_NET_ADMIN",
            "AmbientCapabilities=CAP_NET_ADMIN",
            "ExecStart=/usr/local/bin/mihomo -d /etc/mihomo",
            "ExecReload=",
            "WantedBy=multi-user.target",
        ] {
            assert!(unit.contains(value));
        }
        for forbidden in [
            format!("{}{}", "--", "user"),
            format!("{}{}", "default", ".target"),
            format!("{}/{}", "systemd", "user"),
            format!("{}/{}", "~", ".config"),
        ] {
            assert!(!unit.contains(&forbidden));
        }

        let escaped = render_service_string("/opt/mihomo core", "/etc/mihomo config");
        assert!(escaped.contains("ExecStart=\"/opt/mihomo core\" -d \"/etc/mihomo config\""));
    }

    #[tokio::test]
    async fn invalid_remote_config_does_not_replace_existing_config() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/config.yaml"))
            .respond_with(ResponseTemplate::new(200).set_body_string("tun: [invalid"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let original = "tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n";
        let config_path = dir.path().join("config.yaml");
        fs::write(&config_path, original).unwrap();

        let config = Config {
            remote_config_url: format!("{}/config.yaml", server.uri()),
            mihomo_config_root: dir.path().display().to_string(),
            ..Config::default()
        };
        let mihoto = Mihoto::from_config(config);
        assert!(mihoto.update_config(&Client::new()).await.is_err());
        assert_eq!(fs::read_to_string(config_path).unwrap(), original);
    }

    #[tokio::test]
    async fn semantically_invalid_remote_config_does_not_replace_existing_config() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/config.yaml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "proxies:\n  - name: broken\n    type: invalid\nrules:\n  - MATCH,broken\n",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let original = "tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n";
        let config_path = dir.path().join("config.yaml");
        fs::write(&config_path, original).unwrap();
        let mihomo_binary = fake_mihomo(dir.path(), 1);

        let config = Config {
            remote_config_url: format!("{}/config.yaml", server.uri()),
            mihomo_binary_path: mihomo_binary.display().to_string(),
            mihomo_config_root: dir.path().display().to_string(),
            ..Config::default()
        };
        let mihoto = Mihoto::from_config(config);

        assert!(mihoto.update_config(&Client::new()).await.is_err());
        assert_eq!(fs::read_to_string(config_path).unwrap(), original);
        let calls = fs::read_to_string(dir.path().join("mihomo.log")).unwrap();
        assert!(calls.contains("-t"));
        assert!(calls.contains("-d"));
        assert!(calls.contains("-f"));
    }

    #[tokio::test]
    async fn updated_remote_config_is_private() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/config.yaml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n"),
            )
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let mihomo_binary = fake_mihomo(dir.path(), 0);
        let config = Config {
            remote_config_url: format!("{}/config.yaml", server.uri()),
            mihomo_binary_path: mihomo_binary.display().to_string(),
            mihomo_config_root: dir.path().display().to_string(),
            ..Config::default()
        };
        let mihoto = Mihoto::from_config(config);
        mihoto.update_config(&Client::new()).await.unwrap();
        let mode = fs::metadata(dir.path().join("config.yaml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn semantically_invalid_override_does_not_replace_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let original = "tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n";
        let config_path = dir.path().join("config.yaml");
        fs::write(&config_path, original).unwrap();
        let mihomo_binary = fake_mihomo(dir.path(), 1);
        let mut config = Config::default();
        config.mihomo_binary_path = mihomo_binary.display().to_string();
        config.mihomo_config_root = dir.path().display().to_string();
        config.mihomo_config.mixed_port = Some(17890);
        let mihoto = Mihoto::from_config(config);

        assert!(mihoto.apply_existing_config_atomically().is_err());
        assert_eq!(fs::read_to_string(config_path).unwrap(), original);
    }

    #[tokio::test]
    async fn invalid_gzip_does_not_leave_a_binary() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("mihomo");
        let config = Config {
            mihomo_binary_path: target.display().to_string(),
            mihomo_config_root: dir.path().join("config").display().to_string(),
            ..Config::default()
        };
        let mihoto = Mihoto::from_config(config);
        let mut staged = NamedTempFile::new().unwrap();
        staged.write_all(b"not a gzip stream").unwrap();

        assert!(mihoto.install_binary(staged).await.is_err());
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn unchanged_service_has_its_permissions_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let service_path = dir.path().join("mihomo.service");
        let config = Config {
            mihomo_binary_path: "/usr/local/bin/mihomo".into(),
            mihomo_config_root: "/etc/mihomo".into(),
            ..Config::default()
        };
        let mut mihoto = Mihoto::from_config(config);
        mihoto.mihomo_target_service_path = service_path.display().to_string();
        fs::write(
            &service_path,
            render_service_string(
                &mihoto.mihomo_target_binary_path,
                &mihoto.mihomo_target_config_root,
            ),
        )
        .unwrap();
        fs::set_permissions(&service_path, fs::Permissions::from_mode(0o600)).unwrap();

        assert!(matches!(
            mihoto.ensure_service().await.unwrap(),
            StageStatus::Skipped(_)
        ));
        assert_eq!(
            fs::metadata(service_path).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn uninstall_disables_timer_first_and_retains_data_without_purge() {
        let dir = tempfile::tempdir().unwrap();
        let manager_config = dir.path().join("mihoto.toml");
        let config_root = dir.path().join("mihomo");
        let mihomo_binary = dir.path().join("mihomo-bin");
        let mihoto_binary = dir.path().join("mihoto-bin");
        let service = dir.path().join("mihomo.service");
        let timer_service = dir.path().join("mihoto-update.service");
        let timer = dir.path().join("mihoto-update.timer");
        fs::create_dir(&config_root).unwrap();
        for path in [
            &manager_config,
            &mihomo_binary,
            &mihoto_binary,
            &service,
            &timer_service,
            &timer,
        ] {
            fs::write(path, "fixture").unwrap();
        }
        let systemctl = fake_systemctl(dir.path(), 0);
        let config = Config {
            mihoto_binary_path: mihoto_binary.display().to_string(),
            mihomo_binary_path: mihomo_binary.display().to_string(),
            mihomo_config_root: config_root.display().to_string(),
            ..Config::default()
        };
        let mut mihoto = Mihoto::from_config(config);
        mihoto.mihomo_target_service_path = service.display().to_string();

        mihoto
            .uninstall_with_paths(false, &manager_config, &timer_service, &timer, &systemctl)
            .unwrap();

        assert!(!service.exists());
        assert!(!timer_service.exists());
        assert!(!timer.exists());
        assert!(manager_config.exists());
        assert!(config_root.exists());
        assert!(mihomo_binary.exists());
        assert!(mihoto_binary.exists());
        let calls = fs::read_to_string(dir.path().join("systemctl.log")).unwrap();
        let timer_disable = calls.find("disable --now mihoto-update.timer").unwrap();
        let service_disable = calls.find("disable --now mihomo.service").unwrap();
        assert!(timer_disable < service_disable);
        assert!(
            !calls.lines().any(|call| call == "reset-failed"),
            "uninstall must not reset unrelated failed units"
        );
    }

    #[test]
    fn purge_removes_all_managed_system_data_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let manager_config = dir.path().join("mihoto.toml");
        let config_root = dir.path().join("mihomo");
        let mihomo_binary = dir.path().join("mihomo-bin");
        let mihoto_binary = dir.path().join("mihoto-bin");
        let service = dir.path().join("mihomo.service");
        let timer_service = dir.path().join("mihoto-update.service");
        let timer = dir.path().join("mihoto-update.timer");
        fs::create_dir(&config_root).unwrap();
        for path in [
            &manager_config,
            &mihomo_binary,
            &mihoto_binary,
            &service,
            &timer_service,
            &timer,
        ] {
            fs::write(path, "fixture").unwrap();
        }
        let systemctl = fake_systemctl(dir.path(), 1);
        let config = Config {
            mihoto_binary_path: mihoto_binary.display().to_string(),
            mihomo_binary_path: mihomo_binary.display().to_string(),
            mihomo_config_root: config_root.display().to_string(),
            ..Config::default()
        };
        let mut mihoto = Mihoto::from_config(config);
        mihoto.mihomo_target_service_path = service.display().to_string();

        for _ in 0..2 {
            mihoto
                .uninstall_with_paths(true, &manager_config, &timer_service, &timer, &systemctl)
                .unwrap();
        }

        for path in [
            &manager_config,
            &config_root,
            &mihomo_binary,
            &mihoto_binary,
            &service,
            &timer_service,
            &timer,
        ] {
            assert!(!path.exists(), "{} should be removed", path.display());
        }
    }

    #[test]
    fn purge_rejects_dangerous_directory_targets() {
        let dir = tempfile::tempdir().unwrap();
        let systemctl = fake_systemctl(dir.path(), 1);
        let config = Config {
            mihomo_config_root: "/".into(),
            ..Config::default()
        };
        let mihoto = Mihoto::from_config(config);
        assert!(mihoto
            .uninstall_with_paths(
                true,
                &dir.path().join("mihoto.toml"),
                &dir.path().join("update.service"),
                &dir.path().join("update.timer"),
                &systemctl,
            )
            .is_err());
    }

    #[test]
    fn purge_rejects_targets_that_do_not_look_mihoto_managed() {
        for (label, target) in [
            ("mihomo config root", "/home/pectics"),
            ("mihomo config root", "/var/lib"),
            ("mihomo binary", "/usr/bin/bash"),
            ("mihoto binary", "/usr/bin/curl"),
            ("manager config", "/etc/passwd"),
        ] {
            assert!(
                validate_safe_purge_target(label, Path::new(target)).is_err(),
                "{target} must not be accepted as a {label} purge target"
            );
        }
    }
}
