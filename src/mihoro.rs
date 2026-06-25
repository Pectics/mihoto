use crate::cmd::{CronCommands, DeployCommands, ProfileCommands, ProxyCommands, ScheduleCommands};
use crate::config::{parse_config, Config, DeploymentBackend, ProfileConfig, ProfileSource};
use crate::config_store::ConfigGenerationStore;
use crate::cron;
use crate::proxy::{proxy_export_cmd, proxy_unset_cmd};
use crate::resolve_mihomo_bin;
use crate::source::fetch_profile_source;
use crate::systemctl::{Systemctl, SystemdScope};
use crate::ui::{install_ui, resolve_external_ui_path};
use crate::utils::{
    create_parent_dir, create_private_parent_dir, delete_file, download_file, extract_gzip,
    redact_sensitive, redact_sensitive_values, write_private_file, DETAIL_PREFIX,
};

use anyhow::Error;

use std::os::unix::prelude::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{collections::HashMap, fs};

use anyhow::{anyhow, Result};
use colored::Colorize;
use local_ip_address::local_ip;
use reqwest::Client;
use shellexpand::tilde;
use tempfile::NamedTempFile;

pub struct ApplyOptions<'a> {
    pub profile: Option<&'a str>,
    pub dry_run: bool,
    pub diff: bool,
}

#[derive(Debug)]
pub struct Mihoro {
    // global mihoro config
    pub prefix: String,
    pub config: Config,

    // mihomo global variables derived from mihoro config
    pub mihomo_target_binary_path: String,
    pub mihomo_target_config_root: String,
    pub mihomo_target_config_path: String,
    pub mihomo_target_service_path: String,
    systemd_scope: SystemdScope,
}

struct DeploymentPaths {
    binary_path: String,
    config_root: String,
    service_path: String,
    scope: SystemdScope,
}

impl DeploymentPaths {
    fn from_config(config: &Config) -> Self {
        match config.deployment.backend {
            DeploymentBackend::SystemdUser => {
                let config_root = tilde(&config.mihomo_config_root).to_string();
                Self {
                    binary_path: tilde(&config.mihomo_binary_path).to_string(),
                    config_root,
                    service_path: tilde(&format!("{}/mihomo.service", config.user_systemd_root))
                        .to_string(),
                    scope: SystemdScope::User,
                }
            }
            DeploymentBackend::SystemdSystem => Self {
                binary_path: "/usr/local/libexec/mihoto/mihomo".to_string(),
                config_root: "/etc/mihoto".to_string(),
                service_path: "/etc/systemd/system/mihomo.service".to_string(),
                scope: SystemdScope::System,
            },
        }
    }
}

/// Outcome of a single setup stage, used by `mihoro init`.
pub enum StageStatus {
    Installed,
    Skipped(String),
    Failed(Error),
}

/// Plan returned by [`Mihoro::prepare_binary`]: either we already have the binary and
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

impl Mihoro {
    pub fn new(config_path: &str) -> Result<Mihoro> {
        let config = parse_config(tilde(config_path).as_ref())?;
        Ok(Self::from_config(config))
    }

    /// Build a `Mihoro` from an already-validated `Config`.
    pub fn from_config(config: Config) -> Mihoro {
        let paths = DeploymentPaths::from_config(&config);
        Mihoro {
            prefix: String::from("mihoro:"),
            config,
            mihomo_target_binary_path: paths.binary_path,
            mihomo_target_config_root: paths.config_root.clone(),
            mihomo_target_config_path: format!("{}/config.yaml", paths.config_root),
            mihomo_target_service_path: paths.service_path,
            systemd_scope: paths.scope,
        }
    }

    pub fn systemd_scope(&self) -> SystemdScope {
        self.systemd_scope
    }

    fn config_generation_store(&self) -> ConfigGenerationStore {
        self.config_generation_store_for_profile(&self.config.active_profile)
    }

    fn config_generation_store_for_profile(&self, profile: &str) -> ConfigGenerationStore {
        let profile_state_root = tilde(&self.config.profile_state_root);
        ConfigGenerationStore::new_for_profile(
            Path::new(profile_state_root.as_ref())
                .join("profiles")
                .join(profile),
            Path::new(&self.mihomo_target_config_path),
        )
    }

    fn selected_profile_name<'a>(&'a self, profile: Option<&'a str>) -> &'a str {
        profile.unwrap_or(&self.config.active_profile)
    }

    fn selected_profile(&self, profile: Option<&str>) -> Result<(String, ProfileConfig)> {
        let name = self.selected_profile_name(profile).to_string();
        let config = self
            .config
            .effective_profile(&name)
            .ok_or_else(|| anyhow!("profile `{}` not found", name))?;
        Ok((name, config))
    }

