use crate::config::DeploymentBackend;
use crate::deployment::MIHOTO_MANAGED_MARKER;
use crate::systemctl::SystemdScope;

use std::path::PathBuf;

pub const UPDATE_SERVICE_UNIT: &str = "mihoto-update.service";
pub const UPDATE_TIMER_UNIT: &str = "mihoto-update.timer";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TimerScope {
    User,
    System,
}

impl TimerScope {
    pub fn from_deployment_backend(backend: DeploymentBackend) -> Self {
        match backend {
            DeploymentBackend::SystemdUser => TimerScope::User,
            DeploymentBackend::SystemdSystem => TimerScope::System,
        }
    }

    pub fn systemd_scope(self) -> SystemdScope {
        match self {
            TimerScope::User => SystemdScope::User,
            TimerScope::System => SystemdScope::System,
        }
    }
}

pub struct UpdateServiceSpec<'a> {
    pub mihoro_bin: &'a str,
    pub config_path: &'a str,
}

pub struct UpdateTimerSpec<'a> {
    pub on_calendar: &'a str,
    pub persistent: bool,
    pub randomized_delay_sec: Option<&'a str>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TimerInstallPaths {
    pub service_path: PathBuf,
    pub timer_path: PathBuf,
    pub scope: TimerScope,
}

impl TimerInstallPaths {
    pub fn for_backend(backend: DeploymentBackend, user_systemd_root: &str) -> Self {
        let scope = TimerScope::from_deployment_backend(backend);
        let root = match scope {
            TimerScope::User => PathBuf::from(user_systemd_root),
            TimerScope::System => PathBuf::from("/etc/systemd/system"),
        };
        Self {
            service_path: root.join(UPDATE_SERVICE_UNIT),
            timer_path: root.join(UPDATE_TIMER_UNIT),
            scope,
        }
    }
}

pub fn render_update_service_unit(spec: &UpdateServiceSpec<'_>) -> String {
    format!(
        "{managed}
[Unit]
Description=Update Mihomo artifacts managed by Mihoto

[Service]
Type=oneshot
ExecStart={mihoro_bin} --mihoro-config {config_path} update --all
",
        managed = MIHOTO_MANAGED_MARKER,
        mihoro_bin = spec.mihoro_bin,
        config_path = spec.config_path,
    )
}

pub fn render_update_timer_unit(spec: &UpdateTimerSpec<'_>) -> String {
    let randomized_delay = spec
        .randomized_delay_sec
        .map(|value| format!("RandomizedDelaySec={value}\n"))
        .unwrap_or_default();
    format!(
        "{managed}
[Unit]
Description=Scheduled Mihoto update

[Timer]
OnCalendar={on_calendar}
Persistent={persistent}
{randomized_delay}
[Install]
WantedBy=timers.target
",
        managed = MIHOTO_MANAGED_MARKER,
        on_calendar = spec.on_calendar,
        persistent = if spec.persistent { "true" } else { "false" },
        randomized_delay = randomized_delay,
    )
}

pub fn managed_unit_content(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim() == MIHOTO_MANAGED_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_update_service_with_config_path() {
        let unit = render_update_service_unit(&UpdateServiceSpec {
            mihoro_bin: "/usr/local/bin/mihoro",
            config_path: "/etc/mihoro.toml",
        });

        assert!(unit.contains("# X-Mihoto-Managed: true"));
        assert!(unit.contains("Type=oneshot"));
        assert!(unit.contains(
            "ExecStart=/usr/local/bin/mihoro --mihoro-config /etc/mihoro.toml update --all"
        ));
    }

    #[test]
    fn renders_persistent_timer_with_randomized_delay() {
        let unit = render_update_timer_unit(&UpdateTimerSpec {
            on_calendar: "0/12:00:00",
            persistent: true,
            randomized_delay_sec: Some("15min"),
        });

        assert!(unit.contains("# X-Mihoto-Managed: true"));
        assert!(unit.contains("OnCalendar=0/12:00:00"));
        assert!(unit.contains("Persistent=true"));
        assert!(unit.contains("RandomizedDelaySec=15min"));
        assert!(unit.contains("WantedBy=timers.target"));
    }

    #[test]
    fn derives_timer_paths_from_deployment_backend() {
        let user = TimerInstallPaths::for_backend(
            DeploymentBackend::SystemdUser,
            "/home/me/.config/systemd/user",
        );
        assert_eq!(
            user.service_path,
            PathBuf::from("/home/me/.config/systemd/user/mihoto-update.service")
        );
        assert_eq!(user.scope, TimerScope::User);
        assert_eq!(user.scope.systemd_scope(), SystemdScope::User);

        let system = TimerInstallPaths::for_backend(
            DeploymentBackend::SystemdSystem,
            "/home/me/.config/systemd/user",
        );
        assert_eq!(
            system.timer_path,
            PathBuf::from("/etc/systemd/system/mihoto-update.timer")
        );
        assert_eq!(system.scope.systemd_scope(), SystemdScope::System);
    }
}
