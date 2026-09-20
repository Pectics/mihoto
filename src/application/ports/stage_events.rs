use crate::application::stage::StageEntry;
use crate::domain::config::Config;

/// Synchronous output boundary for stage progress and summaries.
pub(crate) trait StageEvents {
    fn begin(&mut self, name: &'static str, description: Option<&str>);
    fn summary(&mut self, label: &str, entries: &[StageEntry]);
}

pub(crate) trait InitEvents: StageEvents {
    fn initializing(&mut self);
    fn dashboard(&mut self, config: &Config);
}
