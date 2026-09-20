use crate::application::ports::{StageEvents, UpdateOperations};
use crate::application::stage::{StageReport, StageStatus};

use anyhow::{bail, Result};

pub(crate) struct UpdateOptions<'a> {
    pub(crate) config: bool,
    pub(crate) core: bool,
    pub(crate) geodata: bool,
    pub(crate) ui: bool,
    pub(crate) all: bool,
    pub(crate) arch: Option<&'a str>,
}

pub(crate) async fn run<O, E>(operations: &O, options: UpdateOptions<'_>, events: E) -> Result<()>
where
    O: UpdateOperations,
    E: StageEvents,
{
    let mut report = StageReport::new(events);

    if options.all {
        report
            .run("config", Some("refreshing remote config"), || {
                operations.update_config()
            })
            .await;
        report
            .run("geodata", Some("refreshing geodata"), || {
                operations.update_geodata()
            })
            .await;
        report
            .run("ui", Some("refreshing dashboard assets"), || {
                operations.update_ui()
            })
            .await;
        report
            .run("core", Some("refreshing mihomo core"), || {
                operations.update_core(options.arch)
            })
            .await;
        if report.has_failures() {
            report.record(
                "service restart",
                StageStatus::Skipped("skipped due to earlier failures".to_string()),
            );
        } else {
            report
                .run("service restart", Some("restarting mihomo.service"), || {
                    operations.restart_service()
                })
                .await;
        }
    } else if options.core {
        report
            .run("core", Some("refreshing mihomo core"), || {
                operations.update_core(options.arch)
            })
            .await;
        if !report.has_failures() && report.has_installed("core") {
            report
                .run("service restart", Some("restarting mihomo.service"), || {
                    operations.restart_service()
                })
                .await;
        } else if report.has_failures() {
            report.record(
                "service restart",
                StageStatus::Skipped("skipped due to earlier failures".to_string()),
            );
        } else {
            report.record(
                "service restart",
                StageStatus::Skipped("core already up to date".to_string()),
            );
        }
    } else if options.ui {
        report
            .run("ui", Some("refreshing dashboard assets"), || {
                operations.update_ui()
            })
            .await;
    } else if options.geodata {
        report
            .run("geodata", Some("refreshing geodata"), || {
                operations.update_geodata()
            })
            .await;
    } else if options.config || (!options.core && !options.geodata && !options.ui) {
        report
            .run("config", Some("refreshing remote config"), || {
                operations.update_config()
            })
            .await;
        let reason = if report.has_failures() {
            "skipped due to earlier failures"
        } else {
            "completed transactionally with config update"
        };
        report.record("service restart", StageStatus::Skipped(reason.to_string()));
    }

    report.print("update summary");
    if report.has_failures() {
        bail!("one or more update stages failed - see summary above");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::stage::StageEntry;
    use std::{
        cell::RefCell,
        future::{ready, Future},
    };

    #[derive(Default)]
    struct FakeOperations {
        calls: RefCell<Vec<&'static str>>,
    }

    impl FakeOperations {
        fn call(&self, name: &'static str) -> Result<StageStatus> {
            self.calls.borrow_mut().push(name);
            Ok(StageStatus::Installed)
        }
    }

    impl UpdateOperations for FakeOperations {
        fn update_config(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.call("config"))
        }

        fn update_geodata(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.call("geodata"))
        }

        fn update_ui(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.call("ui"))
        }

        fn update_core(&self, _arch: Option<&str>) -> impl Future<Output = Result<StageStatus>> {
            ready(self.call("core"))
        }

        fn restart_service(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.call("restart"))
        }
    }

    #[derive(Default)]
    struct SilentEvents;

    impl StageEvents for SilentEvents {
        fn begin(&mut self, _name: &'static str, _description: Option<&str>) {}
        fn summary(&mut self, _label: &str, _entries: &[StageEntry]) {}
    }

    #[tokio::test]
    async fn all_preserves_stage_and_restart_order() {
        let operations = FakeOperations::default();
        run(
            &operations,
            UpdateOptions {
                config: false,
                core: false,
                geodata: false,
                ui: false,
                all: true,
                arch: None,
            },
            SilentEvents,
        )
        .await
        .unwrap();
        assert_eq!(
            *operations.calls.borrow(),
            ["config", "geodata", "ui", "core", "restart"]
        );
    }

    #[tokio::test]
    async fn default_update_is_config_only() {
        let operations = FakeOperations::default();
        run(
            &operations,
            UpdateOptions {
                config: false,
                core: false,
                geodata: false,
                ui: false,
                all: false,
                arch: None,
            },
            SilentEvents,
        )
        .await
        .unwrap();
        assert_eq!(*operations.calls.borrow(), ["config"]);
    }
}
