use crate::application::ports::UpgradeOperations;

use anyhow::Result;

pub(crate) enum UpgradeOutcome {
    Checked(Option<String>),
    Installed,
}

pub(crate) async fn run(
    operations: &impl UpgradeOperations,
    yes: bool,
    check: bool,
    target: Option<String>,
) -> Result<UpgradeOutcome> {
    if check {
        Ok(UpgradeOutcome::Checked(operations.check().await?))
    } else {
        operations.install(yes, target).await?;
        Ok(UpgradeOutcome::Installed)
    }
}
