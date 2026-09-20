use crate::infrastructure::filesystem::delete_file;
use crate::infrastructure::systemctl::Systemctl;
use crate::infrastructure::timer;

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};
use colored::Colorize;

use super::Mihoto;

impl Mihoto {
    pub fn uninstall(&self, purge: bool, manager_config_path: &Path) -> Result<()> {
        self.uninstall_with_paths(
            purge,
            manager_config_path,
            Path::new(timer::SERVICE_PATH),
            Path::new(timer::TIMER_PATH),
            Path::new("systemctl"),
        )
    }

    pub(super) fn uninstall_with_paths(
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
}

pub(super) fn validate_safe_purge_target(label: &str, path: &Path) -> Result<()> {
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
