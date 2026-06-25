use crate::config::DeploymentBackend;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MIHOTO_MANAGED_MARKER: &str = "# X-Mihoto-Managed: true";
pub const MIHOTO_SERVICE_USER: &str = "mihomo";
pub const MIHOTO_SERVICE_GROUP: &str = "mihomo";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ServiceScope {
    User,
    System,
}

impl ServiceScope {
    fn backend_name(self) -> &'static str {
        match self {
            ServiceScope::User => "systemd-user",
            ServiceScope::System => "systemd-system",
        }
    }

    fn wanted_by(self) -> &'static str {
        match self {
            ServiceScope::User => "default.target",
            ServiceScope::System => "multi-user.target",
        }
    }
}

pub struct ServiceUnitSpec<'a> {
    pub scope: ServiceScope,
    pub binary_path: &'a str,
    pub config_root: &'a str,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ExistingUnit {
    Missing,
    Managed,
    Unmanaged,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnitWritePlan {
    pub existing: ExistingUnit,
    pub backup_required: bool,
}

pub struct ServiceIdentity {
    pub user: &'static str,
    pub group: &'static str,
    pub group_create_command: String,
    pub user_create_command: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct DeploymentRollbackRecord {
    pub id: String,
    pub previous_backend: DeploymentBackend,
    pub target_backend: DeploymentBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_backup_path: Option<String>,
}

pub fn render_mihomo_service_unit(spec: &ServiceUnitSpec<'_>) -> String {
    let system_identity = if spec.scope == ServiceScope::System {
        format!(
            "User={}\nGroup={}\n",
            MIHOTO_SERVICE_USER, MIHOTO_SERVICE_GROUP
        )
    } else {
        String::new()
    };
    let system_hardening = if spec.scope == ServiceScope::System {
        "\
StateDirectory=mihoto
RuntimeDirectory=mihoto
DevicePolicy=closed
DeviceAllow=/dev/null rw
DeviceAllow=/dev/net/tun rw
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
"
    } else {
        ""
    };

    format!(
        "{managed}
# X-Mihoto-Backend: {backend}
# X-Mihoto-ConfigRoot: {config_root}
[Unit]
Description=Mihomo daemon managed by Mihoto
After=network.target NetworkManager.service systemd-networkd.service iwd.service

[Service]
Type=simple
LimitNPROC=4096
LimitNOFILE=65536
Restart=always
ExecStartPre=/usr/bin/sleep 1s
{system_identity}ExecStart={binary_path} -d {config_root}
ExecReload=/bin/kill -HUP $MAINPID
{system_hardening}
[Install]
WantedBy={wanted_by}
",
        managed = MIHOTO_MANAGED_MARKER,
        backend = spec.scope.backend_name(),
        config_root = spec.config_root,
        binary_path = spec.binary_path,
        system_identity = system_identity,
        system_hardening = system_hardening,
        wanted_by = spec.scope.wanted_by(),
    )
}

pub fn classify_existing_unit(existing: Option<&str>) -> ExistingUnit {
    match existing {
        None => ExistingUnit::Missing,
        Some(content)
            if content
                .lines()
                .any(|line| line.trim() == MIHOTO_MANAGED_MARKER) =>
        {
            ExistingUnit::Managed
        }
        Some(_) => ExistingUnit::Unmanaged,
    }
}

pub fn plan_unit_write(existing: Option<&str>, adopt_existing_unit: bool) -> Result<UnitWritePlan> {
    let existing = classify_existing_unit(existing);
    if existing == ExistingUnit::Unmanaged && !adopt_existing_unit {
        return Err(anyhow!(
            "refusing to overwrite unmanaged mihomo.service; pass --adopt-existing-unit to back it up and adopt it"
        ));
    }
    Ok(UnitWritePlan {
        existing,
        backup_required: existing == ExistingUnit::Unmanaged,
    })
}

pub fn system_service_identity() -> ServiceIdentity {
    ServiceIdentity {
        user: MIHOTO_SERVICE_USER,
        group: MIHOTO_SERVICE_GROUP,
        group_create_command: "getent group mihomo >/dev/null || groupadd --system mihomo"
            .to_string(),
        user_create_command:
            "id -u mihomo >/dev/null 2>&1 || useradd --system --no-create-home --gid mihomo --shell /usr/sbin/nologin mihomo"
                .to_string(),
    }
}

pub fn deployment_records_dir(profile_state_root: &str) -> PathBuf {
    Path::new(profile_state_root).join("deployments")
}

pub fn create_rollback_record(
    profile_state_root: &str,
    previous_backend: DeploymentBackend,
    target_backend: DeploymentBackend,
    unit_backup_path: Option<String>,
) -> Result<DeploymentRollbackRecord> {
    let record = DeploymentRollbackRecord {
        id: deployment_record_id(),
        previous_backend,
        target_backend,
        unit_backup_path,
    };
    write_rollback_record(profile_state_root, &record)?;
    Ok(record)
}

pub fn write_rollback_record(
    profile_state_root: &str,
    record: &DeploymentRollbackRecord,
) -> Result<PathBuf> {
    let dir = deployment_records_dir(profile_state_root);
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create deployment records dir `{}`",
            dir.display()
        )
    })?;
    let path = dir.join(format!("{}.toml", record.id));
    fs::write(&path, toml::to_string(record)?.as_bytes()).with_context(|| {
        format!(
            "failed to write deployment rollback record `{}`",
            path.display()
        )
    })?;
    Ok(path)
}

