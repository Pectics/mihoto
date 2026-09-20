use crate::infrastructure::download::{download_file, DETAIL_PREFIX};
use crate::infrastructure::ui::install_ui;

use std::path::Path;

use anyhow::Result;
use colored::Colorize;
use reqwest::Client;

use super::{Mihoto, StageStatus};

impl Mihoto {
    /// Download geodata.  Skips files that already exist (unless `force`).
    pub async fn ensure_geodata(&self, client: &Client, force: bool) -> Result<StageStatus> {
        let Some(ref geox_url) = self.config.mihomo_config.geox_url else {
            return Ok(StageStatus::Skipped("geox_url not configured".to_string()));
        };

        let geodata_mode = self.config.mihomo_config.geodata_mode.unwrap_or(false);
        let config_root = Path::new(&self.mihomo_target_config_root);

        if geodata_mode {
            let geoip_path = config_root.join("geoip.dat");
            let geosite_path = config_root.join("geosite.dat");
            if !force && geoip_path.exists() && geosite_path.exists() {
                return Ok(StageStatus::Skipped("geodata present".to_string()));
            }
            if force || !geoip_path.exists() {
                download_file(
                    client,
                    &geox_url.geoip,
                    &geoip_path,
                    &self.config.mihoto_user_agent,
                )
                .await?;
            }
            if force || !geosite_path.exists() {
                download_file(
                    client,
                    &geox_url.geosite,
                    &geosite_path,
                    &self.config.mihoto_user_agent,
                )
                .await?;
            }
        } else {
            let mmdb_path = config_root.join("country.mmdb");
            if !force && mmdb_path.exists() {
                return Ok(StageStatus::Skipped("geodata present".to_string()));
            }
            download_file(
                client,
                &geox_url.mmdb,
                &mmdb_path,
                &self.config.mihoto_user_agent,
            )
            .await?;
        }

        Ok(StageStatus::Installed)
    }

    /// Install the web dashboard.  Skips if the target directory already has an `index.html`
    /// (unless `force`).
    pub async fn ensure_ui(&self, client: &Client, force: bool) -> Result<StageStatus> {
        let Some(ui) = self.config.ui.as_ref() else {
            return Ok(StageStatus::Skipped("UI management disabled".to_string()));
        };
        let Some(target_dir) = self.external_ui_target_dir() else {
            return Ok(StageStatus::Skipped("`external_ui` path unset".to_string()));
        };
        if !force && target_dir.join("index.html").exists() {
            return Ok(StageStatus::Skipped(format!(
                "{} already installed",
                ui.as_config_value()
            )));
        }
        install_ui(
            client,
            ui,
            &target_dir,
            &self.config.mihoto_user_agent,
            DETAIL_PREFIX.cyan(),
        )
        .await?;
        Ok(StageStatus::Installed)
    }

    pub async fn update_geodata(&self, client: &Client) -> Result<StageStatus> {
        if let Some(geox_url) = self.config.mihomo_config.geox_url.clone() {
            // Download geodata files based on `geodata_mode`
            let geodata_mode = self.config.mihomo_config.geodata_mode.unwrap_or(false);
            if geodata_mode {
                download_file(
                    client,
                    &geox_url.geoip,
                    &Path::new(&self.mihomo_target_config_root).join("geoip.dat"),
                    &self.config.mihoto_user_agent,
                )
                .await?;
                download_file(
                    client,
                    &geox_url.geosite,
                    &Path::new(&self.mihomo_target_config_root).join("geosite.dat"),
                    &self.config.mihoto_user_agent,
                )
                .await?;
            } else {
                download_file(
                    client,
                    &geox_url.mmdb,
                    &Path::new(&self.mihomo_target_config_root).join("country.mmdb"),
                    &self.config.mihoto_user_agent,
                )
                .await?;
            }

            println!("{} Downloaded and updated geodata", DETAIL_PREFIX.cyan());
        } else {
            return Ok(StageStatus::Skipped("`geox_url` undefined".to_string()));
        }
        Ok(StageStatus::Installed)
    }

