use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(author, about, version, arg_required_else_help(true))]
pub struct Args {
    /// Path to mihoro config file
    #[clap(short, long, default_value = "~/.config/mihoro.toml")]
    pub mihoro_config: String,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize mihoro: download binary, config, geodata, and set up the systemd service
    Init {
        /// Re-download all artifacts even if they already exist
        #[arg(long)]
        force: bool,

        /// Non-interactive mode: fail if required config fields are missing instead of prompting
        #[arg(short = 'y', long)]
        yes: bool,

        /// Override architecture detection
        ///
        /// Supported options on Linux: 386, 386-go120, 386-go123, 386-softfloat, amd64,
        /// amd64-compatible, amd64-v1/v2/v3 (with -go120/-go123 variants),
        /// arm64, armv5, armv6, armv7, loong64-abi1/abi2, mips-hardfloat,
        /// mips-softfloat, mips64, mips64le, mipsle-hardfloat, mipsle-softfloat,
        /// ppc64le, riscv64, s390x
        #[arg(long)]
        arch: Option<String>,

        /// Persisted deployment backend to write before initialization
        #[arg(long)]
        backend: Option<DeploymentBackendArg>,
    },
    /// Deprecated: use `mihoro init` instead
    #[command(hide = true)]
    Setup {
        /// Force download mihomo binary even if it already exists
        #[arg(long)]
        overwrite: bool,

        /// Override architecture detection
        #[arg(long)]
        arch: Option<String>,
    },
    /// Update mihomo components (config by default)
    Update {
        /// Profile to update; defaults to the active profile
        #[arg(long)]
        profile: Option<String>,

        /// Update remote config
        #[arg(long)]
        config: bool,

        /// Update external UI assets
        #[arg(long)]
        ui: bool,

        /// Update mihomo core binary
        #[arg(long)]
        core: bool,

        /// Update geodata
        #[arg(long)]
        geodata: bool,

        /// Update everything: config, geodata, and mihomo core binary
        #[arg(long, conflicts_with_all = ["config", "ui", "core", "geodata"])]
        all: bool,

        /// Override architecture detection (used with --core or --all)
        ///
        /// Supported options on Linux: 386, 386-go120, 386-go123, 386-softfloat, amd64,
        /// amd64-compatible, amd64-v1/v2/v3 (with -go120/-go123 variants),
        /// arm64, armv5, armv6, armv7, loong64-abi1/abi2, mips-hardfloat,
        /// mips-softfloat, mips64, mips64le, mipsle-hardfloat, mipsle-softfloat,
        /// ppc64le, riscv64, s390x
        #[arg(long)]
        arch: Option<String>,
    },
    /// Apply mihomo config overrides and restart mihomo.service
    Apply {
        /// Profile to apply; defaults to the active profile
        #[arg(long)]
        profile: Option<String>,

        /// Render and validate without activating config or restarting service
        #[arg(long)]
        dry_run: bool,

        /// Print a redacted semantic diff between active and candidate config
        #[arg(long)]
        diff: bool,
    },
    /// Manage named config profiles
    Profile {
        #[clap(subcommand)]
        profile: Option<ProfileCommands>,
    },
    /// Manage service deployment backend
    Deploy {
        #[clap(subcommand)]
        deploy: Option<DeployCommands>,
    },
    /// Manage scheduled updates
    Schedule {
        #[clap(subcommand)]
        schedule: Option<ScheduleCommands>,
    },
    /// Start mihomo.service with systemctl
    Start,
    /// Check mihomo.service status with systemctl
    Status,
    /// Stop mihomo.service with systemctl
    Stop,
    /// Restart mihomo.service with systemctl
    Restart,
    /// Check mihomo.service logs with journalctl
    #[clap(visible_alias("logs"))]
    Log,
    /// Output proxy export commands
    Proxy {
        #[clap(subcommand)]
        proxy: Option<ProxyCommands>,
    },
    /// Uninstall and remove mihoro and config
    Uninstall,
    /// Generate shell completions for mihoro
    Completions {
        #[clap(subcommand)]
        shell: Option<ClapShell>,
    },
    /// Manage auto-update cron job
    Cron {
        #[clap(subcommand)]
        cron: Option<CronCommands>,
    },
    #[cfg_attr(not(feature = "self_update"), command(hide = true))]
    /// Upgrade mihoro to the latest version
    Upgrade {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,

        /// Only check for updates, don't install
        #[arg(long)]
        check: bool,

        /// Override target triple (e.g., x86_64-unknown-linux-gnu)
        #[arg(long)]
        target: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum DeploymentBackendArg {
    SystemdUser,
    SystemdSystem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum SchedulerBackendArg {
    SystemdTimer,
    Cron,
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum DeployCommands {
    /// Print current deployment backend and derived paths
    Status,
    /// Apply a deployment backend
    Apply {
        /// Backend to apply
        #[arg(long)]
        backend: DeploymentBackendArg,

        /// Print the plan without mutating files or services
        #[arg(long)]
        dry_run: bool,

        /// Adopt and back up an existing unmanaged mihomo.service
        #[arg(long)]
        adopt_existing_unit: bool,
    },
    /// Import an existing Mihoro configuration tree
    Import {
        /// Source Mihoro config path
        #[arg(long = "from-mihoro")]
        from_mihoro: String,

        /// Print the plan without mutating files or services
        #[arg(long)]
        dry_run: bool,

        /// Remove imported source files after a successful import
        #[arg(long)]
        cleanup: bool,
    },
    /// Migrate the active deployment backend
    Migrate {
        /// Target backend
        #[arg(long)]
        to: DeploymentBackendArg,

        /// Print the plan without mutating files or services
        #[arg(long)]
        dry_run: bool,

        /// Adopt and back up an existing unmanaged mihomo.service
        #[arg(long)]
        adopt_existing_unit: bool,
    },
    /// Roll back the last deployment migration
    Rollback {
        /// Migration id to roll back; defaults to latest
        #[arg(long)]
        id: Option<String>,
    },
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum ScheduleCommands {
    /// Enable scheduled updates
    Enable {
        /// Scheduler backend
        #[arg(long)]
        backend: Option<SchedulerBackendArg>,

        /// systemd OnCalendar expression
        #[arg(long)]
        on_calendar: Option<String>,

        /// systemd RandomizedDelaySec value
        #[arg(long)]
        randomized_delay_sec: Option<String>,
    },
    /// Disable scheduled updates
    Disable,
    /// Show scheduled update status
    Status,
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum ProxyCommands {
    /// Output and copy proxy export shell commands
    Export,
    /// Output and copy proxy export shell commands for LAN access
    ExportLan,
    /// Output and copy proxy unset shell commands
    Unset,
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum ProfileCommands {
    /// Add or replace a named profile
    Add {
        /// Profile name
        name: String,

        /// Subscription URL source
        #[arg(long, conflicts_with_all = ["file", "existing"], required_unless_present_any = ["file", "existing"])]
        url: Option<String>,

        /// Local file source copied into the profile
        #[arg(long, conflicts_with_all = ["url", "existing"])]
        file: Option<String>,

        /// Existing config source imported into the profile
        #[arg(long, conflicts_with_all = ["url", "file"])]
        existing: Option<String>,

        /// Per-profile User-Agent for URL sources
        #[arg(long)]
        user_agent: Option<String>,

        /// Per-profile HTTP header in KEY=VALUE form; may be repeated
        #[arg(long = "header")]
        header: Vec<String>,

        /// Replace an existing profile
        #[arg(long)]
        force: bool,
    },
    /// List profiles
    List,
    /// Show one profile
    Show {
        /// Profile name
        name: String,
    },
    /// Make a profile active
    Use {
        /// Profile name
        name: String,
    },
    /// Remove a profile
    Remove {
        /// Profile name
        name: String,
    },
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum ClapShell {
    /// Generate bash completions
    Bash,
    /// Generate fish completions
    Fish,
    /// Generate zsh completions
    Zsh,
    // #[command(about = "Generate powershell completions")]
    // Powershell,
    // #[command(about = "Generate elvish completions")]
    // Elvish,
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum CronCommands {
    /// Enable auto-update cron job
    Enable,
    /// Disable auto-update cron job
    Disable,
    /// Show auto-update cron job status
    Status,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_update_ui_flag() {
        let args = Args::parse_from(["mihoro", "update", "--ui"]);
        match args.command {
            Some(Commands::Update {
                ui,
                config,
                core,
                geodata,
                all,
                ..
            }) => {
                assert!(ui);
                assert!(!config);
                assert!(!core);
                assert!(!geodata);
                assert!(!all);
            }
            _ => panic!("expected update command"),
        }
    }

    #[test]
    fn test_parse_init_backend_flag() {
        let args = Args::parse_from(["mihoro", "init", "--backend", "systemd-system"]);
        match args.command {
            Some(Commands::Init { backend, .. }) => {
                assert_eq!(backend, Some(DeploymentBackendArg::SystemdSystem));
            }
            _ => panic!("expected init command"),
        }
    }

    #[test]
    fn test_parse_deploy_apply_and_migrate_commands() {
        let args = Args::parse_from([
            "mihoro",
            "deploy",
            "apply",
            "--backend",
            "systemd-system",
            "--dry-run",
            "--adopt-existing-unit",
        ]);
        match args.command {
            Some(Commands::Deploy {
                deploy:
                    Some(DeployCommands::Apply {
                        backend,
                        dry_run,
                        adopt_existing_unit,
                    }),
            }) => {
                assert_eq!(backend, DeploymentBackendArg::SystemdSystem);
                assert!(dry_run);
                assert!(adopt_existing_unit);
            }
            _ => panic!("expected deploy apply command"),
        }

        let args = Args::parse_from(["mihoro", "deploy", "migrate", "--to", "systemd-system"]);
        match args.command {
            Some(Commands::Deploy {
                deploy: Some(DeployCommands::Migrate { to, .. }),
            }) => assert_eq!(to, DeploymentBackendArg::SystemdSystem),
            _ => panic!("expected deploy migrate command"),
        }
    }

    #[test]
    fn test_parse_schedule_enable_command() {
        let args = Args::parse_from([
            "mihoro",
            "schedule",
            "enable",
            "--backend",
            "systemd-timer",
            "--on-calendar",
            "*-*-* 03:00:00",
            "--randomized-delay-sec",
            "5min",
        ]);

        match args.command {
            Some(Commands::Schedule {
                schedule:
                    Some(ScheduleCommands::Enable {
                        backend,
                        on_calendar,
                        randomized_delay_sec,
                    }),
            }) => {
                assert_eq!(backend, Some(SchedulerBackendArg::SystemdTimer));
                assert_eq!(on_calendar.as_deref(), Some("*-*-* 03:00:00"));
                assert_eq!(randomized_delay_sec.as_deref(), Some("5min"));
            }
            _ => panic!("expected schedule enable command"),
        }
    }

    #[test]
    fn test_parse_update_all_flag() {
        let args = Args::parse_from(["mihoro", "update", "--all"]);
        match args.command {
            Some(Commands::Update { all, ui, .. }) => {
                assert!(all);
                assert!(!ui);
            }
            _ => panic!("expected update command"),
        }
    }

    #[test]
    fn test_parse_profile_add_url_with_headers() {
        let args = Args::parse_from([
            "mihoro",
            "profile",
            "add",
            "work",
            "--url",
            "https://example.com/sub",
            "--user-agent",
            "mihoro-test",
            "--header",
            "Authorization=Bearer token",
        ]);

        match args.command {
            Some(Commands::Profile {
                profile:
                    Some(ProfileCommands::Add {
                        name,
                        url,
                        file,
                        existing,
                        user_agent,
                        header,
                        force,
                    }),
            }) => {
                assert_eq!(name, "work");
                assert_eq!(url.as_deref(), Some("https://example.com/sub"));
                assert!(file.is_none());
                assert!(existing.is_none());
                assert_eq!(user_agent.as_deref(), Some("mihoro-test"));
                assert_eq!(header, vec!["Authorization=Bearer token".to_string()]);
                assert!(!force);
            }
            _ => panic!("expected profile add command"),
        }
    }

    #[test]
    fn test_parse_update_profile_and_apply_dry_run_diff() {
        let update = Args::parse_from(["mihoro", "update", "--profile", "work"]);
        match update.command {
            Some(Commands::Update { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("work"));
            }
            _ => panic!("expected update command"),
        }

        let apply = Args::parse_from([
            "mihoro",
            "apply",
            "--profile",
            "work",
            "--dry-run",
            "--diff",
        ]);
        match apply.command {
            Some(Commands::Apply {
                profile,
                dry_run,
                diff,
            }) => {
                assert_eq!(profile.as_deref(), Some("work"));
                assert!(dry_run);
                assert!(diff);
            }
            _ => panic!("expected apply command"),
        }
    }
}
