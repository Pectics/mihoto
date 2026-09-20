use crate::application::stage::StageStatus;

use std::future::Future;

use anyhow::Result;

pub(crate) enum BinaryPlan<H> {
    Skip(String),
    Install(H),
}

/// External operations required by the initialization workflow.
pub(crate) trait InitOperations {
    type BinaryHandle;

    fn prepare_binary(
        &self,
        force: bool,
        arch: Option<&str>,
    ) -> impl Future<Output = Result<BinaryPlan<Self::BinaryHandle>>>;
    fn install_binary(
        &self,
        handle: Self::BinaryHandle,
    ) -> impl Future<Output = Result<StageStatus>>;
    fn ensure_remote_config(&self, force: bool) -> impl Future<Output = Result<StageStatus>>;
    fn ensure_geodata(&self, force: bool) -> impl Future<Output = Result<StageStatus>>;
    fn ensure_ui(&self, force: bool) -> impl Future<Output = Result<StageStatus>>;
    fn validate_installed_config(&self) -> Result<StageStatus>;
    fn ensure_service(&self) -> impl Future<Output = Result<StageStatus>>;
    fn ensure_service_running(&self) -> impl Future<Output = Result<StageStatus>>;
    fn reconcile_timer(&self, config_path: &str) -> Result<StageStatus>;
}