    pub async fn update_ui(&self, client: &Client) -> Result<StageStatus> {
        let Some(ui) = self.config.ui.as_ref() else {
            return Ok(StageStatus::Skipped("UI management disabled".to_string()));
        };

        let Some(target_dir) = self.external_ui_target_dir() else {
            return Ok(StageStatus::Skipped("`external_ui` undefined".to_string()));
        };

        install_ui(
            client,
            ui,
            &target_dir,
            &self.config.mihoto_user_agent,
            DETAIL_PREFIX.cyan(),
        )
        .await?;
        Ok(StageStatus::Installed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::{Config, GeoxUrl};
    use std::fs;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn mihoto_with_root(root: &Path) -> Mihoto {
        let config = Config {
            mihomo_config_root: root.to_string_lossy().into_owned(),
            ..Config::default()
        };
        Mihoto::from_config(config)
    }

    #[tokio::test]
    async fn missing_configuration_and_existing_ui_use_documented_skip_reasons() {
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new();
        let mut mihoto = mihoto_with_root(dir.path());
        mihoto.config.mihomo_config.geox_url = None;
        assert!(matches!(
            mihoto.ensure_geodata(&client, false).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "geox_url not configured"
        ));
        assert!(matches!(
            mihoto.update_geodata(&client).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "`geox_url` undefined"
        ));

        mihoto.config.ui = None;
        assert!(matches!(
            mihoto.ensure_ui(&client, false).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "UI management disabled"
        ));
        assert!(matches!(
            mihoto.update_ui(&client).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "UI management disabled"
        ));

        mihoto.config.ui = Some(crate::domain::ui::Ui::Metacubexd);
        mihoto.config.mihomo_config.external_ui = None;
        assert!(matches!(
            mihoto.ensure_ui(&client, false).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "`external_ui` path unset"
        ));
        assert!(matches!(
            mihoto.update_ui(&client).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "`external_ui` undefined"
        ));

        mihoto.config.mihomo_config.external_ui = Some("dashboard".to_string());
        fs::create_dir_all(dir.path().join("dashboard")).unwrap();
        fs::write(dir.path().join("dashboard/index.html"), "installed").unwrap();
        assert!(matches!(
            mihoto.ensure_ui(&client, false).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "metacubexd already installed"
        ));
    }

    #[tokio::test]
    async fn dat_mode_downloads_only_missing_files_unless_forced() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/geoip.dat"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"geoip"))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/geosite.dat"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"geosite"))
            .expect(3)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("geoip.dat"), "existing").unwrap();
        let mut mihoto = mihoto_with_root(dir.path());
        mihoto.config.mihomo_config.geodata_mode = Some(true);
        mihoto.config.mihomo_config.geox_url = Some(GeoxUrl {
            geoip: format!("{}/geoip.dat", server.uri()),
            geosite: format!("{}/geosite.dat", server.uri()),
            mmdb: format!("{}/country.mmdb", server.uri()),
        });
        let client = Client::new();

        assert!(matches!(
            mihoto.ensure_geodata(&client, false).await.unwrap(),
            StageStatus::Installed
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join("geoip.dat")).unwrap(),
            "existing"
        );
        assert!(matches!(
            mihoto.ensure_geodata(&client, false).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "geodata present"
        ));
        assert!(matches!(
            mihoto.ensure_geodata(&client, true).await.unwrap(),
            StageStatus::Installed
        ));
        assert!(matches!(
            mihoto.update_geodata(&client).await.unwrap(),
            StageStatus::Installed
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join("geoip.dat")).unwrap(),
            "geoip"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("geosite.dat")).unwrap(),
            "geosite"
        );
    }

    #[tokio::test]
    async fn mmdb_mode_skips_existing_file_and_update_forces_download() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/country.mmdb"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new mmdb"))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("country.mmdb"), "existing").unwrap();
        let mut mihoto = mihoto_with_root(dir.path());
        mihoto.config.mihomo_config.geodata_mode = Some(false);
        mihoto.config.mihomo_config.geox_url = Some(GeoxUrl {
            geoip: "unused".to_string(),
            geosite: "unused".to_string(),
            mmdb: format!("{}/country.mmdb", server.uri()),
        });
        let client = Client::new();

        assert!(matches!(
            mihoto.ensure_geodata(&client, false).await.unwrap(),
            StageStatus::Skipped(reason) if reason == "geodata present"
        ));
        assert!(matches!(
            mihoto.update_geodata(&client).await.unwrap(),
            StageStatus::Installed
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join("country.mmdb")).unwrap(),
            "new mmdb"
        );
    }
}
