pub use crate::mihomo_config::{apply_mihomo_override, MihomoConfig};
use crate::ui::{default_ui, Ui};
use crate::utils::create_parent_dir;

use std::{fs, io::Write, path::Path};

use anyhow::{bail, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

/// Mihomo release channel for automatic binary fetching.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub enum MihomoChannel {
    #[default]
    #[serde(alias = "stable", rename(serialize = "stable"))]
    Stable,
    #[serde(alias = "alpha", rename(serialize = "alpha"))]
    Alpha,
}

/// `mihoto` configurations.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub remote_config_url: String,
    #[serde(default = "default_ui", skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
    pub mihomo_channel: MihomoChannel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_mihomo_binary_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mihomo_arch: Option<String>,
    pub mihoto_binary_path: String,
    pub mihomo_binary_path: String,
    pub mihomo_config_root: String,
    pub mihoto_user_agent: String,
    pub auto_update_interval: u16,
    pub mihomo_config: MihomoConfig,
}

// Serde defaults for Config
impl Default for Config {
    fn default() -> Self {
        Config {
            ui: default_ui(),
            remote_mihomo_binary_url: None,
            mihomo_channel: MihomoChannel::default(),
            mihomo_arch: None,
            remote_config_url: String::from(""),
            mihoto_binary_path: String::from("/usr/local/bin/mihoto"),
            mihomo_binary_path: String::from("/usr/local/bin/mihomo"),
            mihomo_config_root: String::from("/etc/mihomo"),
            mihoto_user_agent: String::from("mihoto"),
            auto_update_interval: 12,
            mihomo_config: MihomoConfig::default(),
        }
    }
}

impl Config {
    pub fn new() -> Config {
        Config::default()
    }

    /// Read raw config string from path and parse with crate toml.
    pub fn setup_from(path: &str) -> Result<Config> {
        let raw_config = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&raw_config)?;
        Ok(config)
    }

    pub fn write(&mut self, path: &Path) -> Result<()> {
        let mut serialized_config = toml::to_string(&self)?;
        serialized_config.push_str(DEFAULT_CONFIG_TEMPLATE_COMMENTS);
        create_parent_dir(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("config path has no parent: {}", path.display()))?;
        let mut staged = NamedTempFile::new_in(parent)?;
        staged.write_all(serialized_config.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        staged.as_file().sync_all()?;
        staged.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

const DEFAULT_CONFIG_TEMPLATE_COMMENTS: &str = r#"
# Mihoto keeps the downloaded subscription as the source for omitted Mihomo fields.
# Add only the local overrides you need under [mihomo_config].
#
# Basic example:
# [mihomo_config]
# mode = "rule"
# mixed_port = 7890
# allow_lan = false
# external_ui = "ui"
#
# TUN example:
# [mihomo_config.tun]
# enable = true
# stack = "mixed"
# auto_route = true
# auto_detect_interface = true
# dns_hijack = ["any:53"]
#
# DNS example:
# [mihomo_config.dns]
# enable = true
# enhanced_mode = "fake-ip"
# nameserver = ["1.1.1.1"]
#
# See the Mihomo support matrix and official documentation for the complete
# list of supported stable non-dynamic fields.
"#;

pub fn validate_manager_config_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("manager config path must be absolute: {}", path.display());
    }
    if path.file_name().is_none() {
        bail!("manager config path must name a file: {}", path.display());
    }
    Ok(())
}

/// Load config from path without validation.  Returns `Ok(None)` if the file does not exist.
pub fn load_config(path: &str) -> Result<Option<Config>> {
    let config_path = Path::new(path);
    if !config_path.exists() {
        return Ok(None);
    }
    Ok(Some(Config::setup_from(path)?))
}

/// Write default config to path if it does not exist.  Returns `true` if the file was created.
pub fn write_default_if_missing(path: &str) -> Result<bool> {
    let config_path = Path::new(path);
    create_parent_dir(config_path)?;
    if config_path.exists() {
        return Ok(false);
    }
    Config::new().write(config_path)?;
    Ok(true)
}

