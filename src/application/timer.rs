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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeOperations(RefCell<Vec<&'static str>>);

    impl FakeOperations {
        fn call(&self, name: &'static str) -> Result<()> {
            self.0.borrow_mut().push(name);
            Ok(())
        }
    }

    impl TimerOperations for FakeOperations {
        fn enable(&self) -> Result<()> {
            self.call("enable")
        }
        fn disable(&self) -> Result<()> {
            self.call("disable")
        }
        fn status(&self) -> Result<()> {
            self.call("status")
        }
    }

    #[test]
    fn every_action_routes_to_exactly_one_operation() {
        let operations = FakeOperations::default();
        for action in [
            TimerAction::Enable,
            TimerAction::Disable,
            TimerAction::Status,
        ] {
            run(&operations, action).unwrap();
        }
        assert_eq!(*operations.0.borrow(), ["enable", "disable", "status"]);
    }
}
