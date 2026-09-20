use crate::application::ports::ServiceOperations;

use anyhow::Result;

pub(crate) enum ServiceAction {
    Start,
    Status,
    Stop,
    Restart,
}

pub(crate) fn run(operations: &impl ServiceOperations, action: ServiceAction) -> Result<()> {
    match action {
        ServiceAction::Start => operations.start(),
        ServiceAction::Status => operations.status(),
        ServiceAction::Stop => operations.stop(),
        ServiceAction::Restart => operations.restart(),
    }
}
