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
        rc::Rc,
    };

    struct FakeOperations {
        calls: RefCell<Vec<String>>,
        fail_on: Option<&'static str>,
        skip_core: bool,
    }

    impl FakeOperations {
        fn call(&self, name: &'static str) -> Result<StageStatus> {
            self.calls.borrow_mut().push(name.to_string());
            if self.fail_on == Some(name) {
                anyhow::bail!("{name} failed");
            }
            if name == "core" && self.skip_core {
                return Ok(StageStatus::Skipped("already current".to_string()));
            }
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

        fn update_core(&self, arch: Option<&str>) -> impl Future<Output = Result<StageStatus>> {
            self.calls.borrow_mut().push(format!("arch:{arch:?}"));
            ready(self.call("core"))
        }

        fn restart_service(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.call("restart"))
        }
    }

    #[derive(Default)]
    struct EventLog {
        begins: Vec<&'static str>,
        summaries: Vec<(String, Vec<String>)>,
    }

    struct RecordingEvents(Rc<RefCell<EventLog>>);

    impl StageEvents for RecordingEvents {
        fn begin(&mut self, name: &'static str, _description: Option<&str>) {
            self.0.borrow_mut().begins.push(name);
        }

        fn summary(&mut self, label: &str, entries: &[StageEntry]) {
            self.0.borrow_mut().summaries.push((
                label.to_string(),
                entries
                    .iter()
                    .map(|entry| format!("{}:{:?}", entry.name, entry.status))
                    .collect(),
            ));
        }
    }

    fn operations(fail_on: Option<&'static str>, skip_core: bool) -> FakeOperations {
        FakeOperations {
            calls: RefCell::new(Vec::new()),
            fail_on,
            skip_core,
        }
    }

    fn options() -> UpdateOptions<'static> {
        UpdateOptions {
            config: false,
            core: false,
            geodata: false,
            ui: false,
            all: false,
            arch: None,
        }
    }

    fn events() -> (RecordingEvents, Rc<RefCell<EventLog>>) {
        let log = Rc::new(RefCell::new(EventLog::default()));
        (RecordingEvents(log.clone()), log)
    }

    #[tokio::test]
    async fn all_preserves_stage_and_restart_order() {
        let operations = operations(None, false);
        let (events, log) = events();
        run(
            &operations,
            UpdateOptions {
                all: true,
                ..options()
            },
            events,
        )
        .await
        .unwrap();
        assert_eq!(
            *operations.calls.borrow(),
            ["config", "geodata", "ui", "arch:None", "core", "restart"]
        );
        assert_eq!(
            log.borrow().begins,
            ["config", "geodata", "ui", "core", "service restart"]
        );
    }

    #[tokio::test]
    async fn default_update_is_config_only() {
        let operations = operations(None, false);
        let (events, log) = events();
        run(&operations, options(), events).await.unwrap();
        assert_eq!(*operations.calls.borrow(), ["config"]);
        assert_eq!(
            log.borrow().summaries[0].1,
            [
                "config:Installed",
                "service restart:Skipped(\"completed transactionally with config update\")"
            ]
        );
    }

    #[tokio::test]
    async fn all_continues_download_stages_after_failure_and_skips_restart() {
        let operations = operations(Some("geodata"), false);
        let (events, log) = events();
        let result = run(
            &operations,
            UpdateOptions {
                all: true,
                ..options()
            },
            events,
        )
        .await;
        assert_eq!(
            result.unwrap_err().to_string(),
            "one or more update stages failed - see summary above"
        );
        assert_eq!(
            *operations.calls.borrow(),
            ["config", "geodata", "ui", "arch:None", "core"]
        );
        assert_eq!(
            log.borrow().summaries[0].1.last().unwrap(),
            "service restart:Skipped(\"skipped due to earlier failures\")"
        );
    }

    #[tokio::test]
    async fn core_restart_depends_on_install_status_and_forwards_arch() {
        for (fail_on, skip_core, expected_calls, expected_tail, succeeds) in [
            (
                None,
                false,
                vec!["arch:Some(\"arm64\")", "core", "restart"],
                "service restart:Installed",
                true,
            ),
            (
                None,
                true,
                vec!["arch:Some(\"arm64\")", "core"],
                "service restart:Skipped(\"core already up to date\")",
                true,
            ),
            (
                Some("core"),
                false,
                vec!["arch:Some(\"arm64\")", "core"],
                "service restart:Skipped(\"skipped due to earlier failures\")",
                false,
            ),
        ] {
            let operations = operations(fail_on, skip_core);
            let (events, log) = events();
            let result = run(
                &operations,
                UpdateOptions {
                    core: true,
                    arch: Some("arm64"),
                    ..options()
                },
                events,
            )
            .await;
            assert_eq!(*operations.calls.borrow(), expected_calls);
            assert_eq!(log.borrow().summaries[0].1.last().unwrap(), expected_tail);
            assert_eq!(result.is_ok(), succeeds);
        }
    }

    #[tokio::test]
    async fn explicit_single_resource_flags_are_mutually_exclusive_by_priority() {
        for (options, expected) in [
            (
                UpdateOptions {
                    ui: true,
                    geodata: true,
                    config: true,
                    ..options()
                },
                "ui",
            ),
            (
                UpdateOptions {
                    geodata: true,
                    config: true,
                    ..options()
                },
                "geodata",
            ),
            (
                UpdateOptions {
                    config: true,
                    ..options()
                },
                "config",
            ),
        ] {
            let operations = operations(None, false);
            let (events, _) = events();
            run(&operations, options, events).await.unwrap();
            assert_eq!(*operations.calls.borrow(), [expected]);
        }
    }

    #[tokio::test]
    async fn config_failure_is_reported_and_restart_is_skipped() {
        let operations = operations(Some("config"), false);
        let (events, log) = events();
        assert!(run(&operations, options(), events).await.is_err());
        assert_eq!(
            log.borrow().summaries[0].1.last().unwrap(),
            "service restart:Skipped(\"skipped due to earlier failures\")"
        );
    }

    #[tokio::test]
    async fn restart_failure_turns_successful_core_or_all_updates_into_errors() {
        for options in [
            UpdateOptions {
                core: true,
                ..options()
            },
            UpdateOptions {
                all: true,
                ..options()
            },
        ] {
            let operations = operations(Some("restart"), false);
            let (events, log) = events();
            assert!(run(&operations, options, events).await.is_err());
            assert_eq!(
                log.borrow().summaries[0].1.last().unwrap(),
                "service restart:Failed(restart failed)"
            );
        }
    }
}
