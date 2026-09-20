use crate::application::ports::{BinaryPlan, InitEvents, InitOperations};
use crate::application::stage::{StageReport, StageStatus};
use crate::domain::config::Config;

use anyhow::{bail, Result};

pub struct InitOptions {
    pub force: bool,
    pub arch: Option<String>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn run<O, E>(
    config_path: &str,
    config: &Config,
    operations: &O,
    opts: InitOptions,
    mut events: E,
) -> Result<()>
where
    O: InitOperations,
    E: InitEvents,
{
    events.initializing();

    let force = opts.force;
    let arch = opts.arch.as_deref();
    let mut report = StageReport::new(events);

    // --- download phase -------------------------------------------------------
    //
    // Binary is downloaded first so that the running mihomo proxy is still alive
    // for subsequent requests (config / geodata / UI may all go through
    // https_proxy=http://127.0.0.1:<port>).  The actual service stop + binary
    // swap is deferred to the "install binary" stage after all downloads finish.

    report.begin("mihomo binary", Some("downloading mihomo binary"));
    let binary_temp = match operations.prepare_binary(force, arch).await {
        Ok(BinaryPlan::Install(temp)) => {
            report.record("mihomo binary", StageStatus::Installed);
            Some(temp)
        }
        Ok(BinaryPlan::Skip(reason)) => {
            report.record("mihomo binary", StageStatus::Skipped(reason));
            None
        }
        Err(e) => {
            report.record("mihomo binary", StageStatus::Failed(e));
            None
        }
    };

    report
        .run(
            "remote config",
            Some("downloading and merging remote config"),
            || operations.ensure_remote_config(force),
        )
        .await;
    report
        .run("geodata", Some("downloading geodata"), || {
            operations.ensure_geodata(force)
        })
        .await;
    report
        .run(
            "web dashboard",
            Some("downloading dashboard assets"),
            || operations.ensure_ui(force),
        )
        .await;

    // --- install phase --------------------------------------------------------
    //
    // All network calls are done.  Now it is safe to stop the running service
    // (which also tears down the proxy) and swap in the new binary.
    //
    // If remote config stage failed we must not proceed: installing a new
    // binary or restarting mihomo.service on top of a missing / corrupt config.yaml
    // would break an environment that may have been working before.

    if report.stage_failed("remote config") || report.stage_failed("mihomo binary") {
        let reason = if report.stage_failed("remote config") {
            "remote config stage failed"
        } else {
            "mihomo binary stage failed"
        };
        let skip = || StageStatus::Skipped(format!("skipped: {reason}"));
        report.record("install binary", skip());
        report.record("systemd service", skip());
        report.record("service start", skip());
        report.record("update timer", skip());
    } else {
        report.begin("install binary", Some("installing mihomo binary"));
        let install_status = match binary_temp {
            None => StageStatus::Skipped("nothing to install".to_string()),
            Some(temp) => match operations.install_binary(temp).await {
                Ok(s) => s,
                Err(e) => StageStatus::Failed(e),
            },
        };
        report.record("install binary", install_status);

        if !report.stage_failed("install binary") {
            report.begin(
                "config validation",
                Some("validating config with the installed Mihomo core"),
            );
            let validation_status = match operations.validate_installed_config() {
                Ok(status) => status,
                Err(error) => StageStatus::Failed(error),
            };
            report.record("config validation", validation_status);
        }

        if report.stage_failed("install binary") || report.stage_failed("config validation") {
            let reason = if report.stage_failed("install binary") {
                "binary installation failed"
            } else {
                "config validation failed"
            };
            report.record(
                "systemd service",
                StageStatus::Skipped(format!("skipped: {reason}")),
            );
            report.record(
                "service start",
                StageStatus::Skipped(format!("skipped: {reason}")),
            );
            report.record(
                "update timer",
                StageStatus::Skipped(format!("skipped: {reason}")),
            );
        } else {
            report
                .run(
                    "systemd service",
                    Some("writing systemd service"),
                    || async { operations.ensure_service().await },
                )
                .await;
            if report.stage_failed("systemd service") {
                report.record(
                    "service start",
                    StageStatus::Skipped("skipped: systemd service stage failed".to_string()),
                );
                report.record(
                    "update timer",
                    StageStatus::Skipped("skipped: systemd service stage failed".to_string()),
                );
            } else {
                report
                    .run(
                        "service start",
                        Some("starting and enabling mihomo.service"),
                        || async { operations.ensure_service_running().await },
                    )
                    .await;
                if report.has_failures() {
                    report.record(
                        "update timer",
                        StageStatus::Skipped("skipped due to earlier failures".to_string()),
                    );
                } else {
                    report
                        .run(
                            "update timer",
                            Some("reconciling mihoto-update.timer"),
                            || async { operations.reconcile_timer(config_path) },
                        )
                        .await;
                }
            }
        }
    }

    report.print("init summary");

    report.events_mut().dashboard(config);

    if report.has_failures() {
        bail!("one or more stages failed - see summary above");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::StageEvents;
    use crate::application::stage::StageEntry;
    use std::{
        cell::RefCell,
        future::{ready, Future},
    };

    struct FakeOperations {
        calls: RefCell<Vec<&'static str>>,
        fail_remote: bool,
    }

    impl FakeOperations {
        fn status(&self, name: &'static str) -> Result<StageStatus> {
            self.calls.borrow_mut().push(name);
            Ok(StageStatus::Installed)
        }
    }

    impl InitOperations for FakeOperations {
        type BinaryHandle = &'static str;

        fn prepare_binary(
            &self,
            _force: bool,
            _arch: Option<&str>,
        ) -> impl Future<Output = Result<BinaryPlan<Self::BinaryHandle>>> {
            self.calls.borrow_mut().push("prepare binary");
            ready(Ok(BinaryPlan::Install("binary")))
        }

        fn install_binary(
            &self,
            _handle: Self::BinaryHandle,
        ) -> impl Future<Output = Result<StageStatus>> {
            ready(self.status("install binary"))
        }

        fn ensure_remote_config(&self, _force: bool) -> impl Future<Output = Result<StageStatus>> {
            self.calls.borrow_mut().push("remote config");
            ready(if self.fail_remote {
                Err(anyhow::anyhow!("remote failed"))
            } else {
                Ok(StageStatus::Installed)
            })
        }

        fn ensure_geodata(&self, _force: bool) -> impl Future<Output = Result<StageStatus>> {
            ready(self.status("geodata"))
        }

        fn ensure_ui(&self, _force: bool) -> impl Future<Output = Result<StageStatus>> {
            ready(self.status("ui"))
        }

        fn validate_installed_config(&self) -> Result<StageStatus> {
            self.status("validate")
        }

        fn ensure_service(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.status("service"))
        }

        fn ensure_service_running(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.status("service running"))
        }

        fn reconcile_timer(&self, _config_path: &str) -> Result<StageStatus> {
            self.status("timer")
        }
    }

