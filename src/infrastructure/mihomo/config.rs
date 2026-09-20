use crate::infrastructure::config_store::apply_mihomo_override;
use crate::infrastructure::download::{download_file, DETAIL_PREFIX};
use crate::infrastructure::filesystem::try_decode_base64_file_inplace;
use crate::infrastructure::systemctl::Systemctl;

use std::fs;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Result};
use colored::Colorize;
use reqwest::Client;
use tempfile::NamedTempFile;

use super::{Mihoto, StageStatus, SERVICE_HEALTH_DELAY};

impl Mihoto {
    fn ensure_config_root_secure(&self) -> Result<()> {
        fs::create_dir_all(&self.mihomo_target_config_root)?;
        fs::set_permissions(
            &self.mihomo_target_config_root,
            fs::Permissions::from_mode(0o750),
        )?;
        Ok(())
    }

    fn validate_mihomo_config(&self, config_path: &Path) -> Result<()> {
        let output = Command::new(&self.mihomo_target_binary_path)
            .arg("-t")
            .arg("-d")
            .arg(&self.mihomo_target_config_root)
            .arg("-f")
            .arg(config_path)
            .output()
            .map_err(|error| {
                anyhow!(
                    "failed to validate staged Mihomo config with `{}`: {error}",
                    self.mihomo_target_binary_path
                )
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            if detail.is_empty() {
                anyhow::bail!("Mihomo rejected staged config with {}", output.status);
            }
            anyhow::bail!("Mihomo rejected staged config: {detail}");
        }
        Ok(())
    }

    pub fn validate_installed_config(&self) -> Result<StageStatus> {
        self.validate_mihomo_config(Path::new(&self.mihomo_target_config_path))?;
        Ok(StageStatus::Installed)
    }

    fn backup_current_config(&self) -> Result<Option<NamedTempFile>> {
        let target = Path::new(&self.mihomo_target_config_path);
        if !target.exists() {
            return Ok(None);
        }
        let backup = NamedTempFile::new_in(&self.mihomo_target_config_root)?;
        fs::copy(target, backup.path())?;
        fs::set_permissions(backup.path(), fs::Permissions::from_mode(0o600))?;
        backup.as_file().sync_all()?;
        Ok(Some(backup))
    }

    fn restore_config_backup(&self, backup: Option<NamedTempFile>) -> Result<bool> {
        let target = Path::new(&self.mihomo_target_config_path);
        match backup {
            Some(backup) => {
                backup.persist(target).map_err(|error| error.error)?;
                Ok(true)
            }
            None => {
                if target.exists() {
                    fs::remove_file(target)?;
                }
                Ok(false)
            }
        }
    }

    async fn restart_and_verify_with_program(
        &self,
        systemctl_program: &Path,
        health_delay: Duration,
    ) -> Result<()> {
        let restarts_before =
            Systemctl::property_u64_with_program(systemctl_program, "mihomo.service", "NRestarts")?;
        Systemctl::with_program(systemctl_program)
            .restart("mihomo.service")
            .execute()?;
        if !health_delay.is_zero() {
            tokio::time::sleep(health_delay).await;
        }
        if !Systemctl::is_active_with_program(systemctl_program, "mihomo.service") {
            anyhow::bail!("mihomo.service did not remain active after restart");
        }
        let restarts_after =
            Systemctl::property_u64_with_program(systemctl_program, "mihomo.service", "NRestarts")?;
        if restarts_after > restarts_before {
            anyhow::bail!(
				"mihomo.service restarted during health verification ({restarts_before} -> {restarts_after})"
			);
        }
        Ok(())
    }

    async fn rollback_config_after_failed_restart(
        &self,
        backup: Option<NamedTempFile>,
        systemctl_program: &Path,
        health_delay: Duration,
    ) -> Result<()> {
        let _ = Systemctl::with_program(systemctl_program)
            .stop("mihomo.service")
            .execute();
        let had_previous_config = self.restore_config_backup(backup)?;
        if !had_previous_config {
            return Ok(());
        }
        Systemctl::with_program(systemctl_program)
            .reset_failed("mihomo.service")
            .execute()?;
        self.restart_and_verify_with_program(systemctl_program, health_delay)
            .await
    }

    pub(super) async fn update_config_and_restart_with_program(
        &self,
        client: &Client,
        systemctl_program: &Path,
        health_delay: Duration,
    ) -> Result<StageStatus> {
        let backup = self.backup_current_config()?;
        self.update_config(client).await?;
        if let Err(restart_error) = self
            .restart_and_verify_with_program(systemctl_program, health_delay)
            .await
        {
            return match self
				.rollback_config_after_failed_restart(
					backup,
					systemctl_program,
					health_delay,
				)
				.await
			{
				Ok(()) => Err(anyhow!(
					"updated config failed service health verification and was rolled back: {restart_error:#}"
				)),
				Err(recovery_error) => Err(anyhow!(
					"updated config failed service health verification ({restart_error:#}); rollback recovery also failed: {recovery_error:#}"
				)),
			};
        }
        Ok(StageStatus::Installed)
    }

    pub async fn update_config_and_restart(&self, client: &Client) -> Result<StageStatus> {
        self.update_config_and_restart_with_program(
            client,
            Path::new("systemctl"),
            SERVICE_HEALTH_DELAY,
        )
        .await
    }

    pub(super) fn apply_existing_config_atomically(&self) -> Result<bool> {
        self.ensure_config_root_secure()?;
        let target = Path::new(&self.mihomo_target_config_path);
        let staged = NamedTempFile::new_in(&self.mihomo_target_config_root)?;
        fs::copy(target, staged.path())?;
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o600))?;
        let changed = apply_mihomo_override(
            staged.path().to_string_lossy().as_ref(),
            &self.config.mihomo_config,
        )?;
        self.validate_mihomo_config(staged.path())?;
        if changed {
            staged.as_file().sync_all()?;
            staged.persist(target).map_err(|error| error.error)?;
        } else {
            fs::set_permissions(target, fs::Permissions::from_mode(0o600))?;
        }
        Ok(changed)
    }

    async fn download_and_install_config(
        &self,
        client: &Client,
        require_validation: bool,
    ) -> Result<()> {
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
        if require_validation || Path::new(&self.mihomo_target_binary_path).exists() {
            self.validate_mihomo_config(staged.path())?;
        }
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

        self.download_and_install_config(client, false).await?;
        Ok(StageStatus::Installed)
    }

    pub async fn update_config(&self, client: &Client) -> Result<StageStatus> {
        self.download_and_install_config(client, true).await?;
        println!(
            "{} Updated and applied config overrides",
            DETAIL_PREFIX.cyan()
        );
        Ok(StageStatus::Installed)
    }

    pub async fn apply(&self) -> Result<()> {
        let backup = self.backup_current_config()?;

        // Apply mihomo config override
        self.apply_existing_config_atomically()?;
        println!(
            "{} Applied mihomo config overrides",
            self.prefix.green().bold()
        );

        // Restart Mihomo and restore the previous config if the service does not stay healthy.
        if let Err(restart_error) = self
            .restart_and_verify_with_program(Path::new("systemctl"), SERVICE_HEALTH_DELAY)
            .await
        {
            return match self
				.rollback_config_after_failed_restart(
					backup,
					Path::new("systemctl"),
					SERVICE_HEALTH_DELAY,
				)
				.await
			{
				Ok(()) => Err(anyhow!(
					"applied config failed service health verification and was rolled back: {restart_error:#}"
				)),
				Err(recovery_error) => Err(anyhow!(
					"applied config failed service health verification ({restart_error:#}); rollback recovery also failed: {recovery_error:#}"
				)),
			};
        }
        println!("{} Restarted mihomo.service", self.prefix.green().bold());
        Ok(())
    }
}
