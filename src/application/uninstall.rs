use crate::application::ports::UninstallOperations;

use std::path::Path;

use anyhow::Result;

pub(crate) fn run(
    operations: &impl UninstallOperations,
    purge: bool,
    manager_config_path: &Path,
) -> Result<()> {
    operations.uninstall(purge, manager_config_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, path::PathBuf};

    #[derive(Default)]
    struct FakeOperations(RefCell<Vec<(bool, PathBuf)>>);

    impl UninstallOperations for FakeOperations {
        fn uninstall(&self, purge: bool, manager_config_path: &Path) -> Result<()> {
            self.0
                .borrow_mut()
                .push((purge, manager_config_path.to_owned()));
            Ok(())
        }
    }

    #[test]
    fn forwards_purge_and_manager_config_path() {
        let operations = FakeOperations::default();
        run(&operations, true, Path::new("/custom/mihoto.toml")).unwrap();
        assert_eq!(
            *operations.0.borrow(),
            [(true, PathBuf::from("/custom/mihoto.toml"))]
        );
    }
}