    #[derive(Default)]
    struct SilentEvents;

    impl StageEvents for SilentEvents {
        fn begin(&mut self, _name: &'static str, _description: Option<&str>) {}
        fn summary(&mut self, _label: &str, _entries: &[StageEntry]) {}
    }

    impl InitEvents for SilentEvents {
        fn initializing(&mut self) {}
        fn dashboard(&mut self, _config: &Config) {}
    }

    fn options() -> InitOptions {
        InitOptions {
            force: false,
            arch: None,
        }
    }

    #[tokio::test]
    async fn downloads_finish_before_binary_install_and_service_work() {
        let operations = FakeOperations {
            calls: RefCell::new(Vec::new()),
            fail_remote: false,
        };
        run(
            "/etc/mihoto.toml",
            &Config::default(),
            &operations,
            options(),
            SilentEvents,
        )
        .await
        .unwrap();
        assert_eq!(
            *operations.calls.borrow(),
            [
                "prepare binary",
                "remote config",
                "geodata",
                "ui",
                "install binary",
                "validate",
                "service",
                "service running",
                "timer"
            ]
        );
    }

    #[tokio::test]
    async fn remote_failure_skips_install_and_service_work() {
        let operations = FakeOperations {
            calls: RefCell::new(Vec::new()),
            fail_remote: true,
        };
        assert!(run(
            "/etc/mihoto.toml",
            &Config::default(),
            &operations,
            options(),
            SilentEvents,
        )
        .await
        .is_err());
        assert_eq!(
            *operations.calls.borrow(),
            ["prepare binary", "remote config", "geodata", "ui"]
        );
    }
}

#[cfg(test)]
mod branch_tests {
    use super::*;
    use crate::application::{ports::StageEvents, stage::StageEntry};
    use std::{
        cell::RefCell,
        future::{ready, Future},
        rc::Rc,
    };

