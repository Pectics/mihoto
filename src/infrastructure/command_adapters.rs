use crate::application::ports::{
    ApplyOperations, ServiceOperations, TimerOperations, UninstallOperations,
};
use crate::domain::config::Config;
use crate::infrastructure::{mihomo::Mihoto, systemctl::Systemctl, timer};

use std::{future::Future, path::Path};

use anyhow::Result;

pub(crate) struct MihotoCommands<'a> {
    pub(crate) mihoto: &'a Mihoto,
    pub(crate) manager_config_path: &'a str,
}

impl ApplyOperations for MihotoCommands<'_> {
    fn apply(&self) -> impl Future<Output = Result<()>> {
        self.mihoto.apply()
    }

    fn reconcile_timer(&self) -> Result<()> {
        timer::reconcile(&self.mihoto.config, self.manager_config_path)
    }
}

impl UninstallOperations for MihotoCommands<'_> {
    fn uninstall(&self, purge: bool, manager_config_path: &Path) -> Result<()> {
        self.mihoto.uninstall(purge, manager_config_path)
    }
}

pub(crate) struct SystemService;

impl ServiceOperations for SystemService {
    fn start(&self) -> Result<()> {
        Systemctl::new().start("mihomo.service").execute()?;
        Ok(())
    }

    fn status(&self) -> Result<()> {
        Systemctl::new().status("mihomo.service").execute()?;
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        Systemctl::new().stop("mihomo.service").execute()?;
        Ok(())
    }

    fn restart(&self) -> Result<()> {
        Systemctl::new().restart("mihomo.service").execute()?;
        Ok(())
    }
}

pub(crate) struct SystemTimer<'a> {
    pub(crate) config: Option<&'a Config>,
    pub(crate) manager_config_path: &'a str,
}

impl TimerOperations for SystemTimer<'_> {
    fn enable(&self) -> Result<()> {
        timer::enable(
            self.config.expect("timer enable requires config"),
            self.manager_config_path,
        )
    }

    fn disable(&self) -> Result<()> {
        timer::disable()
    }

    fn status(&self) -> Result<()> {
        timer::status()
    }
}
