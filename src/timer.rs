use crate::config::Config;
use crate::systemctl::Systemctl;
use crate::utils::systemd_escape_exec_arg;
use anyhow::{bail, Result};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path};
use tempfile::NamedTempFile;

pub const SERVICE_PATH: &str = "/etc/systemd/system/mihoto-update.service";
pub const TIMER_PATH: &str = "/etc/systemd/system/mihoto-update.timer";

pub fn render_service(config: &Config, config_path: &str) -> String {
    format!(
		"[Unit]\nDescription=Update Mihomo through mihoto\nWants=network-online.target\nAfter=network-online.target\n\n[Service]\nType=oneshot\nUser=root\nExecStart={} --config {} update\n",
		systemd_escape_exec_arg(&config.mihoto_binary_path),
		systemd_escape_exec_arg(config_path)
	)
}
pub fn render_timer(interval: u16) -> Result<String> {
    if interval == 0 {
        bail!("auto-update is disabled because auto_update_interval is 0");
    }
    if interval > 24 {
        bail!("auto_update_interval must be between 0 and 24 hours");
    }
    Ok(format!("[Unit]\nDescription=Periodic mihoto update\n\n[Timer]\nOnBootSec=15min\nOnUnitActiveSec={}h\nRandomizedDelaySec=5min\nPersistent=true\nUnit=mihoto-update.service\n\n[Install]\nWantedBy=timers.target\n", interval))
}
fn write_unit(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("unit path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let mut staged = NamedTempFile::new_in(parent)?;
    staged.write_all(content.as_bytes())?;
    staged
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o644))?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn enable_with_paths(
    config: &Config,
    config_path: &str,
    service_path: &Path,
    timer_path: &Path,
    systemctl_program: &Path,
) -> Result<()> {
    let service = render_service(config, config_path);
    let timer = render_timer(config.auto_update_interval)?;
    write_unit(service_path, &service)?;
    write_unit(timer_path, &timer)?;
    Systemctl::with_program(systemctl_program)
        .daemon_reload()
        .execute()?;
    Systemctl::with_program(systemctl_program)
        .enable_now("mihoto-update.timer")
        .execute()?;
    Ok(())
}

pub fn enable(config: &Config, config_path: &str) -> Result<()> {
    enable_with_paths(
        config,
        config_path,
        Path::new(SERVICE_PATH),
        Path::new(TIMER_PATH),
        Path::new("systemctl"),
    )
}

pub fn disable_with_paths(
    service_path: &Path,
    timer_path: &Path,
    systemctl_program: &Path,
) -> Result<()> {
    let had_files = service_path.exists() || timer_path.exists();
    let known = Systemctl::is_active_with_program(systemctl_program, "mihoto-update.timer")
        || Systemctl::is_enabled_with_program(systemctl_program, "mihoto-update.timer");
    if known {
        Systemctl::with_program(systemctl_program)
            .disable_now("mihoto-update.timer")
            .execute()?;
    }
    if service_path.exists() {
        fs::remove_file(service_path)?;
    }
    if timer_path.exists() {
        fs::remove_file(timer_path)?;
    }
    if known || had_files {
        Systemctl::with_program(systemctl_program)
            .daemon_reload()
            .execute()?;
    }
    Ok(())
}

pub fn disable() -> Result<()> {
    disable_with_paths(
        Path::new(SERVICE_PATH),
        Path::new(TIMER_PATH),
        Path::new("systemctl"),
    )?;
    Ok(())
}

pub fn reconcile(config: &Config, config_path: &str) -> Result<()> {
    if config.auto_update_interval == 0 {
        disable()
    } else {
        enable(config, config_path)
    }
}
pub fn status() -> Result<()> {
    status_with_program(Path::new("systemctl"))
}

