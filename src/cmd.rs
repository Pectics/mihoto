use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mihoto", author, about, version, arg_required_else_help(true))]
pub struct Args {
    /// Path to mihoto config file
    #[arg(short = 'c', long = "config", default_value = "/etc/mihoto.toml")]
    pub mihoto_config: String,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize mihoto and the system Mihomo service
    Init {
        #[arg(long)]
        force: bool,
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(long)]
        arch: Option<String>,
    },
    /// Update Mihomo components
    Update {
        #[arg(long)]
        config: bool,
        #[arg(long)]
        ui: bool,
        #[arg(long)]
        core: bool,
        #[arg(long)]
        geodata: bool,
        #[arg(long, conflicts_with_all = ["config", "ui", "core", "geodata"])]
        all: bool,
        #[arg(long)]
        arch: Option<String>,
    },
    /// Apply configuration overrides
    Apply,
    /// Start mihomo.service
    Start,
    /// Show mihomo.service status
    Status,
    /// Stop mihomo.service
    Stop,
    /// Restart mihomo.service
    Restart,
    /// Follow mihomo.service logs
    #[command(visible_alias = "logs")]
    Log,
    /// Manage the systemd update timer
    Timer {
        #[command(subcommand)]
        timer: Option<TimerCommands>,
    },
    /// Uninstall system units and binaries
    Uninstall {
        #[arg(long)]
        purge: bool,
    },
    /// Generate shell completions
    Completions {
        #[command(subcommand)]
        shell: Option<ClapShell>,
    },
    /// Upgrade mihoto
    Upgrade {
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(long)]
        check: bool,
        #[arg(long)]
        target: Option<String>,
    },
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum TimerCommands {
    Enable,
    Disable,
    Status,
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum ClapShell {
    Bash,
    Fish,
    Zsh,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_cli_contract() {
        let args = Args::parse_from(["mihoto", "--config", "/tmp/a", "timer", "status"]);
        assert_eq!(args.mihoto_config, "/tmp/a");
        assert!(matches!(
            args.command,
            Some(Commands::Timer {
                timer: Some(TimerCommands::Status)
            })
        ));
        for removed in ["setup", "proxy", "cron"] {
            assert!(Args::try_parse_from(["mihoto", removed]).is_err());
        }
    }
}