    pub fn mihomo_binary_exists(&self) -> bool {
        fs::metadata(&self.mihomo_target_binary_path).is_ok()
    }

    fn activate_candidate_config(&self) -> Result<bool> {
        let store = self.config_generation_store();
        self.activate_candidate_config_for_store(&store)
    }

    fn activate_candidate_config_for_store(&self, store: &ConfigGenerationStore) -> Result<bool> {
        if store.candidate_matches_active_and_compat()? {
            return Ok(false);
        }
        self.validate_candidate_config_for_store(store)?;
        store.activate_candidate()
    }

    fn validate_candidate_config_for_store(&self, store: &ConfigGenerationStore) -> Result<()> {
        let candidate_path = store
            .paths
            .candidate_yaml
            .to_str()
            .ok_or_else(|| anyhow!("candidate config path is not valid UTF-8"))?;
        let args = candidate_validation_args(&self.mihomo_target_config_root, candidate_path);
        let output = Command::new(&self.mihomo_target_binary_path)
            .args(&args)
            .output()
            .map_err(|err| {
                anyhow!(
                    "failed to validate candidate config with `{}`: {}",
                    redact_sensitive(&self.mihomo_target_binary_path),
                    err
                )
            })?;

        if output.status.success() {
            return Ok(());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!(
            "candidate config validation failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            redact_sensitive(stdout.trim()),
            redact_sensitive(stderr.trim())
        ))
    }

