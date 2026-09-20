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
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, fs, os::unix::fs::PermissionsExt};

    fn command_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn builder_preserves_exact_argument_order() {
        let mut command = Systemctl::with_program(Path::new("/custom/systemctl"));
        command
            .enable("one.service")
            .enable_now("two.service")
            .disable_now("three.service")
            .start("four.service")
            .stop("five.service")
            .restart("six.service")
            .status("seven.service")
            .daemon_reload()
            .reset_failed("eight.service");

        assert_eq!(
            command.systemctl.get_program(),
            OsStr::new("/custom/systemctl")
        );
        assert_eq!(
            command
                .systemctl
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            [
                "enable",
                "one.service",
                "enable",
                "--now",
                "two.service",
                "disable",
                "--now",
                "three.service",
                "start",
                "four.service",
                "stop",
                "five.service",
                "restart",
                "six.service",
                "status",
                "seven.service",
                "daemon-reload",
                "reset-failed",
                "eight.service"
            ]
        );
        assert_eq!(
            Systemctl::new().systemctl.get_program(),
            OsStr::new("systemctl")
        );
    }

    #[test]
    fn execute_and_boolean_queries_follow_process_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let success = command_script(dir.path(), "success", "exit 0");
        let failure = command_script(dir.path(), "failure", "exit 7");
        let missing = dir.path().join("missing");

        assert!(Systemctl::with_program(&success)
            .execute()
            .unwrap()
            .success());
        assert!(Systemctl::with_program(&failure).execute().is_err());
        assert!(Systemctl::with_program(&missing).execute().is_err());
        assert!(Systemctl::is_active_with_program(
            &success,
            "mihomo.service"
        ));
        assert!(!Systemctl::is_active_with_program(
            &failure,
            "mihomo.service"
        ));
        assert!(!Systemctl::is_active_with_program(
            &missing,
            "mihomo.service"
        ));
        assert!(Systemctl::is_enabled_with_program(
            &success,
            "mihomo.service"
        ));
        assert!(!Systemctl::is_enabled_with_program(
            &failure,
            "mihomo.service"
        ));
        assert!(!Systemctl::is_enabled_with_program(
            &missing,
            "mihomo.service"
        ));
    }

    #[test]
    fn property_query_accepts_u64_and_reports_every_failure_kind() {
        let dir = tempfile::tempdir().unwrap();
        let valid = command_script(
			dir.path(),
			"valid",
			"test \"$1\" = show || exit 2\ntest \"$2\" = mihomo.service || exit 3\ntest \"$3\" = --property || exit 4\ntest \"$4\" = NRestarts || exit 5\ntest \"$5\" = --value || exit 6\nprintf '42\\n'",
		);
        let invalid = command_script(dir.path(), "invalid", "printf 'many\\n'");
        let failure = command_script(dir.path(), "failure", "exit 9");

        assert_eq!(
            Systemctl::property_u64_with_program(&valid, "mihomo.service", "NRestarts").unwrap(),
            42
        );
        assert!(
            Systemctl::property_u64_with_program(&invalid, "mihomo.service", "NRestarts")
                .unwrap_err()
                .to_string()
                .contains("invalid systemd NRestarts value `many`")
        );
        assert!(
            Systemctl::property_u64_with_program(&failure, "mihomo.service", "NRestarts").is_err()
        );
        assert!(Systemctl::property_u64_with_program(
            &dir.path().join("missing"),
            "mihomo.service",
            "NRestarts"
        )
        .is_err());
    }
}

impl Systemctl {
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
