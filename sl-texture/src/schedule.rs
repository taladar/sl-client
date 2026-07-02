//! Priority scheduling, progress observation, and cancellation for texture
//! requests.
//!
//! A [`TextureRequest`] is a cloneable handle to one in-flight (or queued)
//! texture request. It carries the caller's [`Priority`], reports
//! [`TextureProgress`], can be re-prioritized, and cancels the underlying work
//! when the last clone drops (interest-counted, so cancelling one requester
//! never starves another that still wants the same texture). When work is
//! bounded, a priority gate lets the highest-priority queued request proceed
//! first; a queued request that is cancelled is removed before it ever runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use keyed_priority_queue::KeyedPriorityQueue;
use parking_lot::Mutex;
use sl_proto::DiscardLevel;

use crate::entry::TextureEntry;
use crate::store::{TextureError, TextureStore};

/// An abstract scheduling priority: higher is more urgent. How a caller derives
/// it (expected users, on-screen, distance, size on screen) is out of scope —
/// the store only combines and orders by the opaque value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Priority(u32);

impl Priority {
    /// The lowest priority (background / idle work).
    pub const IDLE: Self = Self(0);

    /// A priority from a raw urgency value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The raw urgency value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Combines two requesters' priorities into the effective scheduling
    /// priority. The policy is the maximum: a texture is fetched as urgently as
    /// its most-urgent requester needs.
    #[must_use]
    pub fn combine(first: Self, second: Self) -> Self {
        Self(first.0.max(second.0))
    }
}

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

    /// The combined (max) priority across all requesters, or [`Priority::IDLE`]
    /// when there are none.
    pub(crate) fn effective_priority(&self) -> Priority {
        self.entries
            .iter()
            .map(|(_, priority, _)| *priority)
            .fold(Priority::IDLE, Priority::combine)
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

/// The mutable state of a [`PriorityGate`]: the free-slot count and the
/// priority-ordered set of waiting request ids.
#[derive(Debug)]
struct GateState {
    /// Remaining concurrent work slots.
    slots: usize,
    /// Waiting request ids, ordered by priority (highest served first).
    waiters: KeyedPriorityQueue<u64, Priority>,
}

/// A bounded, priority-ordered admission gate. Callers `acquire` a permit before
/// doing bounded work; when a permit is released the highest-priority waiter is
/// admitted. A waiter cancelled while queued is removed before it runs.
#[derive(Debug)]
pub(crate) struct PriorityGate {
    /// The gate's mutable state.
    state: Mutex<GateState>,
    /// Signalled whenever a slot frees or priorities change, to re-poll waiters.
    wake: event_listener::Event,
    /// Total concurrency, for reporting.
    capacity: AtomicUsize,
}

impl PriorityGate {
    /// A gate admitting `capacity` concurrent permits.
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            state: Mutex::new(GateState {
                slots: capacity,
                waiters: KeyedPriorityQueue::new(),
            }),
            wake: event_listener::Event::new(),
            capacity: AtomicUsize::new(capacity),
        }
    }

    /// Acquires a permit for request `id` at `priority`, waiting behind
    /// higher-priority requests. If the returned future is dropped before it
    /// resolves, the waiter is removed from the queue.
    pub(crate) async fn acquire(&self, id: u64, priority: Priority) -> GatePermit<'_> {
        {
            let mut state = self.state.lock();
            let _existing = state.waiters.push(id, priority);
        }
        // Remove the waiter if this future is dropped before it acquires.
        let mut cleanup = WaiterCleanup {
            gate: self,
            id,
            armed: true,
        };
        loop {
            let listener = self.wake.listen();
            let acquired = {
                let mut state = self.state.lock();
                let is_top = state.waiters.peek().is_some_and(|(top, _)| *top == id);
                let claim = is_top && state.slots > 0;
                if claim {
                    state.slots = state.slots.saturating_sub(1);
                    let _removed = state.waiters.remove(&id);
                }
                drop(state);
                claim
            };
            if acquired {
                cleanup.armed = false;
                return GatePermit { gate: self };
            }
            listener.await;
        }
    }

    /// Re-prioritizes a queued waiter and wakes the gate to re-evaluate.
    pub(crate) fn set_priority(&self, id: u64, priority: Priority) {
        {
            let mut state = self.state.lock();
            if state.waiters.get_priority(&id).is_some() {
                let _old = state.waiters.set_priority(&id, priority);
            }
        }
        let _notified = self.wake.notify(usize::MAX);
    }

    /// Removes a waiter (e.g. a cancelled request) from the queue.
    pub(crate) fn remove(&self, id: u64) {
        {
            let mut state = self.state.lock();
            let _removed = state.waiters.remove(&id);
        }
        let _notified = self.wake.notify(usize::MAX);
    }

    /// Releases one permit, waking waiters to admit the next one.
    fn release(&self) {
        {
            let mut state = self.state.lock();
            state.slots = state
                .slots
                .saturating_add(1)
                .min(self.capacity.load(Ordering::Relaxed));
        }
        let _notified = self.wake.notify(usize::MAX);
    }
}

/// A held admission permit; releases its slot back to the gate on drop.
#[derive(Debug)]
pub(crate) struct GatePermit<'gate> {
    /// The gate to release back to.
    gate: &'gate PriorityGate,
}

impl Drop for GatePermit<'_> {
    fn drop(&mut self) {
        self.gate.release();
    }
}

/// Removes a still-queued waiter from the gate if the acquiring future is
/// dropped before it obtains a permit.
struct WaiterCleanup<'gate> {
    /// The gate to clean up in.
    gate: &'gate PriorityGate,
    /// The waiting request id.
    id: u64,
    /// Whether cleanup is still needed (cleared once a permit is acquired).
    armed: bool,
}

impl Drop for WaiterCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.gate.remove(self.id);
        }
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
    use super::{Priority, PriorityGate, Requesters};
    use pretty_assertions::assert_eq;
    use sl_proto::DiscardLevel;

    #[test]
    fn priority_combine_takes_the_maximum() {
        assert_eq!(
            Priority::combine(Priority::new(3), Priority::new(7)),
            Priority::new(7)
        );
        assert_eq!(Priority::combine(Priority::IDLE, Priority::new(1)).get(), 1);
    }

    #[test]
    fn requesters_aggregate_priority_and_target() {
        let mut requesters = Requesters::default();
        requesters.add(1, Priority::new(2), DiscardLevel::from_clamped(3));
        requesters.add(2, Priority::new(9), DiscardLevel::from_clamped(1));
        assert_eq!(requesters.len(), 2);
        // Effective priority is the max; target want is the finest (smallest).
        assert_eq!(requesters.effective_priority(), Priority::new(9));
        assert_eq!(
            requesters.target_want(),
            Some(DiscardLevel::from_clamped(1))
        );
        // Dropping the higher-priority requester lowers the effective priority.
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

    #[test]
    fn gate_admits_up_to_capacity_then_serialises() {
        pollster::block_on(async {
            let gate = PriorityGate::new(1);
            let first = gate.acquire(1, Priority::new(1)).await;
            // Releasing the only permit lets the next acquirer through.
            drop(first);
            let second = gate.acquire(2, Priority::new(1)).await;
            drop(second);
        });
    }
}
