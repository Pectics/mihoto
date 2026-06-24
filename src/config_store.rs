use crate::config::{render_mihomo_overlay, render_mihomo_override, MihomoConfig};
use crate::utils::create_parent_dir;
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[allow(dead_code)]
pub const DEFAULT_SYSTEM_CONFIG_ROOT: &str = "/etc/mihomo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigGenerationPaths {
    pub root: PathBuf,
    pub source_yaml: PathBuf,
    pub overlay_yaml: PathBuf,
    pub candidate_yaml: PathBuf,
    pub active_yaml: PathBuf,
    pub last_good_yaml: PathBuf,
    pub compat_config_yaml: PathBuf,
}

impl ConfigGenerationPaths {
    pub fn for_user_root(root: impl AsRef<Path>) -> Self {
        Self::from_root(root.as_ref())
    }

    #[allow(dead_code)]
    pub fn system_root() -> Self {
        Self::from_root(Path::new(DEFAULT_SYSTEM_CONFIG_ROOT))
    }

    fn from_root(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            source_yaml: root.join("source.yaml"),
            overlay_yaml: root.join("overlay.yaml"),
            candidate_yaml: root.join("candidate.yaml"),
            active_yaml: root.join("active.yaml"),
            last_good_yaml: root.join("last-good.yaml"),
            compat_config_yaml: root.join("config.yaml"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigGenerationStore {
    pub paths: ConfigGenerationPaths,
}

impl ConfigGenerationStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            paths: ConfigGenerationPaths::for_user_root(root),
        }
    }

    pub fn seed_source_from_legacy_config(&self) -> Result<bool> {
        if !self.paths.compat_config_yaml.exists() {
            return Ok(false);
        }

        let mut changed = false;
        if !self.paths.source_yaml.exists() {
            create_parent_dir(&self.paths.source_yaml)?;
            fs::copy(&self.paths.compat_config_yaml, &self.paths.source_yaml).with_context(
                || {
                    format!(
                        "failed to seed `{}` from `{}`",
                        self.paths.source_yaml.display(),
                        self.paths.compat_config_yaml.display()
                    )
                },
            )?;
            changed = true;
        }
        if !self.paths.active_yaml.exists() {
            create_parent_dir(&self.paths.active_yaml)?;
            fs::copy(&self.paths.compat_config_yaml, &self.paths.active_yaml).with_context(
                || {
                    format!(
                        "failed to seed `{}` from `{}`",
                        self.paths.active_yaml.display(),
                        self.paths.compat_config_yaml.display()
                    )
                },
            )?;
            changed = true;
        }

        Ok(changed)
    }

    pub fn render_candidate(&self, override_config: &MihomoConfig) -> Result<bool> {
        let source_path = self.source_path_for_render()?;
        let overlay_changed = render_mihomo_overlay(&self.paths.overlay_yaml, override_config)?;
        let candidate_changed =
            render_mihomo_override(&source_path, &self.paths.candidate_yaml, override_config)?;
        Ok(overlay_changed || candidate_changed)
    }

    pub fn install_source_from_stage(&self, staged_path: &Path) -> Result<()> {
        create_parent_dir(&self.paths.source_yaml)?;
        fs::rename(staged_path, &self.paths.source_yaml).with_context(|| {
            format!(
                "failed to replace `{}` with staged source `{}`",
                self.paths.source_yaml.display(),
                staged_path.display()
            )
        })
    }

    pub fn activate_candidate(&self) -> Result<bool> {
        let candidate = fs::read(&self.paths.candidate_yaml).with_context(|| {
            format!(
                "failed to read candidate config `{}`",
                self.paths.candidate_yaml.display()
            )
        })?;
        let active = fs::read(&self.paths.active_yaml).ok();
        let compat = fs::read(&self.paths.compat_config_yaml).ok();

        if active.as_deref() == Some(candidate.as_slice())
            && compat.as_deref() == Some(candidate.as_slice())
        {
            return Ok(false);
        }

        if let Some(active) = active {
            if active != candidate {
                create_parent_dir(&self.paths.last_good_yaml)?;
                atomic_write(&self.paths.last_good_yaml, &active)?;
            }
        }

        create_parent_dir(&self.paths.active_yaml)?;
        atomic_write(&self.paths.active_yaml, &candidate)?;
        create_parent_dir(&self.paths.compat_config_yaml)?;
        atomic_write(&self.paths.compat_config_yaml, &candidate)?;
        Ok(true)
    }

