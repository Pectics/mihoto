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