pub fn read_rollback_record(
    profile_state_root: &str,
    id: Option<&str>,
) -> Result<DeploymentRollbackRecord> {
    let path = match id {
        Some(id) => deployment_records_dir(profile_state_root).join(format!("{id}.toml")),
        None => latest_rollback_record_path(profile_state_root)?,
    };
    let raw = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read deployment rollback record `{}`",
            path.display()
        )
    })?;
    Ok(toml::from_str(&raw)?)
}

fn latest_rollback_record_path(profile_state_root: &str) -> Result<PathBuf> {
    let dir = deployment_records_dir(profile_state_root);
    let mut entries = fs::read_dir(&dir)
        .with_context(|| format!("failed to read deployment records dir `{}`", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    entries.sort();
    entries.pop().ok_or_else(|| {
        anyhow!(
            "no deployment rollback records found in `{}`",
            dir.display()
        )
    })
}

fn deployment_record_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("deployment-{millis}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_system_mihomo_unit_with_mihoto_marker_and_hardening() {
        let unit = render_mihomo_service_unit(&ServiceUnitSpec {
            scope: ServiceScope::System,
            binary_path: "/usr/local/libexec/mihoto/mihomo",
            config_root: "/etc/mihoto",
        });

        assert!(unit.contains("# X-Mihoto-Managed: true"));
        assert!(unit.contains("# X-Mihoto-Backend: systemd-system"));
        assert!(unit.contains("# X-Mihoto-ConfigRoot: /etc/mihoto"));
        assert!(unit.contains("User=mihomo"));
        assert!(unit.contains("Group=mihomo"));
        assert!(unit.contains("ExecStart=/usr/local/libexec/mihoto/mihomo -d /etc/mihoto"));
        assert!(unit.contains("CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE"));
        assert!(unit.contains("DeviceAllow=/dev/net/tun rw"));
        assert!(unit.contains("StateDirectory=mihoto"));
        assert!(unit.contains("RuntimeDirectory=mihoto"));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn renders_user_mihomo_unit_without_static_service_user() {
        let unit = render_mihomo_service_unit(&ServiceUnitSpec {
            scope: ServiceScope::User,
            binary_path: "/home/me/.local/bin/mihomo",
            config_root: "/home/me/.config/mihomo",
        });

        assert!(unit.contains("# X-Mihoto-Managed: true"));
        assert!(unit.contains("# X-Mihoto-Backend: systemd-user"));
        assert!(!unit.contains("User=mihomo"));
        assert!(!unit.contains("Group=mihomo"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn unmanaged_existing_unit_requires_explicit_adoption() {
        let existing = "[Unit]\nDescription=hand written mihomo\n";

        let err = plan_unit_write(Some(existing), false)
            .expect_err("unmanaged unit must not be overwritten implicitly");

        assert!(err.to_string().contains("unmanaged mihomo.service"));

        let plan = plan_unit_write(Some(existing), true).expect("explicit adoption is allowed");
        assert_eq!(plan.existing, ExistingUnit::Unmanaged);
        assert!(plan.backup_required);
    }

    #[test]
    fn managed_existing_unit_can_be_updated_without_backup() {
        let existing = "# X-Mihoto-Managed: true\n[Unit]\nDescription=old\n";

        let plan = plan_unit_write(Some(existing), false).unwrap();

        assert_eq!(plan.existing, ExistingUnit::Managed);
        assert!(!plan.backup_required);
    }

    #[test]
    fn system_service_identity_uses_static_mihomo_user_and_group() {
        let identity = system_service_identity();

        assert_eq!(identity.user, "mihomo");
        assert_eq!(identity.group, "mihomo");
        assert!(identity
            .group_create_command
            .contains("groupadd --system mihomo"));
        assert!(identity.user_create_command.contains("useradd --system"));
        assert!(identity.user_create_command.contains("--gid mihomo"));
    }
}
