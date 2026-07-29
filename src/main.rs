mod cmd;
mod config;
mod init;
mod mihoto;
mod resolve_mihomo_bin;
mod systemctl;
mod timer;
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

use cmd::{Args, ClapShell, Commands};
use mihoto::{Mihoto, StageStatus};
use systemctl::Systemctl;

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
        println!("{} {}", "mihoto:".cyan().bold(), label.bold());
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

fn command_requires_root(command: &Commands) -> bool {
    !matches!(
        command,
        Commands::Status
            | Commands::Log
            | Commands::Completions { .. }
            | Commands::Timer {
                timer: Some(cmd::TimerCommands::Status)
            }
            | Commands::Upgrade { check: true, .. }
    )
}

fn running_as_root() -> bool {
    // SAFETY: `geteuid` has no preconditions and does not dereference pointers.
    unsafe { libc::geteuid() == 0 }
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

    // Read-only commands deliberately do not load or create the manager configuration.
    match &args.command {
        Some(Commands::Status) => {
            Systemctl::new().status("mihomo.service").execute()?;
            return Ok(());
        }
        Some(Commands::Log) => {
            let status = Command::new("journalctl")
                .arg("-xeu")
                .arg("mihomo.service")
                .arg("-n")
                .arg("10")
                .arg("-f")
                .status()?;
            if !status.success() {
                anyhow::bail!("journalctl exited with {status}");
            }
            return Ok(());
        }
        Some(Commands::Timer {
            timer: Some(cmd::TimerCommands::Status),
        }) => {
            timer::status()?;
            return Ok(());
        }
        Some(Commands::Completions { shell }) => {
            match shell {
                Some(ClapShell::Bash) => {
                    generate(Bash, &mut Args::command(), "mihoto", &mut io::stdout())
                }
                Some(ClapShell::Zsh) => {
                    generate(Zsh, &mut Args::command(), "mihoto", &mut io::stdout())
                }
                Some(ClapShell::Fish) => {
                    generate(Fish, &mut Args::command(), "mihoto", &mut io::stdout())
                }
                None => {}
            }
            return Ok(());
        }
        _ => {}
    }

    if args.command.as_ref().is_some_and(command_requires_root) && !running_as_root() {
        anyhow::bail!("this command requires root; run it with sudo");
    }

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()?;

    // Handle Init and Setup before constructing Mihoto, which requires a valid config.
    if let Some(Commands::Init { force, arch, yes }) = &args.command {
        return init::run(
            &args.mihoto_config,
            &client,
            init::InitOptions {
                force: *force,
                arch: arch.clone(),
                yes: *yes,
            },
        )
        .await;
    }

    let mihoto = Mihoto::new(&args.mihoto_config)?;

    match &args.command {
        Some(Commands::Init { .. }) => unreachable!(),
        Some(Commands::Update {
            config,
            core,
            geodata,
            all,
            arch,
            ui,
        }) => {
            println!("{} update initiated", "mihoto:".cyan().bold());
            let mut report = StageReport::new();

            if *all {
                report
                    .run("config", Some("refreshing remote config"), || {
                        mihoto.update_config(&client)
                    })
                    .await;
                report
                    .run("geodata", Some("refreshing geodata"), || {
                        mihoto.update_geodata(&client)
                    })
                    .await;
                report
                    .run("ui", Some("refreshing dashboard assets"), || {
                        mihoto.update_ui(&client)
                    })
                    .await;
                report
                    .run("core", Some("refreshing mihomo core"), || {
                        mihoto.update_core(&client, arch.as_deref())
                    })
                    .await;
                if !report.has_failures() {
                    report
                        .run("service restart", Some("restarting mihomo.service"), || {
                            mihoto.restart_service()
                        })
                        .await;
                } else {
                    report.record(
                        "service restart",
                        StageStatus::Skipped("skipped due to earlier failures".to_string()),
                    );
                }
            } else if *core {
                report
                    .run("core", Some("refreshing mihomo core"), || {
                        mihoto.update_core(&client, arch.as_deref())
                    })
                    .await;
                if !report.has_failures() && report.has_installed("core") {
                    report
                        .run("service restart", Some("restarting mihomo.service"), || {
                            mihoto.restart_service()
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
                        mihoto.update_ui(&client)
                    })
                    .await;
            } else if *geodata {
                report
                    .run("geodata", Some("refreshing geodata"), || {
                        mihoto.update_geodata(&client)
                    })
                    .await;
            } else if *config || (!*core && !*geodata && !*ui) {
                report
                    .run("config", Some("refreshing remote config"), || {
                        mihoto.update_config(&client)
                    })
                    .await;
                if !report.has_failures() {
                    report
                        .run("service restart", Some("restarting mihomo.service"), || {
                            mihoto.restart_service()
                        })
                        .await;
                } else {
                    report.record(
                        "service restart",
                        StageStatus::Skipped("skipped due to earlier failures".to_string()),
                    );
                }
            }

            report.print("update summary");
            if report.has_failures() {
                anyhow::bail!("one or more update stages failed - see summary above");
            }
        }
        Some(Commands::Apply) => mihoto.apply().await?,
        Some(Commands::Uninstall { purge }) => mihoto.uninstall(*purge)?,

        Some(Commands::Start) => Systemctl::new()
            .start("mihomo.service")
            .execute()
            .map(|_| {
                println!("{} Started mihomo.service", mihoto.prefix.green());
            })?,

        Some(Commands::Status) => {
            Systemctl::new().status("mihomo.service").execute()?;
        }

        Some(Commands::Stop) => Systemctl::new().stop("mihomo.service").execute().map(|_| {
            println!("{} Stopped mihomo.service", mihoto.prefix.green());
        })?,

        Some(Commands::Restart) => {
            Systemctl::new()
                .restart("mihomo.service")
                .execute()
                .map(|_| {
                    println!("{} Restarted mihomo.service", mihoto.prefix.green());
                })?
        }

        Some(Commands::Log) => {
            Command::new("journalctl")
                .arg("-xeu")
                .arg("mihomo.service")
                .arg("-n")
                .arg("10")
                .arg("-f")
                .spawn()
                .expect("failed to execute process")
                .wait()?;
        }

        Some(Commands::Completions { shell }) => match shell {
            Some(ClapShell::Bash) => {
                generate(Bash, &mut Args::command(), "mihoto", &mut io::stdout())
            }
            Some(ClapShell::Zsh) => {
                generate(Zsh, &mut Args::command(), "mihoto", &mut io::stdout())
            }
            Some(ClapShell::Fish) => {
                generate(Fish, &mut Args::command(), "mihoto", &mut io::stdout())
            }
            _ => (),
        },

        Some(Commands::Timer { timer }) => match timer {
            Some(cmd::TimerCommands::Enable) => timer::enable(&mihoto.config, &args.mihoto_config)?,
            Some(cmd::TimerCommands::Disable) => timer::disable()?,
            Some(cmd::TimerCommands::Status) => timer::status()?,
            None => {}
        },

        #[cfg(feature = "self_update")]
        Some(Commands::Upgrade { yes, check, target }) => {
            if *check {
                match upgrade::check_for_update().await? {
                    Some(version) => {
                        println!(
                            "{} New version available: {}",
                            mihoto.prefix.yellow(),
                            version.bold().green()
                        );
                        println!(
                            "{} Run {} to update",
                            "->".dimmed(),
                            "mihoto upgrade".bold().underline()
                        );
                    }
                    None => {
                        println!(
                            "{} You're running the latest version",
                            mihoto.prefix.green()
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
                "mihoto was built without self_update support, please use your package manager to upgrade"
            );
        }

        None => (),
    }
    Ok(())
}

#[cfg(test)]
mod command_policy_tests {
    use super::*;
    #[test]
    fn root_policy_is_explicit() {
        assert!(command_requires_root(&Commands::Init {
            force: false,
            yes: true,
            arch: None
        }));
        assert!(command_requires_root(&Commands::Update {
            config: false,
            ui: false,
            core: false,
            geodata: false,
            all: false,
            arch: None
        }));
        assert!(command_requires_root(&Commands::Apply));
        assert!(command_requires_root(&Commands::Start));
        assert!(command_requires_root(&Commands::Stop));
        assert!(command_requires_root(&Commands::Restart));
        assert!(command_requires_root(&Commands::Timer {
            timer: Some(cmd::TimerCommands::Enable)
        }));
        assert!(command_requires_root(&Commands::Uninstall { purge: false }));
        assert!(command_requires_root(&Commands::Upgrade {
            yes: true,
            check: false,
            target: None
        }));
        assert!(!command_requires_root(&Commands::Status));
        assert!(!command_requires_root(&Commands::Log));
        assert!(!command_requires_root(&Commands::Completions {
            shell: Some(ClapShell::Bash)
        }));
        assert!(!command_requires_root(&Commands::Upgrade {
            yes: false,
            check: true,
            target: None
        }));
    }
}
