use crate::application::ports::StageEvents;

use std::{fmt, future::Future};

use anyhow::{Error, Result};

pub(crate) enum StageStatus {
    Installed,
    Skipped(String),
    Failed(Error),
}

impl fmt::Debug for StageStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Installed => formatter.write_str("Installed"),
            Self::Skipped(reason) => formatter.debug_tuple("Skipped").field(reason).finish(),
            Self::Failed(error) => formatter
                .debug_tuple("Failed")
                .field(&format_args!("{error:#}"))
                .finish(),
        }
    }
}

pub(crate) struct StageEntry {
    pub(crate) name: &'static str,
    pub(crate) status: StageStatus,
}

pub(crate) struct StageReport<E> {
    entries: Vec<StageEntry>,
    events: E,
}

impl<E: StageEvents> StageReport<E> {
    pub(crate) fn new(events: E) -> Self {
        Self {
            entries: Vec::new(),
            events,
        }
    }

    pub(crate) fn begin(&mut self, name: &'static str, description: Option<&str>) {
        self.events.begin(name, description);
    }

    pub(crate) async fn run<F, Fut>(
        &mut self,
        name: &'static str,
        description: Option<&str>,
        operation: F,
    ) where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<StageStatus>>,
    {
        self.begin(name, description);
        let status = match operation().await {
            Ok(status) => status,
            Err(error) => StageStatus::Failed(error),
        };
        self.record(name, status);
    }

    pub(crate) fn record(&mut self, name: &'static str, status: StageStatus) {
        self.entries.push(StageEntry { name, status });
    }

    pub(crate) fn print(&mut self, label: &str) {
        self.events.summary(label, &self.entries);
    }

    pub(crate) fn events_mut(&mut self) -> &mut E {
        &mut self.events
    }

    pub(crate) fn has_failures(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.status, StageStatus::Failed(_)))
    }

    pub(crate) fn stage_failed(&self, name: &'static str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.name == name && matches!(entry.status, StageStatus::Failed(_)))
    }

    pub(crate) fn has_installed(&self, name: &'static str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.name == name && matches!(entry.status, StageStatus::Installed))
    }
}
