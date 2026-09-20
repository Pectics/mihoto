use crate::application::ports::{InitEvents, StageEvents};
use crate::application::stage::{StageEntry, StageStatus};
use crate::domain::config::Config;

use colored::Colorize;

#[derive(Default)]
pub(crate) struct TerminalStageEvents;

impl StageEvents for TerminalStageEvents {
    fn begin(&mut self, name: &'static str, description: Option<&str>) {
        println!("{} {}", "●".cyan().bold(), name.bold());
        if let Some(description) = description {
            println!("{}  {}", " ⎿".cyan().bold(), description.italic().dimmed());
        }
    }

    fn summary(&mut self, label: &str, entries: &[StageEntry]) {
        println!("{} {}", "mihoto:".cyan().bold(), label.bold());
        for entry in entries {
            match &entry.status {
                StageStatus::Installed => {
                    println!("  {} {}", "✓".green().bold(), entry.name);
                }
                StageStatus::Skipped(reason) => println!(
                    "  {} {} ({})",
                    "↷".dimmed(),
                    entry.name.dimmed(),
                    reason.dimmed()
                ),
                StageStatus::Failed(error) => {
                    println!("  {} {}: {:#}", "✗".red().bold(), entry.name.red(), error)
                }
            }
        }
    }
}

impl InitEvents for TerminalStageEvents {
    fn initializing(&mut self) {
        println!("{} initializing", "mihoto:".cyan().bold());
    }

    fn dashboard(&mut self, config: &Config) {
        let Some(controller) = config.mihomo_config.external_controller.as_deref() else {
            return;
        };
        let Some((host, port)) = controller.rsplit_once(':') else {
            return;
        };
        let host = match host {
            "0.0.0.0" | "[::]" | "" => "127.0.0.1",
            host => host,
        };
        let Some(ui) = config.ui.as_ref() else {
            return;
        };

        println!();
        println!(
            "  {}: {}",
            "Dashboard".bold(),
            format!("http://{host}:{port}/ui/").underline().cyan()
        );
        println!(
            "  Using {} - change via the {} field in {}",
            ui.as_config_value().bold(),
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
