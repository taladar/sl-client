//! Progress observation, per-mesh requester aggregation, and cancellation for
//! mesh requests.
//!
//! A [`MeshRequest`] is a cloneable handle to one in-flight (or queued) mesh
//! request. It carries the caller's [`Priority`], reports [`MeshProgress`], can
//! be re-prioritized, and cancels the underlying work when the last clone drops
//! (interest-counted, so cancelling one requester never starves another that
//! still wants the same mesh). When work is bounded, the shared
//! [`PriorityGate`](sl_asset_sched::PriorityGate) lets the highest-priority
//! queued request proceed first; a queued request that is cancelled is removed
//! before it ever runs.
//!
//! [`Priority`] and the gate live in the shared `sl-asset-sched` crate; only the
//! LOD-carrying pieces ([`MeshProgress`], [`Requesters`], [`MeshRequest`]) stay
//! here.

use std::sync::Arc;

use sl_asset_sched::popularity_boost;
use sl_proto::MeshLod;

pub use sl_asset_sched::Priority;

use crate::entry::MeshEntry;
use crate::store::{MeshError, MeshStore};

/// The observable state of a mesh request as it progresses.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `MeshProgress` reads clearly"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeshProgress {
    /// Registered and awaiting a work slot.
    Queued,
    /// Reading cached asset bytes from the on-disk cache.
    ReadingDisk {
        /// Bytes read from disk so far.
        read: usize,
        /// Bytes needed for the target level of detail.
        total: usize,
    },
    /// Downloading asset bytes over HTTP (`GetMesh2` / `GetMesh`).
    Downloading {
        /// Bytes fetched so far.
        covered: usize,
        /// Bytes needed for the target level of detail.
        needed: usize,
    },
    /// Inflating and decoding a block to geometry.
    Decoding,
    /// Decoded and available at the given level of detail.
    Ready(MeshLod),
    /// The fetch or decode failed.
    Failed,
    /// Cancelled before completion (the last requester withdrew).
    Cancelled,
}

/// The set of live requesters' `(priority, target)` contributions for one mesh,
/// keyed by request id.
#[derive(Debug, Default)]
pub struct Requesters {
    /// Each live request's `(id, priority, target)`.
    entries: Vec<(u64, Priority, MeshLod)>,
}

impl Requesters {
    /// Adds (or updates) a requester's contribution.
    pub(crate) fn add(&mut self, id: u64, priority: Priority, target: MeshLod) {
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

    /// The finest level any requester wants (via [`MeshLod::finer_of`]), or
    /// `None` when there are no requesters.
    pub(crate) fn target_want(&self) -> Option<MeshLod> {
        self.entries
            .iter()
            .map(|(_, _, target)| *target)
            .reduce(MeshLod::finer_of)
    }
}

/// The shared inner state of a [`MeshRequest`].
#[derive(Debug)]
struct RequestInner {
    /// The store the request runs against.
    store: MeshStore,
    /// The mesh entry this request targets.
    entry: Arc<MeshEntry>,
    /// This request's unique id (its contribution key on the entry and gate).
    id: u64,
    /// The level of detail this request wants.
    target: MeshLod,
}

impl Drop for RequestInner {
    fn drop(&mut self) {
        let remaining = self.entry.remove_requester(self.id);
        self.store.gate().remove(self.id);
        if remaining == 0 && self.entry.current_lod().is_none() {
            self.entry.set_progress(MeshProgress::Cancelled);
        }
    }
}

/// A cloneable handle to a queued or in-flight mesh request: observe its
/// [`progress`](Self::progress), await [`resolved`](Self::resolved),
/// re-prioritize with [`set_priority`](Self::set_priority), or drop every clone
/// to cancel.
#[derive(Clone, Debug)]
pub struct MeshRequest(Arc<RequestInner>);

impl MeshRequest {
    /// Builds a request handle, registering its contribution on the entry.
    pub(crate) fn new(
        store: MeshStore,
        entry: Arc<MeshEntry>,
        id: u64,
        priority: Priority,
        target: MeshLod,
    ) -> Self {
        entry.add_requester(id, priority, target);
        entry.set_progress(MeshProgress::Queued);
        Self(Arc::new(RequestInner {
            store,
            entry,
            id,
            target,
        }))
    }

    /// The request's current progress.
    #[must_use]
    pub fn progress(&self) -> MeshProgress {
        self.0.entry.progress()
    }

    /// Awaits the next progress transition (for a poll-free observer).
    pub async fn changed(&self) {
        self.0.entry.progress_changed().await;
    }

    /// The shared mesh entry (its decoded geometry is available once
    /// [`progress`](Self::progress) reports [`MeshProgress::Ready`]).
    #[must_use]
    pub fn entry(&self) -> Arc<MeshEntry> {
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
    /// the decoded entry. Concurrent callers for the same mesh share its work.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError::Cancelled`] if all requesters withdrew before the
    /// work started, or a fetch/decode error otherwise.
    pub async fn resolved(&self) -> Result<Arc<MeshEntry>, MeshError> {
        let inner = &self.0;
        let priority = inner.entry.effective_priority();
        let _permit = inner.store.gate().acquire(inner.id, priority).await;
        if inner.entry.interest() == 0 {
            inner.entry.set_progress(MeshProgress::Cancelled);
            return Err(MeshError::Cancelled);
        }
        inner.store.drive(&inner.entry, inner.target).await?;
        Ok(Arc::clone(&inner.entry))
    }
}

#[cfg(test)]
mod tests {
    use super::{Priority, Requesters};
    use pretty_assertions::assert_eq;
    use sl_proto::MeshLod;

    #[test]
    fn requesters_aggregate_priority_and_finest_target() {
        let mut requesters = Requesters::default();
        requesters.add(1, Priority::new(2), MeshLod::Low);
        requesters.add(2, Priority::new(9), MeshLod::High);
        assert_eq!(requesters.len(), 2);
        // Effective = max(2, 9) + popularity boost for 2 requesters (log2(2)*4=4).
        assert_eq!(requesters.effective_priority(), Priority::new(13));
        // The finest wanted level is High.
        assert_eq!(requesters.target_want(), Some(MeshLod::High));
        // Dropping the High requester leaves Low as the finest wanted.
        requesters.remove(2);
        assert_eq!(requesters.effective_priority(), Priority::new(2));
        assert_eq!(requesters.target_want(), Some(MeshLod::Low));
        requesters.remove(1);
        assert_eq!(requesters.effective_priority(), Priority::IDLE);
        assert_eq!(requesters.target_want(), None);
    }
}
