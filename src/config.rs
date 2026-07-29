use crate::ui::{default_ui, Ui};
use crate::utils::create_parent_dir;

use std::{collections::HashMap, fs, io::Write, path::Path};

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

/// `mihomo` configurations (partial).
///
/// Referenced from https://wiki.metacubex.one/config
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct MihomoConfig {
    pub port: u16,
    pub socks_port: u16,
    pub mixed_port: Option<u16>,
    pub redir_port: Option<u16>,
    pub allow_lan: Option<bool>,
    pub bind_address: Option<String>,
    mode: MihomoMode,
    log_level: MihomoLogLevel,
    ipv6: Option<bool>,
    pub external_controller: Option<String>,
    pub external_ui: Option<String>,
    pub secret: Option<String>,
    pub geodata_mode: Option<bool>,
    pub geo_auto_update: Option<bool>,
    pub geo_update_interval: Option<u16>,
    pub geox_url: Option<GeoxUrl>,
}

impl Default for MihomoConfig {
    fn default() -> Self {
        MihomoConfig {
            port: 7891,
            socks_port: 7892,
            mixed_port: Some(7890),
            redir_port: None,
            allow_lan: Some(false),
            bind_address: Some(String::from("*")),
            mode: MihomoMode::Rule,
            log_level: MihomoLogLevel::Info,
            ipv6: Some(true),
			external_controller: Some(String::from("127.0.0.1:9090")),
            external_ui: Some(String::from("ui")),
            secret: None,
            geodata_mode: Some(false),
            geo_auto_update: Some(true),
            geo_update_interval: Some(24),
            geox_url: Some(GeoxUrl {
                geoip: String::from(
                    "https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/geoip.dat",
                ),
                geosite: String::from(
                    "https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/geosite.dat",
                ),
                mmdb: String::from(
                    "https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/country.mmdb",
                ),
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MihomoMode {
    #[serde(alias = "global", rename(serialize = "global"))]
    Global,
    #[serde(alias = "rule", rename(serialize = "rule"))]
    Rule,
    #[serde(alias = "direct", rename(serialize = "direct"))]
    Direct,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MihomoLogLevel {
    #[serde(alias = "silent", rename(serialize = "silent"))]
    Silent,
    #[serde(alias = "error", rename(serialize = "error"))]
    Error,
    #[serde(alias = "warning", rename(serialize = "warning"))]
    Warning,
    #[serde(alias = "info", rename(serialize = "info"))]
    Info,
    #[serde(alias = "debug", rename(serialize = "debug"))]
    Debug,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeoxUrl {
    pub geoip: String,
    pub geosite: String,
    pub mmdb: String,
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
        let serialized_config = toml::to_string(&self)?;
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
            bail!("`{}` undefined", field);
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

/// `mihomoYamlConfig` is defined to support serde serialization and deserialization of arbitrary
/// mihomo `config.yaml`, with support for fields defined in `mihomoConfig` for overrides and also
/// extra fields that are not managed by `mihoto` by design (namely `proxies`, `proxy-groups`,
/// `rules`, etc.)
#[derive(Serialize, Deserialize, Debug)]
pub struct MihomoYamlConfig {
    port: Option<u16>,

    #[serde(rename = "socks-port")]
    socks_port: Option<u16>,

    #[serde(rename = "mixed-port", skip_serializing_if = "Option::is_none")]
    mixed_port: Option<u16>,

    #[serde(rename = "redir-port", skip_serializing_if = "Option::is_none")]
    redir_port: Option<u16>,

    #[serde(rename = "allow-lan", skip_serializing_if = "Option::is_none")]
    allow_lan: Option<bool>,

    #[serde(rename = "bind-address", skip_serializing_if = "Option::is_none")]
    bind_address: Option<String>,

    mode: Option<MihomoMode>,

    #[serde(rename = "log-level")]
    log_level: Option<MihomoLogLevel>,

    #[serde(skip_serializing_if = "Option::is_none")]
    ipv6: Option<bool>,

    #[serde(
        rename = "external-controller",
        skip_serializing_if = "Option::is_none"
    )]
    external_controller: Option<String>,

    #[serde(rename = "external-ui", skip_serializing_if = "Option::is_none")]
    external_ui: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,

    #[serde(rename = "geodata-mode", skip_serializing_if = "Option::is_none")]
    geodata_mode: Option<bool>,

    #[serde(rename = "geo-auto-update", skip_serializing_if = "Option::is_none")]
    geo_auto_update: Option<bool>,

    #[serde(
        rename = "geo-update-interval",
        skip_serializing_if = "Option::is_none"
    )]
    geo_update_interval: Option<u16>,

    #[serde(rename = "geox-url", skip_serializing_if = "Option::is_none")]
    geox_url: Option<GeoxUrl>,

    #[serde(flatten)]
    extra: HashMap<String, serde_yaml::Value>,
}

/// Apply config overrides to mihomo's `config.yaml`.
///
/// Only a subset of mihomo's config fields are supported, as defined in `mihomoConfig`.
///
/// Rules:
/// * Fields defined in `mihoto.toml` will override the downloaded remote `config.yaml`.
/// * Fields undefined will be removed from the downloaded `config.yaml`.
/// * Fields not supported by `mihoto` will be kept as is.
///
/// Returns `true` when the file contents had to change.
pub fn apply_mihomo_override(path: &str, override_config: &MihomoConfig) -> Result<bool> {
    let raw_mihomo_yaml = fs::read_to_string(path)?;
    let mut mihomo_yaml: MihomoYamlConfig = serde_yaml::from_str(&raw_mihomo_yaml)?;

    // Apply config overrides
    mihomo_yaml.port = Some(override_config.port);
    mihomo_yaml.socks_port = Some(override_config.socks_port);
    mihomo_yaml.mixed_port = override_config.mixed_port;
    mihomo_yaml.redir_port = override_config.redir_port;
    mihomo_yaml.allow_lan = override_config.allow_lan;
    mihomo_yaml.bind_address = override_config.bind_address.clone();
    mihomo_yaml.mode = Some(override_config.mode.clone());
    mihomo_yaml.log_level = Some(override_config.log_level.clone());
    mihomo_yaml.ipv6 = override_config.ipv6;
    mihomo_yaml.external_controller = override_config.external_controller.clone();
    mihomo_yaml.external_ui = override_config.external_ui.clone();
    mihomo_yaml.secret = override_config.secret.clone();
    mihomo_yaml.geodata_mode = override_config.geodata_mode;
    mihomo_yaml.geo_auto_update = override_config.geo_auto_update;
    mihomo_yaml.geo_update_interval = override_config.geo_update_interval;
    mihomo_yaml.geox_url = override_config.geox_url.clone();

    // Avoid rewriting already-current YAML just because formatting or map order changed.
    let serialized_mihomo_yaml = serde_yaml::to_string(&mihomo_yaml)?;
    let raw_value: serde_yaml::Value = serde_yaml::from_str(&raw_mihomo_yaml)?;
    let serialized_value: serde_yaml::Value = serde_yaml::from_str(&serialized_mihomo_yaml)?;
    if raw_value == serialized_value {
        return Ok(false);
    }

    fs::write(path, serialized_mihomo_yaml)?;
    Ok(true)
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
        assert_eq!(
            config.mihomo_config.external_controller.as_deref(),
            Some("127.0.0.1:9090")
        );
        let mut relative = config.clone();
        relative.mihomo_config_root = "relative".into();
        assert!(validate_config(&relative).is_err());
        let mut too_long = config;
        too_long.auto_update_interval = 25;
        assert!(validate_config(&too_long).is_err());
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
    fn tun_and_unknown_yaml_are_preserved() {
        let yaml = "tun:\n  enable: true\ndns:\n  enable: true\nlisteners:\n  - name: test\nproxies: []\nproxy-groups: []\nrules: []\n";
        let parsed: MihomoYamlConfig = serde_yaml::from_str(yaml).unwrap();
        let rendered = serde_yaml::to_string(&parsed).unwrap();
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
    }

    #[test]
    fn applying_overrides_preserves_tun_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
			&path,
			"tun:\n  enable: true\n  stack: system\n  device: mihoto-test\nrules:\n  - MATCH,DIRECT\n",
		)
		.unwrap();

        apply_mihomo_override(path.to_str().unwrap(), &MihomoConfig::default()).unwrap();
        let value: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["tun"]["enable"], true);
        assert_eq!(value["tun"]["stack"], "system");
        assert_eq!(value["tun"]["device"], "mihoto-test");
        assert_eq!(value["rules"][0], "MATCH,DIRECT");
    }
}
