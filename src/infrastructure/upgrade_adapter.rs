use crate::application::ports::UpgradeOperations;
use crate::infrastructure::upgrade;

use std::future::Future;

use anyhow::Result;

pub(crate) struct SelfUpgrade;

impl UpgradeOperations for SelfUpgrade {
    fn check(&self) -> impl Future<Output = Result<Option<String>>> {
        upgrade::check_for_update()
    }

    fn install(
        &self,
        no_confirm: bool,
        target: Option<String>,
    ) -> impl Future<Output = Result<()>> {
        upgrade::run_upgrade(no_confirm, target)
    }
}
