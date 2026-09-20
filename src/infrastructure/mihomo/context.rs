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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::config_store::write_config;

    #[test]
    fn derived_paths_follow_the_validated_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config {
            remote_config_url: "https://example.com/config.yaml".to_string(),
            mihomo_binary_path: dir.path().join("bin/mihomo").to_string_lossy().into_owned(),
            mihomo_config_root: dir.path().join("etc/mihomo").to_string_lossy().into_owned(),
            ..Config::default()
        };
        config.mihomo_config.external_ui = Some("dashboard".to_string());
        let config_path = dir.path().join("mihoto.toml");
        write_config(&config, &config_path).unwrap();

        let mihoto = Mihoto::new(config_path.to_str().unwrap()).unwrap();
        assert_eq!(mihoto.mihomo_target_binary_path, config.mihomo_binary_path);
        assert_eq!(mihoto.mihomo_target_config_root, config.mihomo_config_root);
        assert_eq!(
            mihoto.mihomo_target_config_path,
            format!("{}/config.yaml", config.mihomo_config_root)
        );
        assert_eq!(
            mihoto.external_ui_target_dir().unwrap(),
            PathBuf::from(&config.mihomo_config_root).join("dashboard")
        );
        assert_eq!(mihoto.prefix, "mihoto:");
    }

    #[test]
    fn unset_and_absolute_dashboard_paths_are_preserved() {
        let mut config = Config::default();
        config.mihomo_config.external_ui = None;
        assert!(Mihoto::from_config(config.clone())
            .external_ui_target_dir()
            .is_none());
        config.mihomo_config.external_ui = Some("/srv/dashboard".to_string());
        assert_eq!(
            Mihoto::from_config(config)
                .external_ui_target_dir()
                .unwrap(),
            PathBuf::from("/srv/dashboard")
        );
    }
}
