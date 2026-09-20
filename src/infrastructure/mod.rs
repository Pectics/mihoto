pub(crate) mod command_adapters;
pub(crate) mod config_store;
pub(crate) mod download;
pub(crate) mod filesystem;
mod init_adapter;
pub(crate) mod mihomo;
pub(crate) mod mihomo_release;
pub(crate) mod systemctl;
pub(crate) mod systemd_unit;
pub(crate) mod timer;
pub(crate) mod ui;
mod update_adapter;
#[cfg(feature = "self_update")]
pub(crate) mod upgrade;
#[cfg(feature = "self_update")]
pub(crate) mod upgrade_adapter;
