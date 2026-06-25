use std::process::{Command, ExitStatus};

use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemdScope {
    User,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemdAction {
    Enable,
    Start,
    Stop,
    Restart,
    Status,
    Disable,
    DaemonReload,
    ResetFailed,
}

pub struct Systemctl {
    systemctl: Command,
    scope: SystemdScope,
}

impl Systemctl {
    pub fn new() -> Self {
        Self::with_scope(SystemdScope::User)
    }

    pub fn with_scope(scope: SystemdScope) -> Self {
        Self {
            systemctl: Command::new("systemctl"),
            scope,
        }
    }

    pub fn enable(&mut self, service: &str) -> &mut Self {
        self.push_systemctl_args(SystemdAction::Enable, service);
        self
    }

    pub fn start(&mut self, service: &str) -> &mut Self {
        self.push_systemctl_args(SystemdAction::Start, service);
        self
    }

    pub fn stop(&mut self, service: &str) -> &mut Self {
        self.push_systemctl_args(SystemdAction::Stop, service);
        self
    }

    pub fn restart(&mut self, service: &str) -> &mut Self {
        self.push_systemctl_args(SystemdAction::Restart, service);
        self
    }

    pub fn status(&mut self, service: &str) -> &mut Self {
        self.push_systemctl_args(SystemdAction::Status, service);
        self
    }

    pub fn disable(&mut self, service: &str) -> &mut Self {
        self.push_systemctl_args(SystemdAction::Disable, service);
        self
    }

    pub fn daemon_reload(&mut self) -> &mut Self {
        self.push_systemctl_args(SystemdAction::DaemonReload, "");
        self
    }

    pub fn reset_failed(&mut self) -> &mut Self {
        self.push_systemctl_args(SystemdAction::ResetFailed, "");
        self
    }

    pub fn execute(&mut self) -> Result<ExitStatus> {
        let status = self
            .systemctl
            .spawn()?
            .wait()
            .with_context(|| "failed to execute systemctl")?;
        if status.success() {
            Ok(status)
        } else {
            Err(anyhow!("systemctl exited with {}", status))
        }
    }

    /// Returns `true` if the given user service is currently active.
    pub fn is_active(service: &str) -> bool {
        Command::new("systemctl")
            .arg("--user")
            .arg("is-active")
            .arg("--quiet")
            .arg(service)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Returns `true` if the given user service is enabled for autostart.
    pub fn is_enabled(service: &str) -> bool {
        Command::new("systemctl")
            .arg("--user")
            .arg("is-enabled")
            .arg("--quiet")
            .arg(service)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn push_systemctl_args(&mut self, action: SystemdAction, service: &str) {
        for arg in systemctl_args(self.scope, action, service) {
            self.systemctl.arg(arg);
        }
    }
}

pub fn systemctl_args(scope: SystemdScope, action: SystemdAction, service: &str) -> Vec<String> {
    let mut args = Vec::new();
    if scope == SystemdScope::User {
        args.push("--user".to_string());
    }

    args.push(
        match action {
            SystemdAction::Enable => "enable",
            SystemdAction::Start => "start",
            SystemdAction::Stop => "stop",
            SystemdAction::Restart => "restart",
            SystemdAction::Status => "status",
            SystemdAction::Disable => "disable",
            SystemdAction::DaemonReload => "daemon-reload",
            SystemdAction::ResetFailed => "reset-failed",
        }
        .to_string(),
    );
    if !service.is_empty() {
        args.push(service.to_string());
    }
    args
}

pub fn journalctl_args(
    scope: SystemdScope,
    service: &str,
    lines: usize,
    follow: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if scope == SystemdScope::User {
        args.push("--user".to_string());
    }
    args.extend([
        "-xeu".to_string(),
        service.to_string(),
        "-n".to_string(),
        lines.to_string(),
    ]);
    if follow {
        args.push("-f".to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemctl_user_scope_includes_user_flag() {
        assert_eq!(
            systemctl_args(SystemdScope::User, SystemdAction::Restart, "mihomo.service"),
            vec!["--user", "restart", "mihomo.service"]
        );
    }

    #[test]
    fn systemctl_system_scope_omits_user_flag() {
        assert_eq!(
            systemctl_args(
                SystemdScope::System,
                SystemdAction::Enable,
                "mihomo.service"
            ),
            vec!["enable", "mihomo.service"]
        );
    }

    #[test]
    fn journalctl_user_scope_includes_user_flag() {
        assert_eq!(
            journalctl_args(SystemdScope::User, "mihomo.service", 10, true),
            vec!["--user", "-xeu", "mihomo.service", "-n", "10", "-f"]
        );
    }

    #[test]
    fn journalctl_system_scope_omits_user_flag() {
        assert_eq!(
            journalctl_args(SystemdScope::System, "mihomo.service", 50, false),
            vec!["-xeu", "mihomo.service", "-n", "50"]
        );
    }
}