    struct ScenarioOperations {
        calls: RefCell<Vec<String>>,
        fail_on: Option<&'static str>,
        skip_binary: bool,
    }

    impl ScenarioOperations {
        fn new(fail_on: Option<&'static str>, skip_binary: bool) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail_on,
                skip_binary,
            }
        }

        fn status(&self, name: &'static str) -> Result<StageStatus> {
            self.calls.borrow_mut().push(name.to_string());
            if self.fail_on == Some(name) {
                anyhow::bail!("{name} failed");
            }
            Ok(StageStatus::Installed)
        }
    }

    impl InitOperations for ScenarioOperations {
        type BinaryHandle = &'static str;

        fn prepare_binary(
            &self,
            force: bool,
            arch: Option<&str>,
        ) -> impl Future<Output = Result<BinaryPlan<Self::BinaryHandle>>> {
            self.calls
                .borrow_mut()
                .push(format!("prepare:{force}:{arch:?}"));
            ready(if self.fail_on == Some("prepare") {
                Err(anyhow::anyhow!("prepare failed"))
            } else if self.skip_binary {
                Ok(BinaryPlan::Skip("already installed".to_string()))
            } else {
                Ok(BinaryPlan::Install("binary handle"))
            })
        }

        fn install_binary(
            &self,
            handle: Self::BinaryHandle,
        ) -> impl Future<Output = Result<StageStatus>> {
            assert_eq!(handle, "binary handle");
            ready(self.status("install"))
        }

        fn ensure_remote_config(&self, force: bool) -> impl Future<Output = Result<StageStatus>> {
            self.calls
                .borrow_mut()
                .push(format!("remote force:{force}"));
            ready(if self.fail_on == Some("remote") {
                Err(anyhow::anyhow!("remote failed"))
            } else {
                Ok(StageStatus::Installed)
            })
        }

        fn ensure_geodata(&self, force: bool) -> impl Future<Output = Result<StageStatus>> {
            self.calls
                .borrow_mut()
                .push(format!("geodata force:{force}"));
            ready(if self.fail_on == Some("geodata") {
                Err(anyhow::anyhow!("geodata failed"))
            } else {
                Ok(StageStatus::Installed)
            })
        }

        fn ensure_ui(&self, force: bool) -> impl Future<Output = Result<StageStatus>> {
            self.calls.borrow_mut().push(format!("ui force:{force}"));
            ready(if self.fail_on == Some("ui") {
                Err(anyhow::anyhow!("ui failed"))
            } else {
                Ok(StageStatus::Installed)
            })
        }

        fn validate_installed_config(&self) -> Result<StageStatus> {
            self.status("validate")
        }

        fn ensure_service(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.status("service"))
        }

        fn ensure_service_running(&self) -> impl Future<Output = Result<StageStatus>> {
            ready(self.status("service running"))
        }

