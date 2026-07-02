//! A bounded, priority-ordered admission gate.
//!
//! A store `acquire`s a [`GatePermit`] before doing bounded fetch/decode work;
//! when a permit is released the highest-priority waiter is admitted. A waiter
//! cancelled while queued (its acquiring future dropped) is removed before it
//! ever runs, so cancelling one request never starves another. This is the
//! intricate concurrency worth not duplicating across the per-asset stores.

use std::sync::atomic::{AtomicUsize, Ordering};

use keyed_priority_queue::KeyedPriorityQueue;
use parking_lot::Mutex;

use crate::priority::Priority;

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
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `PriorityGate` reads clearly"
)]
#[derive(Debug)]
pub struct PriorityGate {
    /// The gate's mutable state.
    state: Mutex<GateState>,
    /// Signalled whenever a slot frees or priorities change, to re-poll waiters.
    wake: event_listener::Event,
    /// Total concurrency, for reporting.
    capacity: AtomicUsize,
}

impl PriorityGate {
    /// A gate admitting `capacity` concurrent permits.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
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
    pub async fn acquire(&self, id: u64, priority: Priority) -> GatePermit<'_> {
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
    pub fn set_priority(&self, id: u64, priority: Priority) {
        {
            let mut state = self.state.lock();
            if state.waiters.get_priority(&id).is_some() {
                let _old = state.waiters.set_priority(&id, priority);
            }
        }
        let _notified = self.wake.notify(usize::MAX);
    }

    /// Removes a waiter (e.g. a cancelled request) from the queue.
    pub fn remove(&self, id: u64) {
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
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `GatePermit` reads clearly"
)]
#[derive(Debug)]
pub struct GatePermit<'gate> {
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

#[cfg(test)]
mod tests {
    use super::PriorityGate;
    use crate::priority::Priority;

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
