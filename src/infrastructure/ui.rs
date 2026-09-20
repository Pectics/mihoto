use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use reqwest::Client;
use tar::Archive;
use tempfile::{tempdir_in, NamedTempFile};

use crate::domain::ui::Ui;
use crate::infrastructure::download::download_file;
use crate::infrastructure::filesystem::create_parent_dir;

pub async fn install_ui(
    client: &Client,
    ui: &Ui,
    target_dir: &Path,
    user_agent: &str,
    prefix: impl std::fmt::Display,
) -> Result<()> {
    let archive_file = NamedTempFile::new()?;
    download_file(client, ui.download_url(), archive_file.path(), user_agent).await?;

    create_parent_dir(target_dir)?;
    let target_parent = target_dir
        .parent()
        .with_context(|| format!("parent directory of `{}` invalid", target_dir.display()))?;
    let extract_dir = tempdir_in(target_parent)?;
    extract_tar_gz(archive_file.path(), extract_dir.path())?;

    let extracted_root = find_archive_root(extract_dir.path())?;
    replace_dir(&extracted_root, target_dir)?;

    println!(
        "{} Installed UI `{}` to {}",
        prefix,
        ui.as_config_value(),
        target_dir.display()
    );
    Ok(())
}

fn extract_tar_gz(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let archive = fs::File::open(archive_path)?;
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    archive.unpack(extract_dir)?;
    Ok(())
}

fn find_archive_root(extract_dir: &Path) -> Result<PathBuf> {
    let mut entries = fs::read_dir(extract_dir)?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    if entries.len() != 1 {
        bail!(
            "expected one root entry in extracted ui archive, found {}",
            entries.len()
        );
    }

    let root = entries.remove(0);
    if !root.is_dir() {
        bail!("expected extracted ui archive root to be a directory");
    }
    Ok(root)
}

fn replace_dir(source_dir: &Path, target_dir: &Path) -> Result<()> {
    create_parent_dir(target_dir)?;

    let parent = target_dir
        .parent()
        .with_context(|| format!("parent directory of `{}` invalid", target_dir.display()))?;

    let staged_dir = parent.join(format!(
        ".{}.tmp",
        target_dir
            .file_name()
            .ok_or_else(|| anyhow!("invalid ui target directory"))?
            .to_string_lossy()
    ));
    let backup_dir = parent.join(format!(
        ".{}.bak",
        target_dir
            .file_name()
            .ok_or_else(|| anyhow!("invalid ui target directory"))?
            .to_string_lossy()
    ));

    if staged_dir.exists() {
        fs::remove_dir_all(&staged_dir)?;
    }
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }

    fs::rename(source_dir, &staged_dir)?;

    if target_dir.exists() {
        fs::rename(target_dir, &backup_dir)?;
    }

    if let Err(err) = fs::rename(&staged_dir, target_dir) {
        if backup_dir.exists() {
            let _ = fs::rename(&backup_dir, target_dir);
        }
        return Err(err).with_context(|| {
            format!(
                "failed to move extracted ui into `{}`",
                target_dir.display()
            )
        });
    }

    if backup_dir.exists() {
        fs::remove_dir_all(backup_dir)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use tar::{Builder, Header};
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn archive_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = Builder::new(encoder);
        for (path, contents) in entries {
            let mut header = Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive.append_data(&mut header, path, *contents).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn extraction_accepts_valid_archives_and_rejects_invalid_gzip() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("ui.tar.gz");
        let output = dir.path().join("output");
        fs::write(
            &archive,
            archive_bytes(&[("dashboard/index.html", b"dashboard")]),
        )
        .unwrap();
        fs::create_dir(&output).unwrap();
        extract_tar_gz(&archive, &output).unwrap();
        assert_eq!(
            fs::read_to_string(output.join("dashboard/index.html")).unwrap(),
            "dashboard"
        );

        fs::write(&archive, b"not gzip").unwrap();
        assert!(extract_tar_gz(&archive, &output).is_err());
    }

    #[test]
    fn archive_root_requires_exactly_one_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_archive_root(dir.path()).is_err());

        fs::write(dir.path().join("root-file"), "content").unwrap();
        assert!(find_archive_root(dir.path())
            .unwrap_err()
            .to_string()
            .contains("root to be a directory"));

        fs::remove_file(dir.path().join("root-file")).unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        assert_eq!(find_archive_root(dir.path()).unwrap(), root);

        fs::create_dir(dir.path().join("second")).unwrap();
        assert!(find_archive_root(dir.path())
            .unwrap_err()
            .to_string()
            .contains("found 2"));
    }

    #[test]
    fn replacement_handles_new_and_existing_targets_and_cleans_stale_staging() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ui");
        let source = dir.path().join("source-one");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("version"), "one").unwrap();
        replace_dir(&source, &target).unwrap();
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "one");
        assert!(!source.exists());

        let stale_stage = dir.path().join(".ui.tmp");
        let stale_backup = dir.path().join(".ui.bak");
        fs::create_dir(&stale_stage).unwrap();
        fs::create_dir(&stale_backup).unwrap();
        fs::write(stale_stage.join("stale"), "stale").unwrap();
        fs::write(stale_backup.join("stale"), "stale").unwrap();
        let source = dir.path().join("source-two");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("version"), "two").unwrap();
        replace_dir(&source, &target).unwrap();
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "two");
        assert!(!stale_stage.exists());
        assert!(!stale_backup.exists());

        assert!(replace_dir(dir.path(), Path::new("/")).is_err());
    }

    #[tokio::test]
    async fn install_downloads_extracts_and_atomically_replaces_dashboard() {
        let server = MockServer::start().await;
        let body = archive_bytes(&[
            ("dashboard/index.html", b"new dashboard"),
            ("dashboard/assets/app.js", b"app"),
        ]);
        Mock::given(method("GET"))
            .and(path("/ui.tar.gz"))
            .and(header("user-agent", "mihoto-test"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested/ui");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("old"), "old dashboard").unwrap();
        let ui = Ui::Custom(format!("{}/ui.tar.gz", server.uri()));
        install_ui(&Client::new(), &ui, &target, "mihoto-test", "test")
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(target.join("index.html")).unwrap(),
            "new dashboard"
        );
        assert_eq!(
            fs::read_to_string(target.join("assets/app.js")).unwrap(),
            "app"
        );
        assert!(!target.join("old").exists());
    }
}
