use crate::application::ports::UpdateOperations;
use crate::application::stage::StageStatus;
use crate::infrastructure::mihomo::Mihoto;

use std::future::Future;

use anyhow::Result;
use reqwest::Client;

impl UpdateOperations for (&Mihoto, &Client) {
    fn update_config(&self) -> impl Future<Output = Result<StageStatus>> {
        self.0.update_config_and_restart(self.1)
    }

    fn update_geodata(&self) -> impl Future<Output = Result<StageStatus>> {
        self.0.update_geodata(self.1)
    }

    fn update_ui(&self) -> impl Future<Output = Result<StageStatus>> {
        self.0.update_ui(self.1)
    }

    fn update_core(&self, arch: Option<&str>) -> impl Future<Output = Result<StageStatus>> {
        self.0.update_core(self.1, arch)
    }

    fn restart_service(&self) -> impl Future<Output = Result<StageStatus>> {
        self.0.restart_service()
    }
}
