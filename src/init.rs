use crate::config::{load_config, validate_config, write_default_if_missing, Config};
use crate::mihoto::{BinaryPlan, Mihoto, StageStatus};
use crate::timer;

use std::{future::Future, path::Path};

use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::Input;
use reqwest::Client;
use serde_yaml::Value as YamlValue;

pub struct InitOptions {
    pub force: bool,
    pub arch: Option<String>,
    pub yes: bool,
}

// ---------------------------------------------------------------------------
// Stage report
// ---------------------------------------------------------------------------

struct StageReport {
    entries: Vec<(&'static str, StageStatus)>,
}

impl StageReport {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Print the stage header and optional first detail line.
    fn begin(&self, name: &'static str, description: Option<&str>) {
        println!("{} {}", "●".cyan().bold(), name.bold());
        if let Some(description) = description {
            println!("{}  {}", " ⎿".cyan().bold(), description.italic().dimmed());
        }
    }

    async fn run<F, Fut>(&mut self, name: &'static str, description: Option<&str>, f: F)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<StageStatus>>,
    {
        self.begin(name, description);
        let status = match f().await {
            Ok(s) => s,
            Err(e) => StageStatus::Failed(e),
        };
        self.entries.push((name, status));
    }

    /// Record a pre-computed status without printing a header.  Pair with
    /// [`Self::begin`] for stages whose header must appear *before* the work runs
    /// (e.g. the binary download / install split, where ownership constraints
    /// make the closure pattern awkward).
    fn record(&mut self, name: &'static str, status: StageStatus) {
        self.entries.push((name, status));
    }

    fn print(&self) {
        println!("{} {}", "mihoto:".cyan().bold(), "init summary".bold());
        for (name, status) in &self.entries {
            match status {
                StageStatus::Installed => {
                    println!("  {} {}", "✓".green().bold(), name);
                }
                StageStatus::Skipped(reason) => {
                    println!("  {} {} ({})", "↷".dimmed(), name.dimmed(), reason.dimmed());
                }
                StageStatus::Failed(err) => {
                    println!("  {} {}: {:#}", "✗".red().bold(), name.red(), err);
                }
            }
        }
    }

    fn has_failures(&self) -> bool {
        self.entries
            .iter()
            .any(|(_, s)| matches!(s, StageStatus::Failed(_)))
    }

    fn stage_failed(&self, name: &'static str) -> bool {
        self.entries
            .iter()
            .any(|(n, s)| *n == name && matches!(s, StageStatus::Failed(_)))
    }
}

// ---------------------------------------------------------------------------
// Dashboard URL helper
// ---------------------------------------------------------------------------

fn dashboard_url(config: &Config, mihomo_config_path: &str) -> Option<String> {
    let controller = config
        .mihomo_config
        .external_controller
        .clone()
        .or_else(|| remote_string_field(mihomo_config_path, "external-controller"))?;
    let (host, port) = controller.rsplit_once(':')?;
    let host = match host {
        "0.0.0.0" | "[::]" | "" => "127.0.0.1",
        h => h,
    };
    Some(format!("http://{host}:{port}/ui/"))
}

fn remote_string_field(path: &str, key: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: YamlValue = serde_yaml::from_str(&raw).ok()?;
    value
        .get(key)
        .and_then(YamlValue::as_str)
        .map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Interactive bootstrap
// ---------------------------------------------------------------------------

fn prompt_subscription_url() -> Result<String> {
    let url: String = Input::new()
        .with_prompt("Remote subscription URL")
        .validate_with(|s: &String| {
            if s.trim().is_empty() {
                Err("URL cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()?;
    Ok(url.trim().to_string())
}

fn bootstrap_config(config_path: &str, yes: bool) -> Result<Config> {
    let just_created = write_default_if_missing(config_path)?;

    // After write_default_if_missing the file always exists.
    let mut config =
        load_config(config_path)?.expect("config file must exist after write_default_if_missing");

    if config.remote_config_url.is_empty() {
        if yes {
            bail!(
				"`remote_config_url` is not set - edit `{config_path}` or run `mihoto init` interactively"
			);
        }

        if just_created {
            println!(
                "{} Created default config at {}",
                "mihoto:".cyan(),
                config_path.underline().yellow()
            );
        }
        println!("{} Enter your remote subscription URL:", "mihoto:".yellow());
        config.remote_config_url = prompt_subscription_url()?;
        config.write(Path::new(config_path))?;
        println!("{} Saved", "mihoto:".green());
    }

    Ok(config)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn run(config_path: &str, client: &Client, opts: InitOptions) -> Result<()> {
    let config_path = std::borrow::Cow::Borrowed(config_path);
    let config_path = config_path.as_ref();
    let config = bootstrap_config(config_path, opts.yes)?;
    validate_config(&config)?;

    let mihoto = Mihoto::from_config(config.clone());

    println!("{} initializing", "mihoto:".cyan().bold());

    let force = opts.force;
    let arch = opts.arch.as_deref();
    let mut report = StageReport::new();

    // --- download phase -------------------------------------------------------
    //
    // Binary is downloaded first so that the running mihomo proxy is still alive
    // for subsequent requests (config / geodata / UI may all go through
    // https_proxy=http://127.0.0.1:<port>).  The actual service stop + binary
    // swap is deferred to the "install binary" stage after all downloads finish.

    report.begin("mihomo binary", Some("downloading mihomo binary"));
    let binary_temp = match mihoto.prepare_binary(client, force, arch).await {
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
            || mihoto.ensure_remote_config(client, force),
        )
        .await;
    report
        .run("geodata", Some("downloading geodata"), || {
            mihoto.ensure_geodata(client, force)
        })
        .await;
    report
        .run(
            "web dashboard",
            Some("downloading dashboard assets"),
            || mihoto.ensure_ui(client, force),
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
            Some(temp) => match mihoto.install_binary(temp).await {
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
            let validation_status = match mihoto.validate_installed_config() {
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
                    || async { mihoto.ensure_service().await },
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
                        || async { mihoto.ensure_service_running().await },
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
                            || async {
                                timer::reconcile(&config, config_path)?;
                                if config.auto_update_interval == 0 {
                                    Ok(StageStatus::Skipped(
                                        "disabled by auto_update_interval".to_string(),
                                    ))
                                } else {
                                    Ok(StageStatus::Installed)
                                }
                            },
                        )
                        .await;
                }
            }
        }
    }

    report.print();

    // Print dashboard URL if UI is configured
    if config.ui.is_some() {
        if let Some(url) = dashboard_url(&config, &mihoto.mihomo_target_config_path) {
            println!();
            println!("  {}: {}", "Dashboard".bold(), url.underline().cyan());
            let ui_name = config
                .ui
                .as_ref()
                .map(|u| u.as_config_value())
                .unwrap_or("ui");
            println!(
                "  Using {} - change via the {} field in {}",
                ui_name.bold(),
                "`ui`".bold(),
                "mihoto.toml".underline()
            );
            if config.mihomo_config.secret.is_some() {
                println!("  Authentication required (secret is set in mihoto.toml)");
            } else {
                println!(
                    "  Set {} in {} to require a password",
                    "`mihomo_config.secret`".bold(),
                    "mihoto.toml".underline()
                );
            }
        }
    }

    if report.has_failures() {
        bail!("one or more stages failed - see summary above");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MihomoConfig;
    use anyhow::anyhow;
    use std::fs;

    #[tokio::test]
    async fn stage_report_records_all_statuses_and_failure_queries() {
        let mut report = StageReport::new();
        report.begin("fixture", Some("testing stage output"));
        report.record("installed", StageStatus::Installed);
        report.record("skipped", StageStatus::Skipped("already done".into()));
        report
            .run("successful", None, || async { Ok(StageStatus::Installed) })
            .await;
        report
            .run("failed", None, || async { Err(anyhow!("fixture failure")) })
            .await;

        assert!(report.has_failures());
        assert!(report.stage_failed("failed"));
        assert!(!report.stage_failed("missing"));
        report.print();
    }

    #[test]
    fn bootstrap_config_requires_url_in_yes_mode_and_loads_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let missing_url = dir.path().join("missing-url.toml");
        assert!(bootstrap_config(missing_url.to_str().unwrap(), true).is_err());
        assert!(missing_url.exists());

        let configured_path = dir.path().join("configured.toml");
        let mut configured = Config {
            remote_config_url: "https://example.test/config.yaml".into(),
            ..Config::default()
        };
        configured.write(&configured_path).unwrap();
        let loaded = bootstrap_config(configured_path.to_str().unwrap(), true).unwrap();
        assert_eq!(loaded.remote_config_url, "https://example.test/config.yaml");
    }

    #[tokio::test]
    async fn init_aborts_install_phases_when_remote_config_stage_fails() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/config.yaml"))
            .respond_with(ResponseTemplate::new(200).set_body_string("tun: [invalid"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let binary_path = dir.path().join("mihomo");
        fs::write(&binary_path, "existing binary").unwrap();
        let config_path = dir.path().join("mihoto.toml");
        let config = Config {
            remote_config_url: format!("{}/config.yaml", server.uri()),
            ui: None,
            mihomo_binary_path: binary_path.display().to_string(),
            mihomo_config_root: dir.path().join("mihomo-config").display().to_string(),
            ..Config::default()
        };
        let mut writable = config.clone();
        writable.write(&config_path).unwrap();

        let error = run(
            config_path.to_str().unwrap(),
            &Client::new(),
            InitOptions {
                force: true,
                arch: None,
                yes: true,
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("one or more stages failed"));
        assert!(!dir.path().join("mihomo-config/config.yaml").exists());
        assert_eq!(fs::read_to_string(&binary_path).unwrap(), "existing binary");
    }

    #[test]
    fn dashboard_url_prefers_local_controller_and_normalizes_wildcards() {
        let dir = tempfile::tempdir().unwrap();
        let remote_path = dir.path().join("config.yaml");
        fs::write(&remote_path, "external-controller: 0.0.0.0:9090\n").unwrap();
        let mut config = Config::default();

        assert_eq!(
            dashboard_url(&config, remote_path.to_str().unwrap()).as_deref(),
            Some("http://127.0.0.1:9090/ui/")
        );

        config.mihomo_config = MihomoConfig {
            external_controller: Some("[::1]:19090".into()),
            ..MihomoConfig::default()
        };
        assert_eq!(
            dashboard_url(&config, remote_path.to_str().unwrap()).as_deref(),
            Some("http://[::1]:19090/ui/")
        );
    }

    #[test]
    fn dashboard_url_rejects_missing_or_malformed_controller_values() {
        let dir = tempfile::tempdir().unwrap();
        let remote_path = dir.path().join("config.yaml");
        let config = Config::default();

        fs::write(&remote_path, "external-controller: 127.0.0.1\n").unwrap();
        assert!(dashboard_url(&config, remote_path.to_str().unwrap()).is_none());

        fs::write(&remote_path, "external-controller: [broken\n").unwrap();
        assert!(dashboard_url(&config, remote_path.to_str().unwrap()).is_none());

        assert!(dashboard_url(&config, "/nonexistent/config.yaml").is_none());
    }
}
