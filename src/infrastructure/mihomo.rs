mod config;
mod context;
mod core;
mod resources;
mod service;
mod uninstall;

pub(crate) use crate::application::stage::StageStatus;
use crate::domain::config::Config;

#[cfg(test)]
use std::fs;
use std::time::Duration;

#[cfg(test)]
use reqwest::Client;
use tempfile::NamedTempFile;

pub(super) const SERVICE_HEALTH_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Mihoto {
    pub prefix: String,
    pub config: Config,
    pub mihomo_target_binary_path: String,
    pub mihomo_target_config_root: String,
    pub mihomo_target_config_path: String,
    pub mihomo_target_service_path: String,
}

/// Download result retained between the init download and installation phases.
pub enum BinaryPlan {
    Skip(String),
    Install(NamedTempFile),
}

#[cfg(test)]
use service::render_service_string;
#[cfg(test)]
use uninstall::validate_safe_purge_target;

#[cfg(test)]
#[cfg(test)]
mod tests;
