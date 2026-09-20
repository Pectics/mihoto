use crate::application::ports::UninstallOperations;

use std::path::Path;

use anyhow::Result;

pub(crate) fn run(
    operations: &impl UninstallOperations,
    purge: bool,
    manager_config_path: &Path,
) -> Result<()> {
    operations.uninstall(purge, manager_config_path)
}
