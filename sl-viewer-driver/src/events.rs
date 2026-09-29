//! Reading the event log: an [`EventCursor`] that reads on from where it
//! stopped, and an [`EventStream`] of what the viewer records from now on.
//! Either way the log is sequence-numbered, so a slow reader misses nothing
//! the viewer still keeps.

use std::time::Duration;

use sl_automation_proto::{
    LogEntry, LogPage, LogStream, RequestId, StateCondition, StateObservation,
};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::error::DriverError;
use crate::viewer::Viewer;

/// A position in a viewer's event log, read forward.
#[derive(Debug, Clone)]
pub struct EventCursor {
    /// The viewer.
    viewer: Viewer,
    /// The next sequence number to read.
    next: u64,
}

impl EventCursor {
    /// A cursor on `viewer`'s log at `next`.
    pub(crate) const fn new(viewer: Viewer, next: u64) -> Self {
        Self { viewer, next }
    }

    /// The next sequence number it reads.
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.next
    }

    /// Every entry of `streams` (every stream when empty) recorded since the
    /// last read, and move past them.
    ///
    /// # Errors
    ///
    /// As any request.
    pub async fn read(&mut self, streams: &[LogStream]) -> Result<Vec<LogEntry>, DriverError> {
        let page = self.viewer.read_log(self.next, streams).await?;
        self.next = page.next;
        Ok(page.entries)
    }

    /// Wait up to `timeout` for an entry of kind `kind` (`ChatReceived`,
    /// `Chat`, `toolbar.inventory`) from here on, and move past it.
    ///
    /// # Errors
    ///
    /// As any request; [`DriverError::Failed`] when none comes.
    pub async fn wait_for(
        &mut self,
        kind: &str,
        timeout: Duration,
    ) -> Result<LogEntry, DriverError> {
        let condition = StateCondition::Logged {
            cursor: self.next,
            streams: Vec::new(),
            kind_is: Some(kind.to_owned()),
            detail_contains: None,
        };
        match self.viewer.wait_for_state(condition, timeout).await? {
            StateObservation::Logged { entry, next } => {
                self.next = next;
                Ok(entry)
            }
            other => Err(DriverError::Unexpected {
                viewer: self.viewer.label().to_owned(),
                what: format!("a wait for a {kind} entry"),
                got: format!("{other:?}"),
            }),
        }
    }
}

/// What a viewer's event log records from the moment it was subscribed, as
/// it records it; the subscription ends when this is dropped.
#[derive(Debug)]
pub struct EventStream {
    /// The viewer.
    viewer: Viewer,
    /// The subscription.
    id: RequestId,
    /// The pages as they arrive.
    pages: UnboundedReceiver<LogPage<LogEntry>>,
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.viewer.unsubscribe(self.id);
    }
}

impl EventStream {
    /// The stream of subscription `id` on `viewer`.
    pub(crate) const fn new(
        viewer: Viewer,
        id: RequestId,
        pages: UnboundedReceiver<LogPage<LogEntry>>,
    ) -> Self {
        Self { viewer, id, pages }
    }

    /// The next page of entries, oldest first — its `dropped` counts entries
    /// the log lost before they could be sent — or `None` once the
    /// connection has closed.
    pub async fn next(&mut self) -> Option<LogPage<LogEntry>> {
        self.pages.recv().await
    }
}
