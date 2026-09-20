use crate::infrastructure::download::DETAIL_PREFIX;
use crate::infrastructure::filesystem::create_parent_dir;
use crate::infrastructure::systemctl::Systemctl;
use crate::infrastructure::systemd_unit::escape_exec_arg;

use std::fs;
use std::io::Write;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;

use anyhow::{anyhow, Result};
use colored::Colorize;
use tempfile::NamedTempFile;

use super::{Mihoto, StageStatus};

impl Mihoto {
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

    pub async fn restart_service(&self) -> Result<StageStatus> {
        println!("{} Restarting mihomo.service...", DETAIL_PREFIX.cyan());
        Systemctl::new().restart("mihomo.service").execute()?;
        Ok(StageStatus::Installed)
    }
}

/// Render the systemd unit file content for mihomo.service.
///
/// Reference: https://wiki.metacubex.one/startup/service/
pub(super) fn render_service_string(binary_path: &str, config_root: &str) -> String {
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
        escape_exec_arg(binary_path),
        escape_exec_arg(config_root)
    )
}
