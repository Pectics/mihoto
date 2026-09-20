use std::{future::Future, path::Path};

use anyhow::Result;

pub(crate) trait ApplyOperations {
    fn apply(&self) -> impl Future<Output = Result<()>>;
    fn reconcile_timer(&self) -> Result<()>;
}

pub(crate) trait ServiceOperations {
    fn start(&self) -> Result<()>;
    fn status(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn restart(&self) -> Result<()>;
}

pub(crate) trait TimerOperations {
    fn enable(&self) -> Result<()>;
    fn disable(&self) -> Result<()>;
    fn status(&self) -> Result<()>;
}

pub(crate) trait UninstallOperations {
    fn uninstall(&self, purge: bool, manager_config_path: &Path) -> Result<()>;
}
