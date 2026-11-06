use crate::domain::config::{merge_mihomo_override, validate_config, Config, MihomoConfig};
use crate::infrastructure::filesystem::create_parent_dir;

use std::{collections::BTreeMap, fs, io::Write, path::Path};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use tempfile::NamedTempFile;

/// Load the manager configuration and atomically migrate legacy field names.
/// Required values may be absent during interactive initialization.
pub fn load_config(path: &str) -> Result<Option<Config>> {
    let config_path = Path::new(path);
    if !config_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(config_path)?;
    // Reject malformed, unknown, or conflicting fields before changing the file.
    let config = toml::from_str(&raw)?;
    if let Some(migrated) = migrate_config_url(&raw)? {
        // Keep symlinks intact by replacing the file they refer to.
        let target = fs::canonicalize(config_path)?;
        write_config_contents(&migrated, &target).with_context(|| {
            format!(
                "failed to migrate remote_config_url to config_url in {}",
                config_path.display()
            )
        })?;
    }
    Ok(Some(config))
}

fn migrate_config_url(raw: &str) -> Result<Option<String>> {
    // Key spans let us change only the root key, preserving all other bytes.
    let fields: BTreeMap<toml::Spanned<String>, serde::de::IgnoredAny> = toml::from_str(raw)?;
    let Some(key) = fields
        .keys()
        .find(|key| key.get_ref() == "remote_config_url")
    else {
        return Ok(None);
    };
    let span = key.span();
    let replacement = match raw[span.clone()].chars().next() {
        Some(quote @ ('\'' | '"')) => format!("{quote}config_url{quote}"),
        _ => "config_url".to_string(),
    };
    let mut migrated = raw.to_string();
    migrated.replace_range(span, &replacement);
    Ok(Some(migrated))
}

/// Persist the manager configuration atomically with private permissions.
pub fn write_config(config: &Config, path: &Path) -> Result<()> {
    let serialized = toml::to_string(config)?;
    write_config_contents(&serialized, path)
}

fn write_config_contents(contents: &str, path: &Path) -> Result<()> {
    create_parent_dir(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent: {}", path.display()))?;
    let mut staged = NamedTempFile::new_in(parent)?;
    staged.write_all(contents.as_bytes())?;
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
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

    #[test]
    fn legacy_field_migration_changes_only_the_key_and_is_idempotent() {
        for (old, new) in [
            ("remote_config_url", "config_url"),
            ("'remote_config_url'", "'config_url'"),
            ("\"remote_config_url\"", "\"config_url\""),
            ("\"remote_\\u0063onfig_url\"", "\"config_url\""),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("mihoto.toml");
            let raw = format!(
                "# 订阅 remote_config_url\r\nauto_update_interval=0\r\n {old}\t= 'file:///tmp/remote_config_url.yaml' # keep\r\n\r\n[mihomo_config]\r\nsecret = 'remote_config_url'\r\nmixed_port=7890\r\n"
            );
            fs::write(&path, &raw).unwrap();
            let config = parse_config(path.to_str().unwrap()).unwrap();
            assert_eq!(config.config_url, "file:///tmp/remote_config_url.yaml");
            let expected = raw.replacen(&format!(" {old}\t="), &format!(" {new}\t="), 1);
            assert_eq!(fs::read_to_string(&path).unwrap(), expected);
            let metadata = fs::metadata(&path).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            load_config(path.to_str().unwrap()).unwrap();
            assert_eq!(fs::metadata(&path).unwrap().ino(), metadata.ino());
            assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        }
    }

    #[test]
    fn current_invalid_and_conflicting_configs_are_not_rewritten() {
        for (raw, valid) in [
            (
                "config_url='file:///tmp/source.yaml' # remote_config_url\n",
                true,
            ),
            ("remote_config_url = [", false),
            ("remote_config_url=123\n", false),
            (
                "remote_config_url='file:///tmp/source.yaml'\nunknown=1\n",
                false,
            ),
            (
                "remote_config_url='file:///tmp/old.yaml'\nconfig_url='file:///tmp/new.yaml'\n",
                false,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("mihoto.toml");
            fs::write(&path, raw).unwrap();
            let metadata = fs::metadata(&path).unwrap();
            assert_eq!(load_config(path.to_str().unwrap()).is_ok(), valid);
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
            assert_eq!(fs::metadata(&path).unwrap().ino(), metadata.ino());
        }
    }

    #[test]
    fn migration_updates_the_target_of_a_config_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("actual.toml");
        let link = dir.path().join("mihoto.toml");
        fs::write(&target, "remote_config_url='file:///tmp/source.yaml'\n").unwrap();
        symlink("actual.toml", &link).unwrap();
        load_config(link.to_str().unwrap()).unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(target).unwrap(),
            "config_url='file:///tmp/source.yaml'\n"
        );
    }

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
            "`config_url` undefined"
        );

        config.config_url = "https://example.com/config.yaml".to_string();
        write_config(&config, &path).unwrap();
        let parsed = parse_config(path.to_str().unwrap()).unwrap();
        assert_eq!(parsed.config_url, config.config_url);
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
