use crate::application::ports::{BinaryPlan, InitOperations};
use crate::application::stage::StageStatus;
use crate::infrastructure::mihomo::{BinaryPlan as RuntimeBinaryPlan, Mihoto};
use crate::infrastructure::timer;

use std::future::Future;

use anyhow::Result;
use reqwest::Client;
use tempfile::NamedTempFile;

impl InitOperations for (&Mihoto, &Client) {
    type BinaryHandle = NamedTempFile;

    fn prepare_binary(
        &self,
        force: bool,
        arch: Option<&str>,
    ) -> impl Future<Output = Result<BinaryPlan<Self::BinaryHandle>>> {
        async move {
            match self.0.prepare_binary(self.1, force, arch).await? {
                RuntimeBinaryPlan::Skip(reason) => Ok(BinaryPlan::Skip(reason)),
                RuntimeBinaryPlan::Install(handle) => Ok(BinaryPlan::Install(handle)),
            }
        }
    }

    fn install_binary(
        &self,
        handle: Self::BinaryHandle,
    ) -> impl Future<Output = Result<StageStatus>> {
        self.0.install_binary(handle)
    }

    fn ensure_remote_config(&self, force: bool) -> impl Future<Output = Result<StageStatus>> {
        self.0.ensure_remote_config(self.1, force)
    }

    fn ensure_geodata(&self, force: bool) -> impl Future<Output = Result<StageStatus>> {
        self.0.ensure_geodata(self.1, force)
    }

    fn ensure_ui(&self, force: bool) -> impl Future<Output = Result<StageStatus>> {
        self.0.ensure_ui(self.1, force)
    }

    fn validate_installed_config(&self) -> Result<StageStatus> {
        self.0.validate_installed_config()
    }

    fn ensure_service(&self) -> impl Future<Output = Result<StageStatus>> {
        self.0.ensure_service()
    }

    fn ensure_service_running(&self) -> impl Future<Output = Result<StageStatus>> {
        self.0.ensure_service_running()
    }

    fn reconcile_timer(&self, config_path: &str) -> Result<StageStatus> {
        timer::reconcile(&self.0.config, config_path)?;
        if self.0.config.auto_update_interval == 0 {
            Ok(StageStatus::Skipped(
                "disabled by auto_update_interval".to_string(),
            ))
        } else {
            Ok(StageStatus::Installed)
        }
    }
}
