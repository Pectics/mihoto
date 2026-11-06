use crate::domain::config::Config;
use crate::infrastructure::config_store::{load_config, write_config, write_default_if_missing};

use std::path::Path;

use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::Input;

fn prompt_subscription_url() -> Result<String> {
    let url: String = Input::new()
        .with_prompt("Subscription URL (https:// or file:///absolute/path)")
        .validate_with(|value: &String| {
            if value.trim().is_empty() {
                Err("URL cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()?;
    Ok(url.trim().to_string())
}

pub(crate) fn bootstrap_config(config_path: &str, yes: bool) -> Result<Config> {
    let just_created = write_default_if_missing(config_path)?;
    let mut config =
        load_config(config_path)?.expect("config file must exist after write_default_if_missing");

    if config.config_url.is_empty() {
        if yes {
            bail!(
                "`config_url` is not set - edit `{config_path}` or run `mihoto init` interactively"
            );
        }
        if just_created {
            println!(
                "{} Created default config at {}",
                "mihoto:".cyan(),
                config_path.underline().yellow()
            );
        }
        println!("{} Enter your subscription URL:", "mihoto:".yellow());
        config.config_url = prompt_subscription_url()?;
        write_config(&config, Path::new(config_path))?;
        println!("{} Saved", "mihoto:".green());
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn yes_mode_creates_defaults_but_requires_a_preconfigured_subscription() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/mihoto.toml");
        let error = bootstrap_config(path.to_str().unwrap(), true).unwrap_err();
        assert!(error.to_string().contains("`config_url` is not set"));
        assert!(path.exists());
        assert_eq!(
            load_config(path.to_str().unwrap())
                .unwrap()
                .unwrap()
                .config_url,
            ""
        );
    }

    #[test]
    fn yes_mode_returns_an_existing_config_without_rewriting_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mihoto.toml");
        let config = Config {
            config_url: "https://example.com/subscription".to_string(),
            ..Config::default()
        };
        write_config(&config, &path).unwrap();
        let original = fs::read_to_string(&path).unwrap();
        let loaded = bootstrap_config(path.to_str().unwrap(), true).unwrap();
        assert_eq!(loaded.config_url, config.config_url);
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn malformed_existing_config_is_reported_without_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mihoto.toml");
        fs::write(&path, "invalid = [").unwrap();
        assert!(bootstrap_config(path.to_str().unwrap(), true).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "invalid = [");
    }
}
