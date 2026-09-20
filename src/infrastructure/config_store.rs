use crate::domain::config::{merge_mihomo_override, validate_config, Config, MihomoConfig};
use crate::infrastructure::filesystem::create_parent_dir;

use std::{fs, io::Write, path::Path};

use anyhow::{bail, Result};
use colored::Colorize;
use tempfile::NamedTempFile;

/// Load the manager configuration without validating required values.
pub fn load_config(path: &str) -> Result<Option<Config>> {
    let config_path = Path::new(path);
    if !config_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(config_path)?;
    Ok(Some(toml::from_str(&raw)?))
}

/// Persist the manager configuration atomically with private permissions.
pub fn write_config(config: &Config, path: &Path) -> Result<()> {
    let serialized = toml::to_string(config)?;
    create_parent_dir(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent: {}", path.display()))?;
    let mut staged = NamedTempFile::new_in(parent)?;
    staged.write_all(serialized.as_bytes())?;
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

/// Write the default manager configuration when no file exists yet.
pub fn write_default_if_missing(path: &str) -> Result<bool> {
    let config_path = Path::new(path);
    create_parent_dir(config_path)?;
    if config_path.exists() {
        return Ok(false);
    }
    write_config(&Config::default(), config_path)?;
    Ok(true)
}

/// Load and validate the manager configuration used by state-changing commands.
pub fn parse_config(path: &str) -> Result<Config> {
    let Some(config) = load_config(path)? else {
        bail!(
            "config `{}` does not exist; run `mihoto init` first",
            path.underline()
        );
    };
    validate_config(&config)?;
    Ok(config)
}

/// Apply the configured overrides to a YAML file, rewriting it only when values change.
pub fn apply_mihomo_override(path: &str, override_config: &MihomoConfig) -> Result<bool> {
    let raw = fs::read_to_string(path)?;
    let Some(serialized) = merge_mihomo_override(&raw, override_config)? else {
        return Ok(false);
    };
    fs::write(path, serialized)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn config_is_private_and_missing_parse_does_not_create_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mihoto.toml");
        write_config(&Config::default(), &path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let missing = dir.path().join("missing.toml");
        let error = parse_config(missing.to_str().unwrap()).unwrap_err();
        assert!(error.to_string().contains("does not exist"));
        assert!(!missing.exists());
    }

    #[test]
    fn file_override_preserves_unmanaged_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            "tun:\n  enable: true\n  stack: system\nrules:\n  - MATCH,DIRECT\n",
        )
        .unwrap();
        assert!(apply_mihomo_override(path.to_str().unwrap(), &MihomoConfig::default()).unwrap());
        let value: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["tun"]["enable"], true);
        assert_eq!(value["tun"]["stack"], "system");
        assert_eq!(value["rules"][0], "MATCH,DIRECT");
    }

    #[test]
    fn load_and_default_creation_distinguish_missing_existing_and_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/mihoto.toml");
        let path_str = path.to_str().unwrap();

        assert!(load_config(path_str).unwrap().is_none());
        assert!(write_default_if_missing(path_str).unwrap());
        let loaded = load_config(path_str).unwrap().unwrap();
        assert_eq!(loaded.mihomo_binary_path, "/usr/local/bin/mihomo");

        let original = fs::read_to_string(&path).unwrap();
        assert!(!write_default_if_missing(path_str).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        fs::write(&path, "not valid = [").unwrap();
        assert!(load_config(path_str).is_err());
    }

    #[test]
    fn parse_validates_loaded_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mihoto.toml");
        let mut config = Config::default();
        write_config(&config, &path).unwrap();
        assert_eq!(
            parse_config(path.to_str().unwrap())
                .unwrap_err()
                .to_string(),
            "`remote_config_url` undefined"
        );

        config.remote_config_url = "https://example.com/config.yaml".to_string();
        write_config(&config, &path).unwrap();
        let parsed = parse_config(path.to_str().unwrap()).unwrap();
        assert_eq!(parsed.remote_config_url, config.remote_config_url);
    }

    #[test]
    fn unchanged_and_malformed_yaml_are_never_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let defaults = MihomoConfig::default();
        let current = merge_mihomo_override("rules: []\n", &defaults)
            .unwrap()
            .unwrap();
        fs::write(&path, &current).unwrap();
        assert!(!apply_mihomo_override(path.to_str().unwrap(), &defaults).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), current);

        let malformed = "port: [";
        fs::write(&path, malformed).unwrap();
        assert!(apply_mihomo_override(path.to_str().unwrap(), &defaults).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), malformed);
    }
}
