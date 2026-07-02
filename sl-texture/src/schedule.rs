//! Progress observation, per-texture requester aggregation, and cancellation for
//! texture requests.
//!
//! A [`TextureRequest`] is a cloneable handle to one in-flight (or queued)
//! texture request. It carries the caller's [`Priority`], reports
//! [`TextureProgress`], can be re-prioritized, and cancels the underlying work
//! when the last clone drops (interest-counted, so cancelling one requester
//! never starves another that still wants the same texture). When work is
//! bounded, the shared [`PriorityGate`](sl_asset_sched::PriorityGate) lets the
//! highest-priority queued request proceed first; a queued request that is
//! cancelled is removed before it ever runs.
//!
//! [`Priority`] and the gate live in the shared `sl-asset-sched` crate; only the
//! LOD-carrying pieces ([`TextureProgress`], [`Requesters`], [`TextureRequest`])
//! stay here.

use std::sync::Arc;

use sl_asset_sched::popularity_boost;
use sl_proto::DiscardLevel;

pub use sl_asset_sched::Priority;

use crate::entry::TextureEntry;
use crate::store::{TextureError, TextureStore};

/// The observable state of a texture request as it progresses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextureProgress {
    /// Registered and awaiting a work slot.
    Queued,
    /// Reading the raw codestream from the on-disk cache.
    ReadingDisk {
        /// Bytes read from disk so far.
        read: usize,
        /// Bytes needed for the target level of detail.
        total: usize,
    },
    /// Downloading the raw codestream over HTTP (`GetTexture`).
    Downloading {
        /// Bytes fetched so far.
        covered: usize,
        /// Bytes needed for the target level of detail.
        needed: usize,
    },
    /// Decoding the codestream to pixels.
    Decoding,
    /// Decoded and available at the given level of detail.
    Ready(DiscardLevel),
    /// The fetch or decode failed.
    Failed,
    /// Cancelled before completion (the last requester withdrew).
    Cancelled,
}

/// The set of live requesters' `(priority, target)` contributions for one
/// texture, keyed by request id.
#[derive(Debug, Default)]
pub struct Requesters {
    /// Each live request's `(id, priority, target)`.
    entries: Vec<(u64, Priority, DiscardLevel)>,
}

impl Requesters {
    /// Adds (or updates) a requester's contribution.
    pub(crate) fn add(&mut self, id: u64, priority: Priority, target: DiscardLevel) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|(entry_id, _, _)| *entry_id == id)
        {
            *existing = (id, priority, target);
        } else {
            self.entries.push((id, priority, target));
        }
    }

    /// Updates a requester's priority, if present.
    pub(crate) fn set_priority(&mut self, id: u64, priority: Priority) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|(entry_id, _, _)| *entry_id == id)
        {
            existing.1 = priority;
        }
    }

    /// Removes a requester, if present.
    pub(crate) fn remove(&mut self, id: u64) {
        self.entries.retain(|(entry_id, _, _)| *entry_id != id);
    }

    /// The number of live requesters.
    pub(crate) const fn len(&self) -> usize {
        self.entries.len()
    }

    /// The effective scheduling priority: the maximum requester priority plus a
    /// diminishing popularity boost for the requester count (see
    /// [`popularity_boost`]). [`Priority::IDLE`] when there are no requesters.
    pub(crate) fn effective_priority(&self) -> Priority {
        if self.entries.is_empty() {
            return Priority::IDLE;
        }
        let base = self
            .entries
            .iter()
            .map(|(_, priority, _)| *priority)
            .fold(Priority::IDLE, Priority::combine);
        Priority::new(
            base.get()
                .saturating_add(popularity_boost(self.entries.len())),
        )
    }

    /// The finest (smallest discard) target any requester wants, or `None` when
    /// there are no requesters.
    pub(crate) fn target_want(&self) -> Option<DiscardLevel> {
        self.entries
            .iter()
            .map(|(_, _, target)| *target)
            .min_by_key(|target| target.get())
    }
}

/// The shared inner state of a [`TextureRequest`].
#[derive(Debug)]
struct RequestInner {
    /// The store the request runs against.
    store: TextureStore,
    /// The texture entry this request targets.
    entry: Arc<TextureEntry>,
    /// This request's unique id (its contribution key on the entry and gate).
    id: u64,
    /// The level of detail this request wants.
    target: DiscardLevel,
}

