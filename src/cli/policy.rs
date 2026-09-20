use crate::cli::args::{Commands, TimerCommands};

pub(crate) fn command_requires_root(command: &Commands) -> bool {
    !matches!(
        command,
        Commands::Status
            | Commands::Log
            | Commands::Completions { .. }
            | Commands::Timer {
                timer: TimerCommands::Status
            }
            | Commands::Upgrade { check: true, .. }
    )
}

pub(crate) fn command_requires_config(command: &Commands) -> bool {
    matches!(
        command,
        Commands::Update { .. }
            | Commands::Apply
            | Commands::Timer {
                timer: TimerCommands::Enable
            }
    )
}

pub(crate) fn running_as_root() -> bool {
    // SAFETY: `geteuid` has no preconditions and does not dereference pointers.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{ClapShell, TimerCommands};
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
            timer: TimerCommands::Enable
        }));
        assert!(command_requires_root(&Commands::Uninstall {
            purge: false,
            yes: false
        }));
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

    #[test]
    fn config_loading_policy_is_explicit() {
        assert!(!command_requires_config(&Commands::Status));
        assert!(!command_requires_config(&Commands::Log));
        assert!(!command_requires_config(&Commands::Start));
        assert!(!command_requires_config(&Commands::Stop));
        assert!(!command_requires_config(&Commands::Restart));
        assert!(!command_requires_config(&Commands::Timer {
            timer: TimerCommands::Disable
        }));
        assert!(!command_requires_config(&Commands::Timer {
            timer: TimerCommands::Status
        }));
        assert!(!command_requires_config(&Commands::Uninstall {
            purge: false,
            yes: false
        }));
        assert!(!command_requires_config(&Commands::Upgrade {
            yes: false,
            check: true,
            target: None
        }));
        assert!(command_requires_config(&Commands::Update {
            config: false,
            ui: false,
            core: false,
            geodata: false,
            all: false,
            arch: None
        }));
        assert!(command_requires_config(&Commands::Apply));
        assert!(command_requires_config(&Commands::Timer {
            timer: TimerCommands::Enable
        }));
    }
}