        fn reconcile_timer(&self, config_path: &str) -> Result<StageStatus> {
            self.calls.borrow_mut().push(format!("timer:{config_path}"));
            if self.fail_on == Some("timer") {
                anyhow::bail!("timer failed");
            }
            Ok(StageStatus::Installed)
        }
    }

    #[derive(Default)]
    struct EventLog {
        initializing: usize,
        dashboard: usize,
        summaries: Vec<Vec<String>>,
    }

    struct RecordingEvents(Rc<RefCell<EventLog>>);

    impl StageEvents for RecordingEvents {
        fn begin(&mut self, _name: &'static str, _description: Option<&str>) {}

        fn summary(&mut self, _label: &str, entries: &[StageEntry]) {
            self.0.borrow_mut().summaries.push(
                entries
                    .iter()
                    .map(|entry| format!("{}:{:?}", entry.name, entry.status))
                    .collect(),
            );
        }
    }

    impl InitEvents for RecordingEvents {
        fn initializing(&mut self) {
            self.0.borrow_mut().initializing += 1;
        }

        fn dashboard(&mut self, _config: &Config) {
            self.0.borrow_mut().dashboard += 1;
        }
    }

    fn events() -> (RecordingEvents, Rc<RefCell<EventLog>>) {
        let log = Rc::new(RefCell::new(EventLog::default()));
        (RecordingEvents(log.clone()), log)
    }

    #[tokio::test]
    async fn options_and_config_path_are_forwarded_when_binary_is_current() {
        let operations = ScenarioOperations::new(None, true);
        let (events, log) = events();
        run(
            "/custom/mihoto.toml",
            &Config::default(),
            &operations,
            InitOptions {
                force: true,
                arch: Some("arm64".to_string()),
            },
            events,
        )
        .await
        .unwrap();

        assert_eq!(
            *operations.calls.borrow(),
            [
                "prepare:true:Some(\"arm64\")",
                "remote force:true",
                "geodata force:true",
                "ui force:true",
                "validate",
                "service",
                "service running",
                "timer:/custom/mihoto.toml"
            ]
        );
        assert_eq!(log.borrow().initializing, 1);
        assert_eq!(log.borrow().dashboard, 1);
    }

    #[tokio::test]
    async fn binary_and_remote_failures_skip_every_install_phase() {
        for failure in ["prepare", "remote"] {
            let operations = ScenarioOperations::new(Some(failure), false);
            let (events, log) = events();
            assert!(run(
                "/etc/mihoto.toml",
                &Config::default(),
                &operations,
                InitOptions {
                    force: false,
                    arch: None,
                },
                events,
            )
            .await
            .is_err());
            assert_eq!(operations.calls.borrow().len(), 4);
            assert!(operations.calls.borrow()[1].starts_with("remote"));
            assert!(operations.calls.borrow()[2].starts_with("geodata"));
            assert!(operations.calls.borrow()[3].starts_with("ui"));
            assert_eq!(log.borrow().dashboard, 1);
            assert!(log.borrow().summaries[0]
                .iter()
                .any(|entry| entry.starts_with("update timer:Skipped")));
        }
    }

    #[tokio::test]
    async fn sequential_failures_stop_only_the_dependent_tail() {
        for (failure, expected_last_call) in [
            ("install", "install"),
            ("validate", "validate"),
            ("service", "service"),
            ("service running", "service running"),
            ("timer", "timer:/etc/mihoto.toml"),
        ] {
            let operations = ScenarioOperations::new(Some(failure), false);
            let (events, log) = events();
            let result = run(
                "/etc/mihoto.toml",
                &Config::default(),
                &operations,
                InitOptions {
                    force: false,
                    arch: None,
                },
                events,
            )
            .await;
            assert!(result.is_err(), "{failure} should fail init");
            assert_eq!(
                operations.calls.borrow().last().unwrap(),
                expected_last_call
            );
            assert_eq!(log.borrow().dashboard, 1);
        }
    }

    #[tokio::test]
    async fn optional_download_failure_allows_service_setup_but_skips_timer() {
        for failure in ["geodata", "ui"] {
            let operations = ScenarioOperations::new(Some(failure), false);
            let (events, log) = events();
            assert!(run(
                "/etc/mihoto.toml",
                &Config::default(),
                &operations,
                InitOptions {
                    force: false,
                    arch: None,
                },
                events,
            )
            .await
            .is_err());
            assert_eq!(operations.calls.borrow().last().unwrap(), "service running");
            assert_eq!(
                log.borrow().summaries[0].last().unwrap(),
                "update timer:Skipped(\"skipped due to earlier failures\")"
            );
        }
    }
}
