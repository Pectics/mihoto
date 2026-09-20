use crate::domain::config::Config;
use crate::infrastructure::config_store::{load_config, write_config, write_default_if_missing};

use std::path::Path;

use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::Input;

fn prompt_subscription_url() -> Result<String> {
    let url: String = Input::new()
        .with_prompt("Remote subscription URL")
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

    if config.remote_config_url.is_empty() {
        if yes {
            bail!(
                "`remote_config_url` is not set - edit `{config_path}` or run `mihoto init` interactively"
            );
        }
        if just_created {
            println!(
                "{} Created default config at {}",
                "mihoto:".cyan(),
                config_path.underline().yellow()
            );
        }
        println!("{} Enter your remote subscription URL:", "mihoto:".yellow());
        config.remote_config_url = prompt_subscription_url()?;
        write_config(&config, Path::new(config_path))?;
        println!("{} Saved", "mihoto:".green());
    }
    Ok(config)
}
