use std::future::Future;

use anyhow::Result;

pub(crate) trait UpgradeOperations {
    fn check(&self) -> impl Future<Output = Result<Option<String>>>;
    fn install(&self, no_confirm: bool, target: Option<String>)
        -> impl Future<Output = Result<()>>;
}
