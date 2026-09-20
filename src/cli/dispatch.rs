#[cfg(feature = "self_update")]
use crate::application::upgrade as upgrade_app;
use crate::application::{apply, init, service, timer as timer_app, uninstall, update};
use crate::cli::args::{self as cmd, Args, ClapShell, Commands};
use crate::cli::init as init_ui;
use crate::cli::policy::{command_requires_config, command_requires_root, running_as_root};
use crate::cli::report::TerminalStageEvents;
use crate::domain::config;
use crate::infrastructure::command_adapters::{MihotoCommands, SystemService, SystemTimer};
use crate::infrastructure::config_store;
use crate::infrastructure::mihomo::Mihoto;
#[cfg(feature = "self_update")]
use crate::infrastructure::upgrade_adapter::SelfUpgrade;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{
    generate,
    shells::{Bash, Fish, Zsh},
};
use colored::Colorize;
use dialoguer::Confirm;
use reqwest::Client;
use std::{io, path::Path, process::Command, time::Duration};

pub(crate) async fn run() -> Result<()> {
    let args = Args::parse();
    if args.command.as_ref().is_some_and(|command| {
        matches!(command, Commands::Init { .. } | Commands::Uninstall { .. })
            || command_requires_config(command)
    }) {
        config::validate_manager_config_path(Path::new(&args.mihoto_config))?;
    }

    // Read-only commands deliberately do not load or create the manager configuration.
    match &args.command {
        Some(Commands::Status) => {
            service::run(&SystemService, service::ServiceAction::Status)?;
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
            timer: cmd::TimerCommands::Status,
        }) => {
            timer_app::run(
                &SystemTimer {
                    config: None,
                    manager_config_path: &args.mihoto_config,
                },
                timer_app::TimerAction::Status,
            )?;
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
        let config = init_ui::bootstrap_config(&args.mihoto_config, *yes)?;
        config::validate_config(&config)?;
        let mihoto = Mihoto::from_config(config.clone());
        return init::run(
            &args.mihoto_config,
            &config,
            &(&mihoto, &client),
            init::InitOptions {
                force: *force,
                arch: arch.clone(),
            },
            TerminalStageEvents,
        )
        .await;
    }

    let mihoto = if args.command.as_ref().is_some_and(command_requires_config) {
        Some(Mihoto::new(&args.mihoto_config)?)
    } else {
        None
    };

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
            let mihoto = mihoto.as_ref().expect("update requires config");
            println!("{} update initiated", "mihoto:".cyan().bold());
            update::run(
                &(mihoto, &client),
                update::UpdateOptions {
                    config: *config,
                    core: *core,
                    geodata: *geodata,
                    ui: *ui,
                    all: *all,
                    arch: arch.as_deref(),
                },
                TerminalStageEvents,
            )
            .await?;
        }
        Some(Commands::Apply) => {
            let mihoto = mihoto.as_ref().expect("apply requires config");
            apply::run(&MihotoCommands {
                mihoto,
                manager_config_path: &args.mihoto_config,
            })
            .await?;
        }
        Some(Commands::Uninstall { purge, yes }) => {
            if *purge
                && !*yes
                && !Confirm::new()
                    .with_prompt(
                        "Permanently remove mihoto, Mihomo, and all managed system configuration?",
                    )
                    .default(false)
                    .interact()?
            {
                anyhow::bail!("purge cancelled");
            }
            let config = config_store::load_config(&args.mihoto_config)?
                .unwrap_or_else(config::Config::default);
            let mihoto = Mihoto::from_config(config);
            uninstall::run(
                &MihotoCommands {
                    mihoto: &mihoto,
                    manager_config_path: &args.mihoto_config,
                },
                *purge,
                Path::new(&args.mihoto_config),
            )?;
        }

        Some(Commands::Start) => {
            service::run(&SystemService, service::ServiceAction::Start)?;
            println!("{} Started mihomo.service", "mihoto:".green());
        }

        Some(Commands::Status) => {
            service::run(&SystemService, service::ServiceAction::Status)?;
        }

        Some(Commands::Stop) => {
            service::run(&SystemService, service::ServiceAction::Stop)?;
            println!("{} Stopped mihomo.service", "mihoto:".green());
        }

        Some(Commands::Restart) => {
            service::run(&SystemService, service::ServiceAction::Restart)?;
            println!("{} Restarted mihomo.service", "mihoto:".green());
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

        Some(Commands::Timer { timer }) => timer_app::run(
            &SystemTimer {
                config: mihoto.as_ref().map(|mihoto| &mihoto.config),
                manager_config_path: &args.mihoto_config,
            },
            match timer {
                cmd::TimerCommands::Enable => timer_app::TimerAction::Enable,
                cmd::TimerCommands::Disable => timer_app::TimerAction::Disable,
                cmd::TimerCommands::Status => timer_app::TimerAction::Status,
            },
        )?,

        #[cfg(feature = "self_update")]
        Some(Commands::Upgrade { yes, check, target }) => {
            match upgrade_app::run(&SelfUpgrade, *yes, *check, target.clone()).await? {
                upgrade_app::UpgradeOutcome::Checked(version) => match version {
                    Some(version) => {
                        println!(
                            "{} New version available: {}",
                            "mihoto:".yellow(),
                            version.bold().green()
                        );
                        println!(
                            "{} Run {} to update",
                            "->".dimmed(),
                            "mihoto upgrade".bold().underline()
                        );
                    }
                    None => {
                        println!("{} You're running the latest version", "mihoto:".green());
                    }
                },
                upgrade_app::UpgradeOutcome::Installed => {}
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
