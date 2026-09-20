use crate::application::ports::ApplyOperations;

use anyhow::Result;

pub(crate) async fn run(operations: &impl ApplyOperations) -> Result<()> {
    operations.apply().await?;
    operations.reconcile_timer()
}