/// Validate that required config fields are non-empty.
pub fn validate_config(config: &Config) -> Result<()> {
    let required_fields = [
        ("remote_config_url", &config.remote_config_url),
        ("mihomo_binary_path", &config.mihomo_binary_path),
        ("mihomo_config_root", &config.mihomo_config_root),
        ("mihoto_binary_path", &config.mihoto_binary_path),
    ];
    for (field, value) in required_fields.iter() {
        if value.is_empty() {
            bail!("`{field}` undefined");
        }
    }
    for (field, value) in [
        ("mihoto_binary_path", &config.mihoto_binary_path),
        ("mihomo_binary_path", &config.mihomo_binary_path),
        ("mihomo_config_root", &config.mihomo_config_root),
    ] {
        if !Path::new(value).is_absolute() {
            bail!("`{field}` must be an absolute path");
        }
    }
    if config.auto_update_interval > 24 {
        bail!("`auto_update_interval` must be between 0 and 24 hours");
    }
    Ok(())
}

/// Tries to parse mihoto config as toml from path.
///
/// * If config file does not exist, returns an error directing the user to run `mihoto init`.
/// * If found, parses the file and validates required fields.
pub fn parse_config(path: &str) -> Result<Config> {
    let config_path = Path::new(path);

    if !config_path.exists() {
        bail!(
            "config `{}` does not exist; run `mihoto init` first",
            path.underline()
        );
    }

    let config = Config::setup_from(path)?;
    validate_config(&config)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn system_defaults_and_validation() {
        let config = Config::default();
        assert_eq!(config.mihoto_binary_path, "/usr/local/bin/mihoto");
        assert_eq!(config.mihomo_binary_path, "/usr/local/bin/mihomo");
        assert_eq!(config.mihomo_config_root, "/etc/mihomo");
        assert_eq!(config.mihoto_user_agent, "mihoto");
        assert_eq!(config.auto_update_interval, 12);
        assert!(config.mihomo_config.external_controller.is_none());
        let mut relative = config.clone();
        relative.mihomo_config_root = "relative".into();
        assert!(validate_config(&relative).is_err());
        let mut too_long = config;
        too_long.auto_update_interval = 25;
        assert!(validate_config(&too_long).is_err());
    }

    #[test]
    fn config_lifecycle_and_validation_cover_public_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/mihoto.toml");
        assert_eq!(Config::new().auto_update_interval, 12);
        assert!(load_config(path.to_str().unwrap()).unwrap().is_none());
        assert!(write_default_if_missing(path.to_str().unwrap()).unwrap());
        assert!(!write_default_if_missing(path.to_str().unwrap()).unwrap());
        assert!(load_config(path.to_str().unwrap()).unwrap().is_some());
        assert!(parse_config(path.to_str().unwrap()).is_err());

        let mut valid = Config {
            remote_config_url: "https://example.test/config.yaml".into(),
            ..Config::default()
        };
        assert!(validate_config(&valid).is_ok());
        assert!(parse_config(path.to_str().unwrap()).is_err());

        for field in [
            "remote_config_url",
            "mihoto_binary_path",
            "mihomo_binary_path",
            "mihomo_config_root",
        ] {
            let mut invalid = valid.clone();
            match field {
                "remote_config_url" => invalid.remote_config_url.clear(),
                "mihoto_binary_path" => invalid.mihoto_binary_path.clear(),
                "mihomo_binary_path" => invalid.mihomo_binary_path.clear(),
                "mihomo_config_root" => invalid.mihomo_config_root.clear(),
                _ => unreachable!(),
            }
            assert!(
                validate_config(&invalid).is_err(),
                "{field} must be required"
            );
        }

        for field in [
            "mihoto_binary_path",
            "mihomo_binary_path",
            "mihomo_config_root",
        ] {
            let mut invalid = valid.clone();
            match field {
                "mihoto_binary_path" => invalid.mihoto_binary_path = "mihoto".into(),
                "mihomo_binary_path" => invalid.mihomo_binary_path = "mihomo".into(),
                "mihomo_config_root" => invalid.mihomo_config_root = "mihomo".into(),
                _ => unreachable!(),
            }
            assert!(
                validate_config(&invalid).is_err(),
                "{field} must be absolute"
            );
        }

        valid.auto_update_interval = 25;
        assert!(validate_config(&valid).is_err());

        let serialized_path = dir.path().join("serialized.toml");
        valid.auto_update_interval = 12;
        valid.write(&serialized_path).unwrap();
        let loaded = Config::setup_from(serialized_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.remote_config_url, valid.remote_config_url);
        assert_eq!(loaded.mihomo_binary_path, valid.mihomo_binary_path);
        assert_eq!(loaded.auto_update_interval, valid.auto_update_interval);
    }

    #[test]
    fn old_fields_are_rejected_and_config_is_private() {
        assert!(
            toml::from_str::<Config>(&format!("{}{} = '/tmp'", "user_", "systemd_root")).is_err()
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mihoto.toml");
        Config::default().write(&path).unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn config_round_trip_preserves_sparse_tun_and_extra_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mihoto.toml");
        let mut config = Config {
            remote_config_url: "https://example.test/subscription.yaml".into(),
            ..Config::default()
        };
        config.mihomo_config.tun = Some(crate::mihomo_config::TunConfig {
            enable: Some(true),
            stack: Some(crate::mihomo_config::TunStack::Mixed),
            dns_hijack: Some(vec!["any:53".into()]),
            ..Default::default()
        });
        config.mihomo_config.extra = toml::map::Map::from_iter([(
            "future-local-field".into(),
            toml::Value::String("literal-value".into()),
        )]);
        config.write(&path).unwrap();

        let loaded = Config::setup_from(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.remote_config_url, config.remote_config_url);
        assert_eq!(loaded.mihomo_config.tun.unwrap().enable, Some(true));
        assert_eq!(
            loaded.mihomo_config.extra["future-local-field"],
            toml::Value::String("literal-value".into())
        );
    }

    #[test]
    fn default_config_is_sparse_and_documents_common_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mihoto.toml");
        Config::default().write(&path).unwrap();
        let rendered = fs::read_to_string(path).unwrap();
        assert!(!rendered
            .lines()
            .any(|line| line.trim() == "mixed_port = 7890"));
        assert!(rendered.contains("# [mihomo_config.tun]"));
        assert!(rendered.contains("# [mihomo_config.dns]"));

        let mut configured = Config {
            remote_config_url: "https://example.test/config.yaml".into(),
            ..Config::default()
        };
        configured.write(&dir.path().join("mihoto.toml")).unwrap();
        let rewritten = fs::read_to_string(dir.path().join("mihoto.toml")).unwrap();
        assert_eq!(
            rewritten
                .matches("# Mihoto keeps the downloaded subscription")
                .count(),
            1
        );
        assert!(rewritten.contains("remote_config_url = \"https://example.test/config.yaml\""));
    }

    #[test]
    fn unmanaged_yaml_and_dynamic_collections_are_preserved() {
        let yaml = "tun:\n  enable: true\ndns:\n  enable: true\nlisteners:\n  - name: test\nproxies: []\nproxy-groups: []\nrules: []\n";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, yaml).unwrap();
        apply_mihomo_override(path.to_str().unwrap(), &MihomoConfig::default()).unwrap();
        let rendered = fs::read_to_string(path).unwrap();
        for field in [
            "tun:",
            "dns:",
            "listeners:",
            "proxies:",
            "proxy-groups:",
            "rules:",
        ] {
            assert!(rendered.contains(field));
        }
    }

    #[test]
    fn parse_config_does_not_create_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.toml");
        let error = parse_config(path.to_str().unwrap()).unwrap_err();
        assert!(error.to_string().contains("does not exist"));
        assert!(!path.exists());
    }

    #[test]
    fn manager_config_path_must_be_absolute() {
        assert!(validate_manager_config_path(Path::new("/etc/mihoto.toml")).is_ok());
        assert!(validate_manager_config_path(Path::new("mihoto.toml")).is_err());
        assert!(validate_manager_config_path(Path::new("")).is_err());
        assert!(validate_manager_config_path(Path::new("/")).is_err());
    }

    #[test]
    fn applying_overrides_preserves_tun_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let original =
            "tun:\n  enable: true\n  stack: system\n  device: mihoto-test\nrules:\n  - MATCH,DIRECT\n";
        fs::write(&path, original).unwrap();

        assert!(!apply_mihomo_override(path.to_str().unwrap(), &MihomoConfig::default()).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let value: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["tun"]["enable"], true);
        assert_eq!(value["tun"]["stack"], "system");
        assert_eq!(value["tun"]["device"], "mihoto-test");
        assert_eq!(value["rules"][0], "MATCH,DIRECT");
    }
}