impl Drop for RequestInner {
    fn drop(&mut self) {
        let remaining = self.entry.remove_requester(self.id);
        self.store.gate().remove(self.id);
        if remaining == 0 && self.entry.current_discard().is_none() {
            self.entry.set_progress(TextureProgress::Cancelled);
        }
    }
}

/// A cloneable handle to a queued or in-flight texture request: observe its
/// [`progress`](Self::progress), await [`resolved`](Self::resolved), re-prioritize
/// with [`set_priority`](Self::set_priority), or drop every clone to cancel.
#[derive(Clone, Debug)]
pub struct TextureRequest(Arc<RequestInner>);

impl TextureRequest {
    /// Builds a request handle, registering its contribution on the entry.
    pub(crate) fn new(
        store: TextureStore,
        entry: Arc<TextureEntry>,
        id: u64,
        priority: Priority,
        target: DiscardLevel,
    ) -> Self {
        entry.add_requester(id, priority, target);
        entry.set_progress(TextureProgress::Queued);
        Self(Arc::new(RequestInner {
            store,
            entry,
            id,
            target,
        }))
    }

    /// The request's current progress.
    #[must_use]
    pub fn progress(&self) -> TextureProgress {
        self.0.entry.progress()
    }

    /// Awaits the next progress transition (for a poll-free observer).
    pub async fn changed(&self) {
        self.0.entry.progress_changed().await;
    }

    /// The shared texture entry (its decoded image is available once
    /// [`progress`](Self::progress) reports [`TextureProgress::Ready`]).
    #[must_use]
    pub fn entry(&self) -> Arc<TextureEntry> {
        Arc::clone(&self.0.entry)
    }

    /// Re-prioritizes this request; takes effect immediately for work still
    /// queued behind the gate.
    pub fn set_priority(&self, priority: Priority) {
        self.0.entry.set_requester_priority(self.0.id, priority);
        self.0
            .store
            .gate()
            .set_priority(self.0.id, self.0.entry.effective_priority());
    }

    /// Drives the request to completion (through the priority gate), returning
    /// the decoded entry. Concurrent callers for the same texture share its work.
    ///
    /// # Errors
    ///
    /// Returns [`TextureError::Cancelled`] if all requesters withdrew before the
    /// work started, or a fetch/decode error otherwise.
    pub async fn resolved(&self) -> Result<Arc<TextureEntry>, TextureError> {
        let inner = &self.0;
        let priority = inner.entry.effective_priority();
        let _permit = inner.store.gate().acquire(inner.id, priority).await;
        if inner.entry.interest() == 0 {
            inner.entry.set_progress(TextureProgress::Cancelled);
            return Err(TextureError::Cancelled);
        }
        inner.store.drive(&inner.entry, inner.target).await?;
        Ok(Arc::clone(&inner.entry))
    }
}

#[cfg(test)]
mod tests {
    use super::{Priority, Requesters};
    use pretty_assertions::assert_eq;
    use sl_proto::DiscardLevel;

    #[test]
    fn requesters_aggregate_priority_and_target() {
        let mut requesters = Requesters::default();
        requesters.add(1, Priority::new(2), DiscardLevel::from_clamped(3));
        requesters.add(2, Priority::new(9), DiscardLevel::from_clamped(1));
        assert_eq!(requesters.len(), 2);
        // Effective = max(2, 9) + popularity boost for 2 requesters (log2(2)*4=4).
        assert_eq!(requesters.effective_priority(), Priority::new(13));
        assert_eq!(
            requesters.target_want(),
            Some(DiscardLevel::from_clamped(1))
        );
        // Dropping the higher-priority requester lowers to max(2) + boost(0).
        requesters.remove(2);
        assert_eq!(requesters.effective_priority(), Priority::new(2));
        assert_eq!(
            requesters.target_want(),
            Some(DiscardLevel::from_clamped(3))
        );
        requesters.remove(1);
        assert_eq!(requesters.effective_priority(), Priority::IDLE);
        assert_eq!(requesters.target_want(), None);
    }
}
