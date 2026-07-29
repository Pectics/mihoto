use std::process::{Command, ExitStatus};

use anyhow::{Context, Result};

pub struct Systemctl {
    systemctl: Command,
}

impl Systemctl {
    pub fn new() -> Self {
        Self {
            systemctl: Command::new("systemctl"),
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

    pub fn disable(&mut self, service: &str) -> &mut Self {
        self.systemctl.arg("disable").arg(service);
        self
    }

    pub fn daemon_reload(&mut self) -> &mut Self {
        self.systemctl.arg("daemon-reload");
        self
    }

    pub fn reset_failed(&mut self) -> &mut Self {
        self.systemctl.arg("reset-failed");
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
        Command::new("systemctl")
            .arg("is-active")
            .arg("--quiet")
            .arg(service)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Returns `true` if the given system service is enabled for autostart.
    pub fn is_enabled(service: &str) -> bool {
        Command::new("systemctl")
            .arg("is-enabled")
            .arg("--quiet")
            .arg(service)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