pub fn status_with_program(systemctl_program: &Path) -> Result<()> {
    let active = Systemctl::is_active_with_program(systemctl_program, "mihoto-update.timer");
    let enabled = Systemctl::is_enabled_with_program(systemctl_program, "mihoto-update.timer");
    if !active && !enabled {
        println!("mihoto: mihoto-update.timer is disabled");
        return Ok(());
    }
    Systemctl::with_program(systemctl_program)
        .status("mihoto-update.timer")
        .execute()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    fn fake_systemctl(dir: &Path, query_exit: i32) -> PathBuf {
        let program = dir.join("systemctl");
        let log = dir.join("systemctl.log");
        fs::write(
			&program,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  is-active|is-enabled) exit {query_exit} ;;\n  *) exit 0 ;;\nesac\n",
				log.display()
			),
		)
		.unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    #[test]
    fn intervals() {
        assert!(render_timer(0).is_err());
        for n in [1, 12, 24] {
            assert!(render_timer(n)
                .unwrap()
                .contains(&format!("OnUnitActiveSec={n}h")));
        }
        assert!(render_timer(25).is_err());
    }
    #[test]
    fn service_paths() {
        let c = Config {
            mihoto_binary_path: "/opt/mihoto".into(),
            ..Config::default()
        };
        let s = render_service(&c, "/etc/custom.toml");
        assert!(s.contains("ExecStart=/opt/mihoto --config /etc/custom.toml update"));
        assert!(s.contains("User=root"));
        assert!(s.contains("Wants=network-online.target"));
        assert!(s.contains("After=network-online.target"));
        let timer = render_timer(12).unwrap();
        assert!(timer.contains("Persistent=true"));
    }

    #[test]
    fn disabled_interval_does_not_write_partial_units() {
        let dir = tempfile::tempdir().unwrap();
        let service_path = dir.path().join("mihoto-update.service");
        let timer_path = dir.path().join("mihoto-update.timer");
        let config = Config {
            auto_update_interval: 0,
            ..Config::default()
        };

        assert!(enable_with_paths(
            &config,
            "/etc/mihoto.toml",
            &service_path,
            &timer_path,
            Path::new("/bin/false"),
        )
        .is_err());
        assert!(!service_path.exists());
        assert!(!timer_path.exists());
    }

    #[test]
    fn enable_writes_private_units_and_calls_systemctl() {
        let dir = tempfile::tempdir().unwrap();
        let service_path = dir.path().join("mihoto-update.service");
        let timer_path = dir.path().join("mihoto-update.timer");
        let systemctl = fake_systemctl(dir.path(), 1);

        enable_with_paths(
            &Config::default(),
            "/etc/mihoto.toml",
            &service_path,
            &timer_path,
            &systemctl,
        )
        .unwrap();

        for path in [&service_path, &timer_path] {
            assert!(path.exists());
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o644
            );
        }
        let calls = fs::read_to_string(dir.path().join("systemctl.log")).unwrap();
        assert!(calls.contains("daemon-reload"));
        assert!(calls.contains("enable --now mihoto-update.timer"));
    }

    #[test]
    fn disable_removes_stale_units_and_reloads_even_when_timer_is_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let service_path = dir.path().join("mihoto-update.service");
        let timer_path = dir.path().join("mihoto-update.timer");
        fs::write(&service_path, "service").unwrap();
        fs::write(&timer_path, "timer").unwrap();
        let systemctl = fake_systemctl(dir.path(), 1);

        disable_with_paths(&service_path, &timer_path, &systemctl).unwrap();

        assert!(!service_path.exists());
        assert!(!timer_path.exists());
        let calls = fs::read_to_string(dir.path().join("systemctl.log")).unwrap();
        assert!(calls.contains("is-active --quiet mihoto-update.timer"));
        assert!(calls.contains("is-enabled --quiet mihoto-update.timer"));
        assert!(calls.contains("daemon-reload"));
        assert!(!calls.contains("disable --now"));
    }

    #[test]
    fn disable_stops_an_active_timer_before_removing_units() {
        let dir = tempfile::tempdir().unwrap();
        let service_path = dir.path().join("mihoto-update.service");
        let timer_path = dir.path().join("mihoto-update.timer");
        fs::write(&service_path, "service").unwrap();
        fs::write(&timer_path, "timer").unwrap();
        let systemctl = fake_systemctl(dir.path(), 0);

        disable_with_paths(&service_path, &timer_path, &systemctl).unwrap();

        let calls = fs::read_to_string(dir.path().join("systemctl.log")).unwrap();
        let disable = calls.find("disable --now mihoto-update.timer").unwrap();
        let reload = calls.rfind("daemon-reload").unwrap();
        assert!(disable < reload);
    }

    #[test]
    fn status_is_successful_when_timer_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let systemctl = fake_systemctl(dir.path(), 1);
        status_with_program(&systemctl).unwrap();
        let calls = fs::read_to_string(dir.path().join("systemctl.log")).unwrap();
        assert!(calls.contains("is-active --quiet mihoto-update.timer"));
        assert!(calls.contains("is-enabled --quiet mihoto-update.timer"));
        assert!(!calls.contains("status mihoto-update.timer"));
    }
}
