mod cmd;
mod config;
mod config_store;
mod cron;
mod deployment;
mod init;
mod mihoro;
mod proxy;
mod resolve_mihomo_bin;
mod schedule;
mod source;
mod systemctl;
mod ui;
#[cfg(feature = "self_update")]
mod upgrade;
mod utils;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{
    generate,
    shells::{Bash, Fish, Zsh},
};
use colored::Colorize;
use reqwest::Client;
use std::{future::Future, io, process::Command, time::Duration};

use cmd::{Args, ClapShell, Commands, DeploymentBackendArg};
use mihoro::{Mihoro, StageStatus};
use systemctl::{journalctl_args, Systemctl};

struct StageReport {
    entries: Vec<(&'static str, StageStatus)>,
}

impl StageReport {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

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
            Ok(status) => status,
            Err(err) => StageStatus::Failed(err),
        };
        self.entries.push((name, status));
    }

    fn record(&mut self, name: &'static str, status: StageStatus) {
        self.entries.push((name, status));
    }

    fn print(&self, label: &str) {
        println!("{} {}", "mihoro:".cyan().bold(), label.bold());
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
            .any(|(_, status)| matches!(status, StageStatus::Failed(_)))
    }

    fn has_installed(&self, name: &'static str) -> bool {
        self.entries.iter().any(|(entry_name, status)| {
            *entry_name == name && matches!(status, StageStatus::Installed)
        })
    }
}

#[tokio::main]
async fn main() {
    if let Err(err) = cli().await {
        eprintln!("{} {}", "error:".bright_red().bold(), err);
        std::process::exit(1);
    }
}

