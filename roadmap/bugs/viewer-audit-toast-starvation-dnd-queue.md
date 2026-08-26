---
id: viewer-audit-toast-starvation-dnd-queue
title: A low-priority toast can be queued forever, and the DND hold list is unbounded
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-notices/src/notification_host.rs` — `MAX_VISIBLE_TOASTS = 1`
(`:175`), `age_and_fade_toasts` skips any toast with `toast.overflowed` so its
timer is paused (`:1256`), and `order_channel_by_priority` re-sorts the whole
channel by priority on every `Added<Toast>` (`:1403`).

Under a steady stream of higher-priority toasts a low-priority one is therefore
**never promoted and never expires** — the queue only drains when the user
clicks "N more". Unbounded entity growth with no cap, reachable from ordinary
in-world traffic.

Separately, `:270` — `DoNotDisturbQueue::held: Vec<ShowNotification>` is
unbounded and drained only on the DND falling edge, so a long DND session under
a notification flood grows without limit and then replays every held item at
once into the one-visible channel.

The 7 existing tests cover suppression, last-response and input-field
resolution; none covers the overflow logic. Extract
`visible_split(ordered) -> (visible, hidden)` and a
`should_age(overflowed, hovered, lifetime)` predicate and assert the missing
invariant: a bounded number of higher-priority arrivals cannot keep the same
toast queued forever.