    /// Stage 1 of the binary install: resolve the URL and download to a temp file.
    ///
    /// Skips if the binary exists and `force` is false. The returned [`BinaryPlan`] is
    /// handed to [`Mihoro::install_binary`] *after* every other download stage so that
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
            &self.config.mihoro_user_agent,
        )
        .await?;
        Ok(BinaryPlan::Install(temp_file))
    }

    /// Stage 2 of the binary install: stop the running service if any, then extract the
    /// downloaded binary into place and set its executable bit.
    ///
    /// Must run *after* every other network-dependent stage; see [`BinaryPlan`].
    pub async fn install_binary(&self, temp_file: NamedTempFile) -> Result<StageStatus> {
        // Stop mihomo.service before overwriting to avoid "Text file busy".
        let binary_exists = fs::metadata(&self.mihomo_target_binary_path).is_ok();
        if binary_exists {
            println!(
                "{} Stopping mihomo.service before overwriting binary...",
                DETAIL_PREFIX.cyan()
            );
            Systemctl::new().stop("mihomo.service").execute()?;
        }

        extract_gzip(
            temp_file.path(),
            &self.mihomo_target_binary_path,
            DETAIL_PREFIX.cyan(),
        )?;
        let executable = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&self.mihomo_target_binary_path, executable)?;
        Ok(StageStatus::Installed)
    }

    /// Download remote config YAML and apply TOML overrides.
    /// If the config file already exists and `force` is false, only re-applies overrides.
    pub async fn ensure_remote_config(&self, client: &Client, force: bool) -> Result<StageStatus> {
        let store = self.config_generation_store();
        let seeded = store.seed_source_from_legacy_config()?;
        let has_local_config = store.paths.source_yaml.exists()
            || store.paths.active_yaml.exists()
            || store.paths.compat_config_yaml.exists();
        if !force && has_local_config {
            let changed = store.render_candidate(&self.config.mihomo_config)?;
            let activated = self.activate_candidate_config()?;
            return if changed || activated || seeded {
                Ok(StageStatus::Installed)
            } else {
                Ok(StageStatus::Skipped("config already current".to_string()))
            };
        }

        create_private_parent_dir(&store.paths.source_yaml)?;
        let staged_source = NamedTempFile::new_in(&store.paths.root)?;
        let (profile_name, profile_config) = self.selected_profile(None)?;
        let headers = read_profile_headers(&self.config, &profile_name)?;
        let user_agent = profile_config
            .user_agent
            .as_deref()
            .unwrap_or(&self.config.mihoro_user_agent);
        fetch_profile_source(
            client,
            &profile_config.source,
            user_agent,
            &headers,
            staged_source.path(),
        )
        .await?;
        store.install_source_from_stage(staged_source.path())?;
        store.render_candidate(&self.config.mihomo_config)?;
        if self.activate_candidate_config()? {
            Ok(StageStatus::Installed)
        } else {
            Ok(StageStatus::Skipped("config already current".to_string()))
        }
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
                    &self.config.mihoro_user_agent,
                )
                .await?;
            }
            if force || !geosite_path.exists() {
                download_file(
                    client,
                    &geox_url.geosite,
                    &geosite_path,
                    &self.config.mihoro_user_agent,
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
                &self.config.mihoro_user_agent,
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
            &self.config.mihoro_user_agent,
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
                return Ok(StageStatus::Skipped("service file unchanged".to_string()));
            }
        }
        create_parent_dir(Path::new(&self.mihomo_target_service_path))?;
        fs::write(&self.mihomo_target_service_path, &service_content)?;
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
                "Mihomo binary not found at {}. Run `mihoro init` first.",
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
                        "already at {}",
                        installed_version
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
            &self.config.mihoro_user_agent,
        )
        .await?;

        // Stop mihomo.service before overwriting binary to avoid "Text file busy" error
        println!(
            "{} Stopping mihomo.service before overwriting...",
            DETAIL_PREFIX.yellow()
        );
        Systemctl::new().stop("mihomo.service").execute()?;

        // Extract and overwrite the binary
        extract_gzip(
            temp_path,
            &self.mihomo_target_binary_path,
            DETAIL_PREFIX.cyan(),
        )?;

        // Set executable permission
        let executable = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&self.mihomo_target_binary_path, executable)?;

        Ok(StageStatus::Installed)
    }

    pub async fn update_config(
        &self,
        client: &Client,
        profile: Option<&str>,
    ) -> Result<StageStatus> {
        let (profile_name, profile_config) = self.selected_profile(profile)?;
        let store = self.config_generation_store_for_profile(&profile_name);
        store.seed_source_from_legacy_config()?;

        // Download remote mihomo config and apply override
        create_private_parent_dir(&store.paths.source_yaml)?;
        let staged_source = NamedTempFile::new_in(&store.paths.root)?;
        let headers = read_profile_headers(&self.config, &profile_name)?;
        let user_agent = profile_config
            .user_agent
            .as_deref()
            .unwrap_or(&self.config.mihoro_user_agent);
        fetch_profile_source(
            client,
            &profile_config.source,
            user_agent,
            &headers,
            staged_source.path(),
        )
        .await?;
        store.install_source_from_stage(staged_source.path())?;

        store.render_candidate(&self.config.mihomo_config)?;
        if self.activate_candidate_config()? {
            println!(
                "{} Updated and applied config overrides",
                DETAIL_PREFIX.cyan()
            );
            Ok(StageStatus::Installed)
        } else {
            Ok(StageStatus::Skipped("config already current".to_string()))
        }
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
                    &self.config.mihoro_user_agent,
                )
                .await?;
                download_file(
                    client,
                    &geox_url.geosite,
                    &Path::new(&self.mihomo_target_config_root).join("geosite.dat"),
                    &self.config.mihoro_user_agent,
                )
                .await?;
            } else {
                download_file(
                    client,
                    &geox_url.mmdb,
                    &Path::new(&self.mihomo_target_config_root).join("country.mmdb"),
                    &self.config.mihoro_user_agent,
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
            &self.config.mihoro_user_agent,
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

    pub async fn restart_service_with_config_rollback(&self) -> Result<StageStatus> {
        println!("{} Restarting mihomo.service...", DETAIL_PREFIX.cyan());
        let restart_result = Systemctl::new().restart("mihomo.service").execute();
        if restart_result.is_ok() && Systemctl::is_active("mihomo.service") {
            return Ok(StageStatus::Installed);
        }

        let store = self.config_generation_store();
        let restored = store.restore_last_good()?;
        if restored {
            let _ = Systemctl::new().restart("mihomo.service").execute();
        }

        match restart_result {
            Ok(status) => Err(anyhow!(
                "mihomo.service was not active after restart (status: {}); restored last-good config: {}",
                status,
                restored
            )),
            Err(err) => Err(anyhow!(
                "failed to restart mihomo.service: {:#}; restored last-good config: {}",
                err,
                restored
            )),
        }
    }

    pub async fn apply(&self, options: ApplyOptions<'_>) -> Result<()> {
        let profile_name = self.selected_profile_name(options.profile);
        let store = self.config_generation_store_for_profile(profile_name);
        store.seed_source_from_legacy_config()?;
        store.render_candidate(&self.config.mihomo_config)?;
        if options.diff {
            let headers = read_profile_headers(&self.config, profile_name)?;
            let diff = store.render_redacted_diff()?;
            print!(
                "{}",
                redact_sensitive_values(&diff, headers.values().map(String::as_str))
            );
        }
        if options.dry_run {
            self.validate_candidate_config_for_store(&store)?;
            println!(
                "{} Dry run succeeded; config was not activated",
                self.prefix.green().bold()
            );
            return Ok(());
        }
        let activated = self.activate_candidate_config_for_store(&store)?;
        println!(
            "{} Applied mihomo config overrides",
            self.prefix.green().bold()
        );

        // Restart mihomo systemd service
        if activated {
            self.restart_service_with_config_rollback().await.map(|_| {
                println!("{} Restarted mihomo.service", self.prefix.green().bold());
            })?;
        }
        Ok(())
    }

    pub fn profile_commands(
        &self,
        config_path: &str,
        profile: &Option<ProfileCommands>,
    ) -> Result<()> {
        let Some(profile) = profile else {
            return Ok(());
        };
        match profile {
            ProfileCommands::Add {
                name,
                url,
                file,
                existing,
                user_agent,
                header,
                force,
            } => {
                let mut config = Config::setup_from(tilde(config_path).as_ref())?;
                if config.profiles.contains_key(name) && !force {
                    anyhow::bail!("profile `{}` already exists; pass --force to replace", name);
                }
                let source = match (url, file, existing) {
                    (Some(url), None, None) => ProfileSource::Url { url: url.clone() },
                    (None, Some(path), None) => ProfileSource::File { path: path.clone() },
                    (None, None, Some(path)) => ProfileSource::Existing { path: path.clone() },
                    _ => anyhow::bail!("exactly one profile source is required"),
                };
                config.profiles.insert(
                    name.clone(),
                    ProfileConfig {
                        source,
                        user_agent: user_agent.clone(),
                    },
                );
                if config.active_profile.is_empty() {
                    config.active_profile = name.clone();
                }
                config.write(Path::new(tilde(config_path).as_ref()))?;
                if !header.is_empty() {
                    write_profile_headers(&config, name, header)?;
                }
                println!("{} Added profile `{}`", self.prefix.green(), name);
            }
            ProfileCommands::List => {
                for name in self.config.profiles.keys() {
                    let marker = if name == &self.config.active_profile {
                        "*"
                    } else {
                        " "
                    };
                    println!("{marker} {name}");
                }
                if self.config.profiles.is_empty() && !self.config.remote_config_url.is_empty() {
                    println!("* default (legacy remote_config_url)");
                }
            }
            ProfileCommands::Show { name } => {
                let profile = self
                    .config
                    .effective_profile(name)
                    .ok_or_else(|| anyhow!("profile `{}` not found", name))?;
                println!("{}", toml::to_string(&profile)?);
            }
            ProfileCommands::Use { name } => {
                let mut config = Config::setup_from(tilde(config_path).as_ref())?;
                if config.effective_profile(name).is_none() {
                    anyhow::bail!("profile `{}` not found", name);
                }
                config.active_profile = name.clone();
                config.write(Path::new(tilde(config_path).as_ref()))?;
                println!("{} Active profile set to `{}`", self.prefix.green(), name);
            }
            ProfileCommands::Remove { name } => {
                let mut config = Config::setup_from(tilde(config_path).as_ref())?;
                if config.profiles.remove(name).is_none() {
                    anyhow::bail!("profile `{}` not found", name);
                }
                if config.active_profile == *name {
                    config.active_profile = config
                        .profiles
                        .keys()
                        .next()
                        .cloned()
                        .unwrap_or_else(|| "default".to_string());
                }
                config.write(Path::new(tilde(config_path).as_ref()))?;
                println!("{} Removed profile `{}`", self.prefix.green(), name);
            }
        }
        Ok(())
    }

    pub fn uninstall(&self) -> Result<()> {
        Systemctl::new().stop("mihomo.service").execute()?;
        Systemctl::new().disable("mihomo.service").execute()?;

        delete_file(&self.mihomo_target_service_path, self.prefix.cyan())?;
        delete_file(&self.mihomo_target_config_path, self.prefix.cyan())?;

        Systemctl::new().daemon_reload().execute()?;
        Systemctl::new().reset_failed().execute()?;
        println!(
            "{} Disabled and reloaded systemd services",
            self.prefix.green()
        );

        // Disable and remove cron job
        cron::disable_auto_update(&self.prefix)?;

        println!(
            "{} You may need to remove mihomo binary and config directory manually",
            self.prefix.yellow()
        );

        let remove_cmd = format!(
            "rm -R {} {}",
            self.mihomo_target_binary_path, self.mihomo_target_config_root
        );
        println!("{} `{}`", "->".dimmed(), remove_cmd.underline().bold());
        Ok(())
    }

    pub fn proxy_commands(&self, proxy: &Option<ProxyCommands>) -> Result<()> {
        // `mixed_port` takes precedence over `port` and `socks_port` for proxy export
        let port = self
            .config
            .mihomo_config
            .mixed_port
            .as_ref()
            .unwrap_or(&self.config.mihomo_config.port);
        let socks_port = self
            .config
            .mihomo_config
            .mixed_port
            .as_ref()
            .unwrap_or(&self.config.mihomo_config.socks_port);

        match proxy {
            Some(ProxyCommands::Export) => {
                println!("{}", proxy_export_cmd("127.0.0.1", port, socks_port))
            }
            Some(ProxyCommands::ExportLan) => {
                if !self.config.mihomo_config.allow_lan.unwrap_or(false) {
                    println!(
                        "{} `{}` is false, proxy is not available for LAN",
                        "warning:".yellow(),
                        "allow_lan".bold()
                    );
                }

                println!(
                    "{}",
                    proxy_export_cmd(&local_ip()?.to_string(), port, socks_port)
                );
            }
            Some(ProxyCommands::Unset) => {
                println!("{}", proxy_unset_cmd())
            }
            _ => (),
        }
        Ok(())
    }

    pub async fn deploy_commands(
        &self,
        _config_path: &str,
        command: &Option<DeployCommands>,
    ) -> Result<()> {
        match command {
            Some(DeployCommands::Status) => {
                println!(
                    "{} Deployment backend: {:?}",
                    self.prefix.green(),
                    self.config.deployment.backend
                );
                println!(
                    "{} Service: {}",
                    "->".dimmed(),
                    self.mihomo_target_service_path
                );
            }
            Some(DeployCommands::Apply { .. })
            | Some(DeployCommands::Import { .. })
            | Some(DeployCommands::Migrate { .. })
            | Some(DeployCommands::Rollback { .. }) => {
                anyhow::bail!("deploy command is not implemented yet")
            }
            None => {}
        }
        Ok(())
    }

    pub fn schedule_commands(&self, command: &Option<ScheduleCommands>) -> Result<()> {
        match command {
            Some(ScheduleCommands::Status) => {
                println!(
                    "{} Scheduler backend: {:?}",
                    self.prefix.green(),
                    self.config.scheduler.backend
                );
            }
            Some(ScheduleCommands::Enable { .. }) | Some(ScheduleCommands::Disable) => {
                anyhow::bail!("schedule command is not implemented yet")
            }
            None => {}
        }
        Ok(())
    }

    pub fn cron_commands(&self, command: &Option<CronCommands>) -> Result<()> {
        match command {
            Some(CronCommands::Enable) => {
                cron::enable_auto_update(self.config.auto_update_interval, &self.prefix)
            }
            Some(CronCommands::Disable) => cron::disable_auto_update(&self.prefix),
            Some(CronCommands::Status) => {
                cron::get_cron_status(&self.prefix, &self.mihomo_target_config_path)
            }
            _ => Ok(()),
        }
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

fn write_profile_headers(config: &Config, name: &str, header: &[String]) -> Result<()> {
    let mut headers = HashMap::new();
    for item in header {
        let (key, value) = item
            .split_once('=')
            .ok_or_else(|| anyhow!("header `{}` must use KEY=VALUE syntax", item))?;
        if key.trim().is_empty() {
            anyhow::bail!("header key cannot be empty");
        }
        headers.insert(key.trim().to_string(), value.trim().to_string());
    }
    let profile_data_root = tilde(&config.profile_data_root);
    let path = Path::new(profile_data_root.as_ref())
        .join("profiles")
        .join(name)
        .join("metadata.toml");
    let serialized = toml::to_string(&toml::toml! { headers = headers })?;
    write_private_file(&path, serialized.as_bytes())
}

fn read_profile_headers(config: &Config, name: &str) -> Result<HashMap<String, String>> {
    #[derive(serde::Deserialize)]
    struct Metadata {
        #[serde(default)]
        headers: HashMap<String, String>,
    }

    let profile_data_root = tilde(&config.profile_data_root);
    let path = Path::new(profile_data_root.as_ref())
        .join("profiles")
        .join(name)
        .join("metadata.toml");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let metadata: Metadata = toml::from_str(&fs::read_to_string(path)?)?;
    Ok(metadata.headers)
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

fn candidate_validation_args(config_root: &str, candidate_path: &str) -> Vec<String> {
    vec![
        "-t".to_string(),
        "-d".to_string(),
        config_root.to_string(),
        "-f".to_string(),
        candidate_path.to_string(),
    ]
}

/// Render the systemd unit file content for mihomo.service.
///
/// Reference: https://wiki.metacubex.one/startup/service/
fn render_service_string(binary_path: &str, config_root: &str) -> String {
    format!(
        "[Unit]
Description=mihomo Daemon, Another Clash Kernel.
After=network.target NetworkManager.service systemd-networkd.service iwd.service

[Service]
Type=simple
LimitNPROC=4096
LimitNOFILE=65536
Restart=always
ExecStartPre=/usr/bin/sleep 1s
ExecStart={} -d {}
ExecReload=/bin/kill -HUP $MAINPID

[Install]
WantedBy=default.target",
        binary_path, config_root
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::apply_mihomo_override;
    use std::fs;
    use tempfile::tempdir;

    /// Test that Mihoro::new correctly parses config and derives paths
    #[test]
    fn test_mihoro_new_parses_config_and_derives_paths() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        // Write a valid config file
        let toml_content = r#"
            remote_config_url = "http://example.com/config.yaml"
            mihomo_binary_path = "/tmp/test/mihomo"
            mihomo_config_root = "/tmp/test/mihomo"
            user_systemd_root = "/tmp/test/systemd"
        "#;
        fs::write(&config_path, toml_content)?;

        let mihoro = Mihoro::new(&config_path.to_str().unwrap().to_string())?;

        assert_eq!(mihoro.mihomo_target_binary_path, "/tmp/test/mihomo");
        assert_eq!(mihoro.mihomo_target_config_root, "/tmp/test/mihomo");
        assert_eq!(
            mihoro.mihomo_target_config_path,
            "/tmp/test/mihomo/config.yaml"
        );
        assert_eq!(
            mihoro.mihomo_target_service_path,
            "/tmp/test/systemd/mihomo.service"
        );

        Ok(())
    }

    #[test]
    fn system_deployment_derives_fixed_system_paths() {
        let mut config = Config::new();
        config.remote_config_url = "https://example.com/sub.yaml".to_string();
        config.deployment.backend = crate::config::DeploymentBackend::SystemdSystem;

        let mihoro = Mihoro::from_config(config);

        assert_eq!(
            mihoro.mihomo_target_binary_path,
            "/usr/local/libexec/mihoto/mihomo"
        );
        assert_eq!(mihoro.mihomo_target_config_root, "/etc/mihoto");
        assert_eq!(mihoro.mihomo_target_config_path, "/etc/mihoto/config.yaml");
        assert_eq!(
            mihoro.mihomo_target_service_path,
            "/etc/systemd/system/mihomo.service"
        );
        assert_eq!(
            mihoro.systemd_scope(),
            crate::systemctl::SystemdScope::System
        );
    }

    /// Test that proxy_commands uses mixed_port when set
    #[test]
    fn test_proxy_commands_uses_mixed_port_when_set() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        let toml_content = r#"
            remote_config_url = "http://example.com/config.yaml"
            mihomo_binary_path = "/tmp/test/mihomo"
            mihomo_config_root = "/tmp/test/mihomo"
            user_systemd_root = "/tmp/test/systemd"

            [mihomo_config]
            port = 7891
            socks_port = 7892
            mixed_port = 7890
        "#;
        fs::write(&config_path, toml_content)?;

        let mihoro = Mihoro::new(&config_path.to_str().unwrap().to_string())?;

        // Test Export command (should use mixed_port 7890)
        let cmd = mihoro.proxy_commands(&Some(ProxyCommands::Export));
        assert!(cmd.is_ok());

        Ok(())
    }

    /// Test that proxy_commands falls back to port/socks_port when mixed_port is None
    #[test]
    fn test_proxy_commands_fallback_to_port_when_mixed_port_none() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        let toml_content = r#"
            remote_config_url = "http://example.com/config.yaml"
            mihomo_binary_path = "/tmp/test/mihomo"
            mihomo_config_root = "/tmp/test/mihomo"
            user_systemd_root = "/tmp/test/systemd"

            [mihomo_config]
            port = 7891
            socks_port = 7892
        "#;
        fs::write(&config_path, toml_content)?;

        let mihoro = Mihoro::new(&config_path.to_str().unwrap().to_string())?;

        let cmd = mihoro.proxy_commands(&Some(ProxyCommands::Export));
        assert!(cmd.is_ok());

        Ok(())
    }

    #[test]
    fn test_external_ui_target_dir_resolves_relative_path() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        let toml_content = r#"
            remote_config_url = "http://example.com/config.yaml"
            mihomo_binary_path = "/tmp/test/mihomo"
            mihomo_config_root = "/tmp/test/mihomo"
            user_systemd_root = "/tmp/test/systemd"

            [mihomo_config]
            external_ui = "ui"
        "#;
        fs::write(&config_path, toml_content)?;

        let mihoro = Mihoro::new(&config_path.to_str().unwrap().to_string())?;
        assert_eq!(
            mihoro.external_ui_target_dir(),
            Some(PathBuf::from("/tmp/test/mihomo/ui"))
        );

        Ok(())
    }

    #[test]
    fn test_extract_mihomo_version_from_stable_output() {
        let output = "Mihomo Meta v1.19.23 linux amd64 with go1.25.1 2026-04-07";
        assert_eq!(extract_mihomo_version(output), Some("v1.19.23".to_string()));
    }

    #[test]
    fn test_extract_mihomo_version_normalizes_bare_stable_output() {
        let output = "Mihomo Meta 1.19.23 linux amd64 with go1.25.1 2026-04-07";
        assert_eq!(extract_mihomo_version(output), Some("v1.19.23".to_string()));
    }

    #[test]
    fn test_extract_mihomo_version_from_alpha_output() {
        let output = "Mihomo Meta alpha-c107c6a linux amd64 with go1.25.1";
        assert_eq!(
            extract_mihomo_version(output),
            Some("alpha-c107c6a".to_string())
        );
    }

    #[test]
    fn test_candidate_validation_args_use_runtime_root_and_candidate() {
        let args = candidate_validation_args("/tmp/mihomo", "/tmp/mihomo/candidate.yaml");

        assert_eq!(
            args,
            vec![
                "-t".to_string(),
                "-d".to_string(),
                "/tmp/mihomo".to_string(),
                "-f".to_string(),
                "/tmp/mihomo/candidate.yaml".to_string(),
            ]
        );
    }

    /// Test integration: download config → apply override → verify result
    #[test]
    fn test_integration_apply_override_flow() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");
        let yaml_path = dir.path().join("config.yaml");
        let profile_state_root = dir.path().join("state");

        // Write config with custom port override
        let toml_content = r#"
            remote_config_url = "http://example.com/config.yaml"
            mihomo_binary_path = "/tmp/test/mihomo"
            mihomo_config_root = "{}"
            profile_state_root = "{}"
            user_systemd_root = "/tmp/test/systemd"

            [mihomo_config]
            port = 9999
            socks_port = 9998
        "#;
        fs::write(
            &config_path,
            toml_content
                .replacen("{}", dir.path().to_str().unwrap(), 1)
                .replacen("{}", profile_state_root.to_str().unwrap(), 1),
        )?;

        // Write initial mihomo config
        let yaml_content = r#"
            port: 8080
            socks-port: 8081
            mode: rule
            proxies:
              - name: "test"
                type: http
                server: example.com
                port: 443
        "#;
        fs::write(&yaml_path, yaml_content)?;

        // Create Mihoro instance and apply override
        let mihoro = Mihoro::new(&config_path.to_str().unwrap().to_string())?;
        apply_mihomo_override(yaml_path.to_str().unwrap(), &mihoro.config.mihomo_config)?;

        // Verify override was applied
        let updated_content = fs::read_to_string(&yaml_path)?;
        assert!(updated_content.contains("port: 9999"));
        assert!(updated_content.contains("socks-port: 9998"));
        assert!(updated_content.contains("proxies:"));

        Ok(())
    }

    #[tokio::test]
    async fn test_ensure_remote_config_seeds_generations_then_skips_when_current() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");
        let yaml_path = dir.path().join("config.yaml");
        let profile_state_root = dir.path().join("state");

        let toml_content = r#"
            remote_config_url = "http://example.com/config.yaml"
            mihomo_binary_path = "/tmp/test/mihomo"
            mihomo_config_root = "{}"
            profile_state_root = "{}"
            user_systemd_root = "/tmp/test/systemd"

            [mihomo_config]
            port = 9999
            socks_port = 9998
        "#;
        fs::write(
            &config_path,
            toml_content
                .replacen("{}", dir.path().to_str().unwrap(), 1)
                .replacen("{}", profile_state_root.to_str().unwrap(), 1),
        )?;

        let yaml_content = r#"
            port: 8080
            socks-port: 8081
            mode: rule
            proxies:
              - name: "test"
                type: http
                server: example.com
                port: 443
        "#;
        fs::write(&yaml_path, yaml_content)?;

        let mihoro = Mihoro::new(&config_path.to_str().unwrap().to_string())?;
        apply_mihomo_override(yaml_path.to_str().unwrap(), &mihoro.config.mihomo_config)?;
        let current_content = fs::read_to_string(&yaml_path)?;

        let status = mihoro.ensure_remote_config(&Client::new(), false).await?;

        match status {
            StageStatus::Installed => {}
            StageStatus::Skipped(reason) => panic!("expected generation seed, got skip: {reason}"),
            StageStatus::Failed(_) => panic!("ensure_remote_config returned a failed status"),
        }
        assert_eq!(fs::read_to_string(&yaml_path)?, current_content);
        let profile_root = profile_state_root.join("profiles/default");
        assert_eq!(
            fs::read_to_string(profile_root.join("source.yaml"))?,
            current_content
        );
        assert_eq!(
            fs::read_to_string(profile_root.join("active.yaml"))?,
            current_content
        );
        assert!(fs::read_to_string(profile_root.join("candidate.yaml"))?.contains("port: 9999"));
        assert!(fs::read_to_string(profile_root.join("overlay.yaml"))?.contains("port: 9999"));

        let status = mihoro.ensure_remote_config(&Client::new(), false).await?;
        match status {
            StageStatus::Skipped(reason) => assert_eq!(reason, "config already current"),
            StageStatus::Installed => panic!("expected remote config to be skipped"),
            StageStatus::Failed(_) => panic!("ensure_remote_config returned a failed status"),
        }

        Ok(())
    }

    #[test]
    fn profile_add_stores_headers_in_private_metadata() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("mihoro.toml");
        let profile_data_root = dir.path().join("data");

        let mut config = Config::new();
        config.remote_config_url = "https://example.com/legacy.yaml".to_string();
        config.profile_data_root = profile_data_root.to_string_lossy().to_string();
        config.write(&config_path)?;

        let mihoro = Mihoro::from_config(config);
        mihoro.profile_commands(
            config_path.to_str().unwrap(),
            &Some(ProfileCommands::Add {
                name: "work".to_string(),
                url: Some("https://example.com/sub.yaml".to_string()),
                file: None,
                existing: None,
                user_agent: Some("mihoro-test".to_string()),
                header: vec!["Authorization=Bearer token".to_string()],
                force: false,
            }),
        )?;

        let main_config = fs::read_to_string(&config_path)?;
        assert!(main_config.contains("[profiles.work]"));
        assert!(!main_config.contains("Bearer token"));

        let metadata_path = profile_data_root.join("profiles/work/metadata.toml");
        let metadata = fs::read_to_string(&metadata_path)?;
        assert!(metadata.contains("Authorization"));
        assert!(metadata.contains("Bearer token"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(metadata_path)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        Ok(())
    }

    #[tokio::test]
    async fn apply_dry_run_renders_candidate_without_activation() -> Result<()> {
        let dir = tempdir()?;
        let binary_path = dir.path().join("mihomo");
        fs::write(&binary_path, "#!/bin/sh\nexit 0\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o755))?;
        }

        let runtime_root = dir.path().join("runtime");
        let profile_state_root = dir.path().join("state");
        let profile_root = profile_state_root.join("profiles/default");
        fs::create_dir_all(&profile_root)?;
        fs::create_dir_all(&runtime_root)?;
        fs::write(
            profile_root.join("source.yaml"),
            "port: 8080\nsocks-port: 8081\nproxies: []\n",
        )?;
        fs::write(
            profile_root.join("active.yaml"),
            "port: 8080\nsocks-port: 8081\nproxies: []\n",
        )?;
        fs::write(
            runtime_root.join("config.yaml"),
            "port: 8080\nsocks-port: 8081\nproxies: []\n",
        )?;

        let mut config = Config::new();
        config.remote_config_url = "https://example.com/sub.yaml".to_string();
        config.mihomo_binary_path = binary_path.to_string_lossy().to_string();
        config.mihomo_config_root = runtime_root.to_string_lossy().to_string();
        config.profile_state_root = profile_state_root.to_string_lossy().to_string();
        config.mihomo_config.port = 9999;
        config.mihomo_config.socks_port = 9998;
        let mihoro = Mihoro::from_config(config);

        mihoro
            .apply(ApplyOptions {
                profile: None,
                dry_run: true,
                diff: true,
            })
            .await?;

        assert!(fs::read_to_string(profile_root.join("candidate.yaml"))?.contains("port: 9999"));
        assert_eq!(
            fs::read_to_string(profile_root.join("active.yaml"))?,
            "port: 8080\nsocks-port: 8081\nproxies: []\n"
        );
        assert_eq!(
            fs::read_to_string(runtime_root.join("config.yaml"))?,
            "port: 8080\nsocks-port: 8081\nproxies: []\n"
        );

        Ok(())
    }
}
