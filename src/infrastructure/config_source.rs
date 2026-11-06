use crate::infrastructure::download::download_file;
use crate::infrastructure::filesystem::create_parent_dir;

use std::path::Path;

use anyhow::{bail, Context, Result};
use reqwest::{Client, Url};

/// Fetch a subscription into a staging file; local sources are read without modification.
pub(crate) async fn fetch_config(
    client: &Client,
    source: &str,
    destination: &Path,
    user_agent: &str,
) -> Result<()> {
    let Some((_, rest)) = source
        .split_once(':')
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("file"))
    else {
        return download_file(client, source, destination, user_agent).await;
    };
    if !rest.starts_with('/') {
        bail!("file URL must use an absolute path, for example file:///etc/mihoto-nodes.yaml");
    }
    let url = Url::parse(source).context("invalid subscription file URL")?;
    if url.query().is_some() || url.fragment().is_some() {
        bail!("subscription file URL must not contain a query or fragment; URL-encode path characters");
    }
    let path = url.to_file_path().map_err(|()| {
        anyhow::anyhow!(
            "subscription file URL must refer to a local path (host must be empty or localhost)"
        )
    })?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .with_context(|| format!("failed to access local subscription {}", path.display()))?;
    if !metadata.is_file() {
        bail!(
            "local subscription must be a regular file: {}",
            path.display()
        );
    }
    let contents = tokio::fs::read(&path)
        .await
        .with_context(|| format!("failed to read local subscription {}", path.display()))?;
    create_parent_dir(destination)?;
    // Write into the existing private staging file, preserving its permissions.
    tokio::fs::write(destination, contents)
        .await
        .context("failed to stage local subscription")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[tokio::test]
    async fn local_url_forms_decode_paths_and_preserve_source_and_staging_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("节点 #1.yaml");
        let contents = b"rules: [MATCH,DIRECT]\n";
        fs::write(&source, contents).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
        let url = Url::from_file_path(&source).unwrap();
        for address in [
            url.to_string(),
            format!("file:{}", url.path()),
            format!("FILE://localhost{}", url.path()),
        ] {
            let staged = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            fetch_config(&Client::new(), &address, staged.path(), "unused")
                .await
                .unwrap();
            assert_eq!(fs::read(staged.path()).unwrap(), contents);
            assert_eq!(
                fs::metadata(staged.path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(fs::read(&source).unwrap(), contents);
            assert_eq!(
                fs::metadata(&source).unwrap().permissions().mode() & 0o777,
                0o644
            );
        }
    }

    #[tokio::test]
    async fn rejected_file_sources_do_not_change_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("staged.yaml");
        fs::write(&destination, "unchanged").unwrap();
        for source in [
            "file:relative.yaml".to_string(),
            "file://remote.example/config.yaml".to_string(),
            "file://[invalid".to_string(),
            "file:///tmp/config.yaml?query".to_string(),
            "file:///tmp/config.yaml#fragment".to_string(),
            Url::from_directory_path(dir.path()).unwrap().to_string(),
            Url::from_file_path(dir.path().join("missing.yaml"))
                .unwrap()
                .to_string(),
        ] {
            assert!(
                fetch_config(&Client::new(), &source, &destination, "unused")
                    .await
                    .is_err(),
                "{source}"
            );
            assert_eq!(fs::read_to_string(&destination).unwrap(), "unchanged");
        }
    }
}
