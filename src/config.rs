use crate::ui::{default_ui, Ui};
use crate::utils::{create_parent_dir, write_private_file};

use std::{collections::HashMap, env, fs, net::IpAddr, path::Path};

use anyhow::{bail, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

pub const ALLOW_INSECURE_CONTROLLER_ENV: &str = "MIHORO_ALLOW_INSECURE_CONTROLLER";

/// Mihomo release channel for automatic binary fetching.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub enum MihomoChannel {
    #[default]
    #[serde(alias = "stable", rename(serialize = "stable"))]
    Stable,
    #[serde(alias = "alpha", rename(serialize = "alpha"))]
    Alpha,
}

/// `mihoro` configurations.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    pub remote_config_url: String,
    pub active_profile: String,
    pub profile_config_root: String,
    pub profile_data_root: String,
    pub profile_state_root: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub profiles: HashMap<String, ProfileConfig>,
    #[serde(default = "default_ui", skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
    pub mihomo_channel: MihomoChannel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_mihomo_binary_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mihomo_arch: Option<String>,
    pub mihomo_binary_path: String,
    pub mihomo_config_root: String,
    pub user_systemd_root: String,
    pub mihoro_user_agent: String,
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
            active_profile: String::from("default"),
            profile_config_root: String::from("~/.config/mihoto"),
            profile_data_root: String::from("~/.local/share/mihoto"),
            profile_state_root: String::from("~/.local/state/mihoto"),
            profiles: HashMap::new(),
            mihomo_binary_path: String::from("~/.local/bin/mihomo"),
            mihomo_config_root: String::from("~/.config/mihomo"),
            user_systemd_root: String::from("~/.config/systemd/user"),
            mihoro_user_agent: String::from("mihoro"),
            auto_update_interval: 12,
            mihomo_config: MihomoConfig::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProfileSource {
    Url { url: String },
    File { path: String },
    Existing { path: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ProfileConfig {
    pub source: ProfileSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
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
        write_private_file(path, serialized_config.as_bytes())?;
        Ok(())
    }

    pub fn effective_profile(&self, name: &str) -> Option<ProfileConfig> {
        if let Some(profile) = self.profiles.get(name) {
            return Some(profile.clone());
        }
        if self.profiles.is_empty() && name == "default" && !self.remote_config_url.is_empty() {
            return Some(ProfileConfig {
                source: ProfileSource::Url {
                    url: self.remote_config_url.clone(),
                },
                user_agent: None,
            });
        }
        None
    }
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
    if config_path.exists() {
        return Ok(false);
    }
    Config::new().write(config_path)?;
    Ok(true)
}

/// Validate that required config fields are non-empty.
pub fn validate_config(config: &Config) -> Result<()> {
    let required_fields = [
        ("mihomo_binary_path", &config.mihomo_binary_path),
        ("mihomo_config_root", &config.mihomo_config_root),
        ("user_systemd_root", &config.user_systemd_root),
        ("profile_config_root", &config.profile_config_root),
        ("profile_data_root", &config.profile_data_root),
        ("profile_state_root", &config.profile_state_root),
    ];
    for (field, value) in required_fields.iter() {
        if value.is_empty() {
            bail!("`{}` undefined", field);
        }
    }
    if config.effective_profile(&config.active_profile).is_none() {
        bail!(
            "`remote_config_url` undefined and active profile `{}` not found",
            config.active_profile
        );
    }
    validate_controller_security(config)?;
    Ok(())
}

fn validate_controller_security(config: &Config) -> Result<()> {
    let Some(controller) = config.mihomo_config.external_controller.as_deref() else {
        return Ok(());
    };
    if controller.trim().is_empty() {
        return Ok(());
    }
    if controller_is_loopback(controller) {
        return Ok(());
    }
    if config
        .mihomo_config
        .secret
        .as_deref()
        .is_some_and(|secret| !secret.trim().is_empty())
    {
        return Ok(());
    }
    if env::var(ALLOW_INSECURE_CONTROLLER_ENV).as_deref() == Ok("1") {
        return Ok(());
    }
    bail!(
        "`mihomo_config.external_controller` binds to a non-loopback address; set \
         `mihomo_config.secret` or export {}=1 to allow this insecure controller",
        ALLOW_INSECURE_CONTROLLER_ENV
    )
}

fn controller_is_loopback(controller: &str) -> bool {
    let Some(host) = controller_host(controller) else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn controller_host(controller: &str) -> Option<&str> {
    let controller = controller
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    if let Some(rest) = controller.strip_prefix('[') {
        let (host, _) = rest.split_once(']')?;
        return Some(host);
    }
    controller.rsplit_once(':').map(|(host, _)| host)
}

/// Tries to parse mihoro config as toml from path.
///
/// * If config file does not exist, creates default config file and returns an error directing
///   the user to run `mihoro init`.
/// * If found, parses the file and validates required fields.
pub fn parse_config(path: &str) -> Result<Config> {
    let config_path = Path::new(path);
    create_parent_dir(config_path)?;

    if !config_path.exists() {
        Config::new().write(config_path)?;
        bail!(
            "created default config at `{}`, run `mihoro init` to finish setup",
            path.underline()
        );
    }

    let config = Config::setup_from(path)?;
    validate_config(&config)?;
    Ok(config)
}

/// `mihomoYamlConfig` is defined to support serde serialization and deserialization of arbitrary
/// mihomo `config.yaml`, with support for fields defined in `mihomoConfig` for overrides and also
/// extra fields that are not managed by `mihoro` by design (namely `proxies`, `proxy-groups`,
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
/// * Fields defined in `mihoro.toml` will override the downloaded remote `config.yaml`.
/// * Fields undefined will be removed from the downloaded `config.yaml`.
/// * Fields not supported by `mihoro` will be kept as is.
///
/// Returns `true` when the file contents had to change.
pub fn render_mihomo_overlay(path: &Path, override_config: &MihomoConfig) -> Result<bool> {
    let overlay = MihomoYamlConfig {
        port: Some(override_config.port),
        socks_port: Some(override_config.socks_port),
        mixed_port: override_config.mixed_port,
        redir_port: override_config.redir_port,
        allow_lan: override_config.allow_lan,
        bind_address: override_config.bind_address.clone(),
        mode: Some(override_config.mode.clone()),
        log_level: Some(override_config.log_level.clone()),
        ipv6: override_config.ipv6,
        external_controller: override_config.external_controller.clone(),
        external_ui: override_config.external_ui.clone(),
        secret: override_config.secret.clone(),
        geodata_mode: override_config.geodata_mode,
        geo_auto_update: override_config.geo_auto_update,
        geo_update_interval: override_config.geo_update_interval,
        geox_url: override_config.geox_url.clone(),
        extra: HashMap::new(),
    };

    let serialized_overlay = serde_yaml::to_string(&overlay)?;
    if let Ok(existing) = fs::read_to_string(path) {
        let existing_value: serde_yaml::Value = serde_yaml::from_str(&existing)?;
        let overlay_value: serde_yaml::Value = serde_yaml::from_str(&serialized_overlay)?;
        if existing_value == overlay_value {
            return Ok(false);
        }
    }

    write_private_file(path, serialized_overlay.as_bytes())?;
    Ok(true)
}

pub fn render_mihomo_override(
    source_path: &Path,
    output_path: &Path,
    override_config: &MihomoConfig,
) -> Result<bool> {
    let raw_mihomo_yaml = fs::read_to_string(source_path)?;
    let source_value: Value = serde_yaml::from_str(&raw_mihomo_yaml)?;
    let overlay = MihomoYamlConfig {
        port: Some(override_config.port),
        socks_port: Some(override_config.socks_port),
        mixed_port: override_config.mixed_port,
        redir_port: override_config.redir_port,
        allow_lan: override_config.allow_lan,
        bind_address: override_config.bind_address.clone(),
        mode: Some(override_config.mode.clone()),
        log_level: Some(override_config.log_level.clone()),
        ipv6: override_config.ipv6,
        external_controller: override_config.external_controller.clone(),
        external_ui: override_config.external_ui.clone(),
        secret: override_config.secret.clone(),
        geodata_mode: override_config.geodata_mode,
        geo_auto_update: override_config.geo_auto_update,
        geo_update_interval: override_config.geo_update_interval,
        geox_url: override_config.geox_url.clone(),
        extra: HashMap::new(),
    };
    let overlay_value = serde_yaml::to_value(overlay)?;
    let mihomo_yaml = apply_yaml_overlay(source_value, overlay_value)?;

    // Avoid rewriting already-current YAML just because formatting or map order changed.
    let serialized_mihomo_yaml = serde_yaml::to_string(&mihomo_yaml)?;
    if let Ok(current_output) = fs::read_to_string(output_path) {
        let raw_value: serde_yaml::Value = serde_yaml::from_str(&current_output)?;
        let serialized_value: serde_yaml::Value = serde_yaml::from_str(&serialized_mihomo_yaml)?;
        if raw_value == serialized_value {
            return Ok(false);
        }
    }

    write_private_file(output_path, serialized_mihomo_yaml.as_bytes())?;
    Ok(true)
}

pub fn apply_yaml_overlay(source: Value, overlay: Value) -> Result<Value> {
    if is_delete_value(&overlay) {
        bail!("`!delete` cannot be used as the root overlay value");
    }
    Ok(merge_yaml_value(source, overlay))
}

fn merge_yaml_value(source: Value, overlay: Value) -> Value {
    match (source, overlay) {
        (Value::Mapping(mut source), Value::Mapping(overlay)) => {
            merge_yaml_mapping(&mut source, overlay);
            Value::Mapping(source)
        }
        (_, overlay) => overlay,
    }
}

fn merge_yaml_mapping(source: &mut Mapping, overlay: Mapping) {
    for (key, value) in overlay {
        if is_delete_value(&value) {
            source.remove(&key);
            continue;
        }
        match (source.remove(&key), value) {
            (Some(Value::Mapping(existing)), Value::Mapping(overlay_mapping)) => {
                let mut existing = existing;
                merge_yaml_mapping(&mut existing, overlay_mapping);
                source.insert(key, Value::Mapping(existing));
            }
            (_, replacement) => {
                source.insert(key, replacement);
            }
        }
    }
}

fn is_delete_value(value: &Value) -> bool {
    match value {
        Value::Tagged(tagged) => tagged.tag == "!delete",
        _ => false,
    }
}

#[allow(dead_code)]
pub fn apply_mihomo_override(path: &str, override_config: &MihomoConfig) -> Result<bool> {
    let path = Path::new(path);
    render_mihomo_override(path, path, override_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_parse_config_creates_default_if_not_exists() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        let result = parse_config(config_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(config_path.exists());

        Ok(())
    }

    #[test]
    fn test_config_write_and_read() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        let mut config = Config::new();
        config.remote_config_url = "http://example.com/config.yaml".to_string();
        config.write(&config_path)?;

        let read_config = Config::setup_from(config_path.to_str().unwrap())?;
        assert_eq!(
            read_config.remote_config_url,
            "http://example.com/config.yaml"
        );
        assert_eq!(read_config.ui, Some(Ui::Metacubexd));

        Ok(())
    }

    #[test]
    fn test_default_controller_is_loopback() {
        assert_eq!(
            Config::new().mihomo_config.external_controller.as_deref(),
            Some("127.0.0.1:9090")
        );
    }

    #[test]
    fn test_parse_config_validates_required_fields() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        let toml_content = r#"
            mihomo_binary_path = "~/.local/bin/mihomo"
            mihomo_config_root = "~/.config/mihomo"
            user_systemd_root = "~/.config/systemd/user"
        "#;
        fs::write(&config_path, toml_content)?;

        let result = parse_config(config_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("remote_config_url"));

        Ok(())
    }

    #[test]
    fn test_validate_config_rejects_non_loopback_controller_without_secret() {
        let mut config = Config::new();
        config.remote_config_url = "http://example.com/config.yaml".to_string();
        config.mihomo_config.external_controller = Some("0.0.0.0:9090".to_string());
        config.mihomo_config.secret = None;

        let err = validate_config(&config).unwrap_err();

        assert!(err
            .to_string()
            .contains("mihomo_config.external_controller"));
        assert!(err
            .to_string()
            .contains("MIHORO_ALLOW_INSECURE_CONTROLLER=1"));
    }

    #[test]
    fn test_apply_mihomo_override() -> Result<()> {
        let dir = tempdir()?;
        let yaml_path = dir.path().join("config.yaml");

        let yaml_content = r#"
            port: 8080
            socks-port: 8081
            mixed-port: 7890
            redir-port: 7893
            allow-lan: false
            mode: rule
            log-level: info
            proxies:
              - name: "test"
                type: http
                server: example.com
                port: 443
        "#;
        fs::write(&yaml_path, yaml_content)?;

        let override_config = MihomoConfig {
            port: 7891,
            socks_port: 7892,
            ..Default::default()
        };

        let changed = apply_mihomo_override(yaml_path.to_str().unwrap(), &override_config)?;
        assert!(changed);

        let updated_content = fs::read_to_string(&yaml_path)?;
        assert!(updated_content.contains("port: 7891"));
        assert!(updated_content.contains("socks-port: 7892"));
        assert!(updated_content.contains("proxies:"));

        Ok(())
    }

    #[test]
    fn test_apply_mihomo_override_skips_when_yaml_already_matches() -> Result<()> {
        let dir = tempdir()?;
        let yaml_path = dir.path().join("config.yaml");

        let yaml_content = r#"
            port: 7891
            socks-port: 7892
            mixed-port: 7890
            allow-lan: false
            bind-address: "*"
            mode: rule
            log-level: info
            ipv6: true
            external-controller: 127.0.0.1:9090
            external-ui: ui
            geodata-mode: false
            geo-auto-update: true
            geo-update-interval: 24
            geox-url:
              geoip: https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/geoip.dat
              geosite: https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/geosite.dat
              mmdb: https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/country.mmdb
            proxies:
              - name: "test"
                type: http
                server: example.com
                port: 443
        "#;
        fs::write(&yaml_path, yaml_content)?;

        let changed = apply_mihomo_override(yaml_path.to_str().unwrap(), &MihomoConfig::default())?;

        assert!(!changed);
        Ok(())
    }

    #[test]
    fn test_parse_config_uses_default_ui() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("test.toml");

        let toml_content = r#"
            remote_config_url = "http://example.com/config.yaml"
            mihomo_binary_path = "~/.local/bin/mihomo"
            mihomo_config_root = "~/.config/mihomo"
            user_systemd_root = "~/.config/systemd/user"
        "#;
        fs::write(&config_path, toml_content)?;

        let config = parse_config(config_path.to_str().unwrap())?;
        assert_eq!(config.ui, Some(Ui::Metacubexd));

        Ok(())
    }

    #[test]
    fn overlay_merges_maps_replaces_arrays_and_deletes_keys() -> Result<()> {
        let source: serde_yaml::Value = serde_yaml::from_str(
            r#"
port: 7890
profile:
  name: remote
  tags:
    - remote
rules:
  - MATCH,DIRECT
secret: keep-me
"#,
        )?;
        let overlay: serde_yaml::Value = serde_yaml::from_str(
            r#"
profile:
  mode: local
  tags:
    - local
rules:
  - DOMAIN,example.com,DIRECT
secret: !delete
"#,
        )?;

        let rendered = apply_yaml_overlay(source, overlay)?;
        let rendered = serde_yaml::to_string(&rendered)?;

        assert!(rendered.contains("port: 7890"));
        assert!(rendered.contains("name: remote"));
        assert!(rendered.contains("mode: local"));
        assert!(rendered.contains("- local"));
        assert!(!rendered.contains("- remote"));
        assert!(rendered.contains("DOMAIN,example.com,DIRECT"));
        assert!(!rendered.contains("secret:"));
        Ok(())
    }

    #[test]
    fn config_synthesizes_default_profile_from_legacy_remote_url() {
        let mut config = Config::new();
        config.remote_config_url = "https://example.com/sub.yaml".to_string();

        let profile = config.effective_profile("default").unwrap();

        assert_eq!(config.active_profile, "default");
        assert_eq!(
            profile.source,
            ProfileSource::Url {
                url: "https://example.com/sub.yaml".to_string()
            }
        );
    }
}
