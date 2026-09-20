use crate::application::ports::TimerOperations;

use anyhow::Result;

pub(crate) enum TimerAction {
    Enable,
    Disable,
    Status,
}

pub(crate) fn run(operations: &impl TimerOperations, action: TimerAction) -> Result<()> {
    match action {
        TimerAction::Enable => operations.enable(),
        TimerAction::Disable => operations.disable(),
        TimerAction::Status => operations.status(),
    }
}
