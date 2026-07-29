use crate::config::Config;
use crate::systemctl::Systemctl;
use anyhow::{bail, Result};
use std::{fs, os::unix::fs::PermissionsExt};

pub const SERVICE_PATH: &str = "/etc/systemd/system/mihoto-update.service";
pub const TIMER_PATH: &str = "/etc/systemd/system/mihoto-update.timer";

pub fn render_service(config: &Config, config_path: &str) -> String {
    format!("[Unit]\nDescription=Update Mihomo through mihoto\n\n[Service]\nType=oneshot\nExecStart={} --config {} update\n", config.mihoto_binary_path, config_path)
}
pub fn render_timer(interval: u16) -> Result<String> {
    if interval == 0 {
        bail!("auto-update is disabled because auto_update_interval is 0");
    }
    if interval > 24 {
        bail!("auto_update_interval must be between 0 and 24 hours");
    }
    Ok(format!("[Unit]\nDescription=Periodic mihoto update\n\n[Timer]\nOnBootSec=15min\nOnUnitActiveSec={}h\nRandomizedDelaySec=5min\nUnit=mihoto-update.service\n\n[Install]\nWantedBy=timers.target\n", interval))
}
fn write_unit(path: &str, content: &str) -> Result<()> {
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    Ok(())
}
pub fn enable(config: &Config, config_path: &str) -> Result<()> {
    write_unit(SERVICE_PATH, &render_service(config, config_path))?;
    write_unit(TIMER_PATH, &render_timer(config.auto_update_interval)?)?;
    Systemctl::new().daemon_reload().execute()?;
    Systemctl::new()
        .enable_now("mihoto-update.timer")
        .execute()?;
    Ok(())
}
pub fn disable() -> Result<()> {
    Systemctl::new()
        .disable_now("mihoto-update.timer")
        .execute()?;
    Ok(())
}
pub fn status() -> Result<()> {
    Systemctl::new().status("mihoto-update.timer").execute()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }
}
