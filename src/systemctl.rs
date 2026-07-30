use std::{
    path::Path,
    process::{Command, ExitStatus},
};

use anyhow::{Context, Result};

pub struct Systemctl {
    systemctl: Command,
}

impl Systemctl {
    pub fn new() -> Self {
        Self::with_program(Path::new("systemctl"))
    }

    pub fn with_program(program: &Path) -> Self {
        Self {
            systemctl: Command::new(program),
        }
    }

    pub fn enable(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("enable").arg(service);
        self
    }

    pub fn enable_now(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("enable").arg("--now").arg(service);
        self
    }

    pub fn disable_now(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("disable").arg("--now").arg(service);
        self
    }

    pub fn start(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("start").arg(service);
        self
    }

    pub fn stop(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("stop").arg(service);
        self
    }

    pub fn restart(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("restart").arg(service);
        self
    }

    pub fn status(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("status").arg(service);
        self
    }

    pub fn daemon_reload(&mut self) -> &mut Self {
        self.systemctl.arg("daemon-reload");
        self
    }

    pub fn reset_failed(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("reset-failed").arg(service);
        self
    }

    pub fn execute(&mut self) -> Result<ExitStatus> {
        let status = self
            .systemctl
            .spawn()?
            .wait()
            .with_context(|| "failed to execute systemctl")?;
        if !status.success() {
            anyhow::bail!("systemctl exited with {status}");
        }
        Ok(status)
    }

    /// Returns `true` if the given system service is currently active.
    pub fn is_active(service: &str) -> bool {
        Self::is_active_with_program(Path::new("systemctl"), service)
    }

    pub fn is_active_with_program(program: &Path, service: &str) -> bool {
        Command::new(program)
            .arg("is-active")
            .arg("--quiet")
            .arg(service)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Returns `true` if the given system service is enabled for autostart.
    pub fn is_enabled(service: &str) -> bool {
        Self::is_enabled_with_program(Path::new("systemctl"), service)
    }

    pub fn is_enabled_with_program(program: &Path, service: &str) -> bool {
        Command::new(program)
            .arg("is-enabled")
            .arg("--quiet")
            .arg(service)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn property_u64_with_program(program: &Path, service: &str, property: &str) -> Result<u64> {
        let output = Command::new(program)
            .arg("show")
            .arg(service)
            .arg("--property")
            .arg(property)
            .arg("--value")
            .output()
            .with_context(|| format!("failed to query systemd property {property}"))?;
        if !output.status.success() {
            anyhow::bail!("systemctl show exited with {}", output.status);
        }
        let value = String::from_utf8_lossy(&output.stdout);
        value
            .trim()
            .parse()
            .with_context(|| format!("invalid systemd {property} value `{}`", value.trim()))
    }
}