async fn cli() -> Result<()> {
    let args = Args::parse();
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()?;

    // Handle Init and Setup before constructing Mihoro, which requires a valid config.
    match &args.command {
        Some(Commands::Init {
            force,
            arch,
            yes,
            backend,
        }) => {
            return init::run(
                &args.mihoro_config,
                &client,
                init::InitOptions {
                    force: *force,
                    arch: arch.clone(),
                    yes: *yes,
                    backend: backend.map(deployment_backend_from_arg),
                },
            )
            .await;
        }
        Some(Commands::Setup { overwrite, arch }) => {
            eprintln!(
                "{} `setup` is deprecated - use `mihoro init` instead",
                "warning:".yellow()
            );
            return init::run(
                &args.mihoro_config,
                &client,
                init::InitOptions {
                    force: *overwrite,
                    arch: arch.clone(),
                    yes: true,
                    backend: None,
                },
            )
            .await;
        }
        _ => {}
    }

    let mihoro = Mihoro::new(&args.mihoro_config)?;

    match &args.command {
        Some(Commands::Init { .. }) | Some(Commands::Setup { .. }) => unreachable!(),
        Some(Commands::Update {
            profile,
            config,
            core,
            geodata,
            all,
            arch,
            ui,
        }) => {
            println!("{} update initiated", "mihoro:".cyan().bold());
            let mut report = StageReport::new();

            if *all {
                report
                    .run("config", Some("refreshing remote config"), || {
                        mihoro.update_config(&client, profile.as_deref())
                    })
                    .await;
                report
                    .run("geodata", Some("refreshing geodata"), || {
                        mihoro.update_geodata(&client)
                    })
                    .await;
                report
                    .run("ui", Some("refreshing dashboard assets"), || {
                        mihoro.update_ui(&client)
                    })
                    .await;
                report
                    .run("core", Some("refreshing mihomo core"), || {
                        mihoro.update_core(&client, arch.as_deref())
                    })
                    .await;
                if !report.has_failures()
                    && (report.has_installed("config") || report.has_installed("core"))
                {
                    report
                        .run("service restart", Some("restarting mihomo.service"), || {
                            mihoro.restart_service_with_config_rollback()
                        })
                        .await;
                } else {
                    report.record(
                        "service restart",
                        StageStatus::Skipped(
                            if report.has_failures() {
                                "skipped due to earlier failures"
                            } else {
                                "nothing changed that requires restart"
                            }
                            .to_string(),
                        ),
                    );
                }
            } else if *core {
                report
                    .run("core", Some("refreshing mihomo core"), || {
                        mihoro.update_core(&client, arch.as_deref())
                    })
                    .await;
                if !report.has_failures() && report.has_installed("core") {
                    report
                        .run("service restart", Some("restarting mihomo.service"), || {
                            mihoro.restart_service()
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
            } else if *ui {
                report
                    .run("ui", Some("refreshing dashboard assets"), || {
                        mihoro.update_ui(&client)
                    })
                    .await;
            } else if *geodata {
                report
                    .run("geodata", Some("refreshing geodata"), || {
                        mihoro.update_geodata(&client)
                    })
                    .await;
            } else if *config || (!*core && !*geodata && !*ui) {
                report
                    .run("config", Some("refreshing remote config"), || {
                        mihoro.update_config(&client, profile.as_deref())
                    })
                    .await;
                if !report.has_failures() && report.has_installed("config") {
                    report
                        .run("service restart", Some("restarting mihomo.service"), || {
                            mihoro.restart_service_with_config_rollback()
                        })
                        .await;
                } else {
                    report.record(
                        "service restart",
                        StageStatus::Skipped(
                            if report.has_failures() {
                                "skipped due to earlier failures"
                            } else {
                                "config already current"
                            }
                            .to_string(),
                        ),
                    );
                }
            }

            report.print("update summary");
            if report.has_failures() {
                anyhow::bail!("one or more update stages failed - see summary above");
            }
        }
        Some(Commands::Apply {
            profile,
            dry_run,
            diff,
        }) => {
            mihoro
                .apply(mihoro::ApplyOptions {
                    profile: profile.as_deref(),
                    dry_run: *dry_run,
                    diff: *diff,
                })
                .await?
        }
        Some(Commands::Profile { profile }) => {
            mihoro.profile_commands(&args.mihoro_config, profile)?;
        }
        Some(Commands::Deploy { deploy }) => {
            mihoro.deploy_commands(&args.mihoro_config, deploy).await?;
        }
        Some(Commands::Schedule { schedule }) => {
            mihoro.schedule_commands(&args.mihoro_config, schedule)?;
        }
        Some(Commands::Uninstall) => mihoro.uninstall()?,
        Some(Commands::Proxy { proxy }) => mihoro.proxy_commands(proxy)?,

        Some(Commands::Start) => Systemctl::with_scope(mihoro.systemd_scope())
            .start("mihomo.service")
            .execute()
            .map(|_| {
                println!("{} Started mihomo.service", mihoro.prefix.green());
            })?,

        Some(Commands::Status) => {
            Systemctl::with_scope(mihoro.systemd_scope())
                .status("mihomo.service")
                .execute()?;
        }

        Some(Commands::Stop) => Systemctl::with_scope(mihoro.systemd_scope())
            .stop("mihomo.service")
            .execute()
            .map(|_| {
                println!("{} Stopped mihomo.service", mihoro.prefix.green());
            })?,

        Some(Commands::Restart) => Systemctl::with_scope(mihoro.systemd_scope())
            .restart("mihomo.service")
            .execute()
            .map(|_| {
                println!("{} Restarted mihomo.service", mihoro.prefix.green());
            })?,

        Some(Commands::Log) => {
            Command::new("journalctl")
                .args(journalctl_args(
                    mihoro.systemd_scope(),
                    "mihomo.service",
                    10,
                    true,
                ))
                .spawn()
                .expect("failed to execute process")
                .wait()?;
        }

        Some(Commands::Completions { shell }) => match shell {
            Some(ClapShell::Bash) => {
                generate(Bash, &mut Args::command(), "mihoro", &mut io::stdout())
            }
            Some(ClapShell::Zsh) => {
                generate(Zsh, &mut Args::command(), "mihoro", &mut io::stdout())
            }
            Some(ClapShell::Fish) => {
                generate(Fish, &mut Args::command(), "mihoro", &mut io::stdout())
            }
            _ => (),
        },

        Some(Commands::Cron { cron }) => mihoro.cron_commands(cron)?,

        #[cfg(feature = "self_update")]
        Some(Commands::Upgrade { yes, check, target }) => {
            if *check {
                match upgrade::check_for_update().await? {
                    Some(version) => {
                        println!(
                            "{} New version available: {}",
                            mihoro.prefix.yellow(),
                            version.bold().green()
                        );
                        println!(
                            "{} Run {} to update",
                            "->".dimmed(),
                            "mihoro upgrade".bold().underline()
                        );
                    }
                    None => {
                        println!(
                            "{} You're running the latest version",
                            mihoro.prefix.green()
                        );
                    }
                }
            } else {
                upgrade::run_upgrade(*yes, target.clone()).await?;
            }
        }

        #[cfg(not(feature = "self_update"))]
        Some(Commands::Upgrade { .. }) => {
            anyhow::bail!(
                "mihoro was built without self_update support, please use your package manager to upgrade"
            );
        }

        None => (),
    }
    Ok(())
}

fn deployment_backend_from_arg(arg: DeploymentBackendArg) -> config::DeploymentBackend {
    match arg {
        DeploymentBackendArg::SystemdUser => config::DeploymentBackend::SystemdUser,
        DeploymentBackendArg::SystemdSystem => config::DeploymentBackend::SystemdSystem,
    }
}