    pub fn candidate_matches_active_and_compat(&self) -> Result<bool> {
        let candidate = match fs::read(&self.paths.candidate_yaml) {
            Ok(candidate) => candidate,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to read candidate config `{}`",
                        self.paths.candidate_yaml.display()
                    )
                })
            }
        };
        let active = fs::read(&self.paths.active_yaml).ok();
        let compat = fs::read(&self.paths.compat_config_yaml).ok();
        Ok(active.as_deref() == Some(candidate.as_slice())
            && compat.as_deref() == Some(candidate.as_slice()))
    }

    pub fn restore_last_good(&self) -> Result<bool> {
        let last_good = match fs::read(&self.paths.last_good_yaml) {
            Ok(last_good) => last_good,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to read last-good config `{}`",
                        self.paths.last_good_yaml.display()
                    )
                })
            }
        };

        create_parent_dir(&self.paths.active_yaml)?;
        atomic_write(&self.paths.active_yaml, &last_good)?;
        create_parent_dir(&self.paths.compat_config_yaml)?;
        atomic_write(&self.paths.compat_config_yaml, &last_good)?;
        Ok(true)
    }

    fn source_path_for_render(&self) -> Result<PathBuf> {
        if self.paths.source_yaml.exists() {
            return Ok(self.paths.source_yaml.clone());
        }
        if self.paths.active_yaml.exists() {
            return Ok(self.paths.active_yaml.clone());
        }
        if self.paths.compat_config_yaml.exists() {
            return Ok(self.paths.compat_config_yaml.clone());
        }
        anyhow::bail!(
            "no source config available under `{}`",
            self.paths.root.display()
        )
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    create_parent_dir(path)?;
    let parent = path
        .parent()
        .with_context(|| format!("parent directory of `{}` invalid", path.display()))?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map(|_| ())
        .map_err(|err| err.error)
        .with_context(|| format!("failed to atomically write `{}`", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MihomoConfig;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn source_yaml(port: u16) -> String {
        format!(
            "
port: {port}
socks-port: 8081
mode: rule
proxies:
  - name: test
    type: http
    server: example.com
    port: 443
"
        )
    }

    #[test]
    fn paths_are_derived_from_user_config_root() {
        let paths = ConfigGenerationPaths::for_user_root("/tmp/mihomo");

        assert_eq!(paths.root, PathBuf::from("/tmp/mihomo"));
        assert_eq!(paths.source_yaml, PathBuf::from("/tmp/mihomo/source.yaml"));
        assert_eq!(
            paths.overlay_yaml,
            PathBuf::from("/tmp/mihomo/overlay.yaml")
        );
        assert_eq!(
            paths.candidate_yaml,
            PathBuf::from("/tmp/mihomo/candidate.yaml")
        );
        assert_eq!(paths.active_yaml, PathBuf::from("/tmp/mihomo/active.yaml"));
        assert_eq!(
            paths.last_good_yaml,
            PathBuf::from("/tmp/mihomo/last-good.yaml")
        );
        assert_eq!(
            paths.compat_config_yaml,
            PathBuf::from("/tmp/mihomo/config.yaml")
        );
    }

    #[test]
    fn system_paths_default_to_etc_mihomo() {
        let paths = ConfigGenerationPaths::system_root();

        assert_eq!(paths.root, PathBuf::from("/etc/mihomo"));
        assert_eq!(paths.source_yaml, PathBuf::from("/etc/mihomo/source.yaml"));
        assert_eq!(
            paths.compat_config_yaml,
            PathBuf::from("/etc/mihomo/config.yaml")
        );
    }

    #[test]
    fn render_candidate_does_not_modify_active_last_good_or_compat() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let store = ConfigGenerationStore::new(dir.path());
        fs::write(&store.paths.source_yaml, source_yaml(8080))?;
        fs::write(&store.paths.active_yaml, "active: old\n")?;
        fs::write(&store.paths.last_good_yaml, "last: good\n")?;
        fs::write(&store.paths.compat_config_yaml, "compat: current\n")?;

        let mut override_config = MihomoConfig::default();
        override_config.port = 9999;
        override_config.socks_port = 9998;

        let changed = store.render_candidate(&override_config)?;

        assert!(changed);
        let candidate = fs::read_to_string(&store.paths.candidate_yaml)?;
        assert!(candidate.contains("port: 9999"));
        assert!(candidate.contains("socks-port: 9998"));
        assert!(candidate.contains("proxies:"));
        let overlay = fs::read_to_string(&store.paths.overlay_yaml)?;
        assert!(overlay.contains("port: 9999"));
        assert_eq!(
            fs::read_to_string(&store.paths.active_yaml)?,
            "active: old\n"
        );
        assert_eq!(
            fs::read_to_string(&store.paths.last_good_yaml)?,
            "last: good\n"
        );
        assert_eq!(
            fs::read_to_string(&store.paths.compat_config_yaml)?,
            "compat: current\n"
        );
        Ok(())
    }

    #[test]
    fn activate_candidate_backs_up_previous_active_and_updates_compat() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let store = ConfigGenerationStore::new(dir.path());
        fs::write(&store.paths.active_yaml, "port: 1111\n")?;
        fs::write(&store.paths.compat_config_yaml, "port: 1111\n")?;
        fs::write(&store.paths.candidate_yaml, "port: 2222\n")?;

        let changed = store.activate_candidate()?;

        assert!(changed);
        assert_eq!(
            fs::read_to_string(&store.paths.last_good_yaml)?,
            "port: 1111\n"
        );
        assert_eq!(
            fs::read_to_string(&store.paths.active_yaml)?,
            "port: 2222\n"
        );
        assert_eq!(
            fs::read_to_string(&store.paths.compat_config_yaml)?,
            "port: 2222\n"
        );
        Ok(())
    }

    #[test]
    fn legacy_config_yaml_seeds_missing_generation_files() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let store = ConfigGenerationStore::new(dir.path());
        fs::write(&store.paths.compat_config_yaml, source_yaml(8080))?;

        let seeded = store.seed_source_from_legacy_config()?;

        assert!(seeded);
        assert_eq!(
            fs::read_to_string(&store.paths.source_yaml)?,
            fs::read_to_string(&store.paths.compat_config_yaml)?
        );
        assert_eq!(
            fs::read_to_string(&store.paths.active_yaml)?,
            fs::read_to_string(&store.paths.compat_config_yaml)?
        );
        Ok(())
    }

    #[test]
    fn restore_last_good_reverts_active_and_compat_config() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let store = ConfigGenerationStore::new(dir.path());
        fs::write(&store.paths.last_good_yaml, "port: 1111\n")?;
        fs::write(&store.paths.active_yaml, "port: 2222\n")?;
        fs::write(&store.paths.compat_config_yaml, "port: 2222\n")?;

        let restored = store.restore_last_good()?;

        assert!(restored);
        assert_eq!(
            fs::read_to_string(&store.paths.active_yaml)?,
            "port: 1111\n"
        );
        assert_eq!(
            fs::read_to_string(&store.paths.compat_config_yaml)?,
            "port: 1111\n"
        );
        Ok(())
    }

    #[test]
    fn install_source_from_stage_replaces_source_atomically() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let store = ConfigGenerationStore::new(dir.path());
        let staged = dir.path().join(".source-download.tmp");
        fs::write(&store.paths.source_yaml, "port: 1111\n")?;
        fs::write(&staged, "port: 2222\n")?;

        store.install_source_from_stage(&staged)?;

        assert_eq!(
            fs::read_to_string(&store.paths.source_yaml)?,
            "port: 2222\n"
        );
        assert!(!staged.exists());
        Ok(())
    }
}
