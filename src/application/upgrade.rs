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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, future::ready};

    struct FakeOperations {
        calls: RefCell<Vec<String>>,
        available: Option<String>,
        fail: bool,
    }

    impl UpgradeOperations for FakeOperations {
        fn check(&self) -> impl std::future::Future<Output = Result<Option<String>>> {
            self.calls.borrow_mut().push("check".to_string());
            ready(if self.fail {
                Err(anyhow::anyhow!("check failed"))
            } else {
                Ok(self.available.clone())
            })
        }

        fn install(
            &self,
            no_confirm: bool,
            target: Option<String>,
        ) -> impl std::future::Future<Output = Result<()>> {
            self.calls
                .borrow_mut()
                .push(format!("install:{no_confirm}:{target:?}"));
            ready(if self.fail {
                Err(anyhow::anyhow!("install failed"))
            } else {
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn check_and_install_are_distinct_paths_with_exact_options() {
        let check = FakeOperations {
            calls: RefCell::new(Vec::new()),
            available: Some("v2.0.0".to_string()),
            fail: false,
        };
        match run(&check, false, true, Some("ignored".to_string()))
            .await
            .unwrap()
        {
            UpgradeOutcome::Checked(version) => assert_eq!(version.as_deref(), Some("v2.0.0")),
            UpgradeOutcome::Installed => panic!("expected check outcome"),
        }
        assert_eq!(*check.calls.borrow(), ["check"]);

        let install = FakeOperations {
            calls: RefCell::new(Vec::new()),
            available: None,
            fail: false,
        };
        assert!(matches!(
            run(&install, true, false, Some("aarch64".to_string()))
                .await
                .unwrap(),
            UpgradeOutcome::Installed
        ));
        assert_eq!(*install.calls.borrow(), ["install:true:Some(\"aarch64\")"]);
    }

    #[tokio::test]
    async fn operation_errors_propagate() {
        let operations = FakeOperations {
            calls: RefCell::new(Vec::new()),
            available: None,
            fail: true,
        };
        assert_eq!(
            run(&operations, false, true, None)
                .await
                .err()
                .unwrap()
                .to_string(),
            "check failed"
        );
        assert_eq!(
            run(&operations, false, false, None)
                .await
                .err()
                .unwrap()
                .to_string(),
            "install failed"
        );
    }
}
