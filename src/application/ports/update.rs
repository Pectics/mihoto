use crate::application::stage::StageStatus;

use std::future::Future;

use anyhow::Result;

/// Operations required by the update workflow, independent from command parsing and output.
pub(crate) trait UpdateOperations {
    fn update_config(&self) -> impl Future<Output = Result<StageStatus>>;
    fn update_geodata(&self) -> impl Future<Output = Result<StageStatus>>;
    fn update_ui(&self) -> impl Future<Output = Result<StageStatus>>;
    fn update_core(&self, arch: Option<&str>) -> impl Future<Output = Result<StageStatus>>;
    fn restart_service(&self) -> impl Future<Output = Result<StageStatus>>;
}
