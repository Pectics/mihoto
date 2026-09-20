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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct EventLog {
        begins: Vec<(&'static str, Option<String>)>,
        summaries: Vec<(String, Vec<String>)>,
    }

    #[derive(Clone)]
    struct RecordingEvents(Rc<RefCell<EventLog>>);

    impl StageEvents for RecordingEvents {
        fn begin(&mut self, name: &'static str, description: Option<&str>) {
            self.0
                .borrow_mut()
                .begins
                .push((name, description.map(str::to_owned)));
        }

        fn summary(&mut self, label: &str, entries: &[StageEntry]) {
            self.0.borrow_mut().summaries.push((
                label.to_owned(),
                entries
                    .iter()
                    .map(|entry| format!("{}:{:?}", entry.name, entry.status))
                    .collect(),
            ));
        }
    }

    #[tokio::test]
    async fn report_records_events_status_queries_and_errors() {
        let log = Rc::new(RefCell::new(EventLog::default()));
        let mut report = StageReport::new(RecordingEvents(log.clone()));

        report
            .run("installed", Some("working"), || async {
                Ok(StageStatus::Installed)
            })
            .await;
        report
            .run("failed", None, || async {
                Err(anyhow::anyhow!("outer").context("inner"))
            })
            .await;
        report.record("skipped", StageStatus::Skipped("not needed".to_string()));

        assert!(report.has_installed("installed"));
        assert!(!report.has_installed("skipped"));
        assert!(report.has_failures());
        assert!(report.stage_failed("failed"));
        assert!(!report.stage_failed("missing"));
        report.print("summary label");

        let log = log.borrow();
        assert_eq!(
            log.begins,
            [("installed", Some("working".to_string())), ("failed", None)]
        );
        assert_eq!(log.summaries[0].0, "summary label");
        assert_eq!(
            log.summaries[0].1,
            [
                "installed:Installed",
                "failed:Failed(inner: outer)",
                "skipped:Skipped(\"not needed\")"
            ]
        );
    }

    #[test]
    fn events_mut_exposes_the_owned_event_sink() {
        let log = Rc::new(RefCell::new(EventLog::default()));
        let mut report = StageReport::new(RecordingEvents(log.clone()));
        report.events_mut().begin("manual", None);
        assert_eq!(log.borrow().begins, [("manual", None)]);
    }
}
