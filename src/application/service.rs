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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeOperations {
        calls: RefCell<Vec<&'static str>>,
        fail: RefCell<Option<&'static str>>,
    }

    impl FakeOperations {
        fn call(&self, name: &'static str) -> Result<()> {
            self.calls.borrow_mut().push(name);
            if *self.fail.borrow() == Some(name) {
                anyhow::bail!("{name} failed");
            }
            Ok(())
        }
    }

    impl ServiceOperations for FakeOperations {
        fn start(&self) -> Result<()> {
            self.call("start")
        }
        fn status(&self) -> Result<()> {
            self.call("status")
        }
        fn stop(&self) -> Result<()> {
            self.call("stop")
        }
        fn restart(&self) -> Result<()> {
            self.call("restart")
        }
    }

    #[test]
    fn every_action_routes_to_exactly_one_operation() {
        let operations = FakeOperations::default();
        for action in [
            ServiceAction::Start,
            ServiceAction::Status,
            ServiceAction::Stop,
            ServiceAction::Restart,
        ] {
            run(&operations, action).unwrap();
        }
        assert_eq!(
            *operations.calls.borrow(),
            ["start", "status", "stop", "restart"]
        );
    }

    #[test]
    fn operation_errors_are_not_swallowed() {
        let operations = FakeOperations::default();
        *operations.fail.borrow_mut() = Some("stop");
        assert_eq!(
            run(&operations, ServiceAction::Stop)
                .unwrap_err()
                .to_string(),
            "stop failed"
        );
    }
}
