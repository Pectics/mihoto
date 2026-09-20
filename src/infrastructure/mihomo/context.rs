use crate::domain::config::Config;
use crate::domain::ui::resolve_external_ui_path;
use crate::infrastructure::config_store::parse_config;

use std::path::PathBuf;

use anyhow::Result;

use super::Mihoto;

impl Mihoto {
    pub fn new(config_path: &str) -> Result<Mihoto> {
        let config = parse_config(config_path)?;
        Ok(Self::from_config(config))
    }

    /// Build a `Mihoto` from an already-validated `Config`.
    pub fn from_config(config: Config) -> Mihoto {
        Mihoto {
            prefix: String::from("mihoto:"),
            mihomo_target_binary_path: config.mihomo_binary_path.clone(),
            mihomo_target_config_root: config.mihomo_config_root.clone(),
            mihomo_target_config_path: format!("{}/config.yaml", config.mihomo_config_root),
            mihomo_target_service_path: String::from("/etc/systemd/system/mihomo.service"),
            config,
        }
    }

    pub(super) fn external_ui_target_dir(&self) -> Option<PathBuf> {
        self.config
            .mihomo_config
            .external_ui
            .as_deref()
            .map(|external_ui| {
                resolve_external_ui_path(&self.mihomo_target_config_root, external_ui)
            })
    }
}
