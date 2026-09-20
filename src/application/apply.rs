use crate::application::ports::ApplyOperations;

use anyhow::Result;

pub(crate) async fn run(operations: &impl ApplyOperations) -> Result<()> {
    operations.apply().await?;
    operations.reconcile_timer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, future::ready};

    struct FakeOperations {
        calls: RefCell<Vec<&'static str>>,
        fail_apply: bool,
        fail_timer: bool,
    }

    impl ApplyOperations for FakeOperations {
        fn apply(&self) -> impl std::future::Future<Output = Result<()>> {
            self.calls.borrow_mut().push("apply");
            ready(if self.fail_apply {
                Err(anyhow::anyhow!("apply failed"))
            } else {
                Ok(())
            })
        }

        fn reconcile_timer(&self) -> Result<()> {
            self.calls.borrow_mut().push("timer");
            if self.fail_timer {
                anyhow::bail!("timer failed");
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn timer_runs_only_after_apply_succeeds_and_errors_propagate() {
        for (fail_apply, fail_timer, expected_calls, expected_error) in [
            (false, false, vec!["apply", "timer"], None),
            (true, false, vec!["apply"], Some("apply failed")),
            (false, true, vec!["apply", "timer"], Some("timer failed")),
        ] {
            let operations = FakeOperations {
                calls: RefCell::new(Vec::new()),
                fail_apply,
                fail_timer,
            };
            let result = run(&operations).await;
            assert_eq!(*operations.calls.borrow(), expected_calls);
            match expected_error {
                Some(message) => assert_eq!(result.unwrap_err().to_string(), message),
                None => result.unwrap(),
            }
        }
    }
}
