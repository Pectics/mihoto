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
