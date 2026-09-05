---
id: viewer-audit-toast-starvation-dnd-queue
title: A low-priority toast can be queued forever, and the DND hold list is unbounded
topic: viewer
status: done
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

## Done

A queued toast now runs a timer of its own. `Toast::queued` accrues exactly when
`Toast::age` does not — `age_and_fade_toasts` advances one or the other, never
both — and `aged_rank(priority, queued)` raises the toast one priority step per
`QUEUE_AGING_SECS` (20 s), capped at `Critical`. `compare_toasts` sorts by that
rank, then by the **longer wait**, then (as before) by age. Three properties
follow:

- **The wait is bounded.** Four steps take anything to the top rank, where the
  wait tie-break puts it above every *fresh* toast of that rank. No number of
  higher-priority arrivals can hold a toast back beyond that.
- **Priority still wins in the short term.** A critical alert raised now is not
  made to wait behind a tip that has been queued for a few seconds — the aging
  is gradual on purpose, rather than a single "waited too long, go first" flag.
- **A promoted toast keeps the slot.** Its wait is never reset, so the next
  arrival cannot displace it a frame later and start the wait over; it holds
  until it expires.

The reference has no equivalent because it has no queue: a toast it has no room
for is `hide()`n straight into the notification well (`llscreenchannel.cpp:736`,
STORM-391), which is a list read at leisure. Our "N more ▸" control *is* that
well — but it holds live toasts, so their turn has to come round on its own.

### Bounds

`MAX_QUEUED_TOASTS` (64) caps the queue; `cap_queued_toasts` drops the excess as
a resolve with no button (the same path the close × takes, so the history entry,
the dedup index and persistence are torn down by the code that knows how, and
each drop is logged). Two rules decide *which*:

- The **most recently raised** go, not the lowest-ranked. A cap that dropped the
  tail of the stack order would delete precisely the starving toast the aging
  above exists to rescue.
- A toast that never expires on its own — an alert still owed an answer — is
  never dropped, however deep the queue. A queue made entirely of those is left
  alone rather than silently thinned.

`MAX_HELD_NOTIFICATIONS` (32) caps the Do Not Disturb hold list via
`DoNotDisturbQueue::hold`, discarding the **oldest** (whose context is least
likely to still exist when the user returns) with a warning naming it, and
counting the drops so the drain line says how many were lost. It is deliberately
smaller than the queue cap, so the drain — which raises everything it held in
one frame — cannot immediately overflow the queue it drains into.

### Two supporting changes

`apply_toast_overflow` was split: the visible/queued decision (no i18n) from
`update_overflow_control`, which paints the "N more ▸" label through the
`Translator`. That is what lets the invariant be driven headlessly.

The restack is still event-driven, now on `Added<Toast>` **or** a
`RestackToasts` written when a wait crosses an aging step. An unconditional
per-frame sort would have been simpler and is wrong twice: it rewrites the
channel's children every frame (the per-frame-write pattern
[[viewer-audit-ui-widget-per-frame-writes]] records) and it would undo the
manual `cycle_toasts` rotation.

### Tests

7 new, in `notification_host`:

- `should_age`'s three-way pause, and the split at the visible cap.
- `aged_rank` one step per interval and stopping at the top; `compare_toasts`
  showing priority winning fresh, still winning at a partial wait, and losing
  at a full one.
- **The invariant**, driven through the real systems: a `Low` tip against a
  `Critical` arriving *every frame* for 80 s. It asserts the tip is shown, that
  it was not shown before its rank was earned, and that the channel never held
  more than the cap. Verified as a canary: with `aged_rank` returning the bare
  priority it fails with "the queued toast was never shown", which is the filed
  bug exactly.

  It runs a **fixed frame count**, not a `while` loop bounded by a multiple of
  `QUEUE_AGING_SECS`. The first draft did the latter, and a canary that set that
  constant to infinity to disable the aging made the test loop for ever instead
  of failing — taking a coordination slot with it. A test whose termination
  depends on the constant it is testing is not a test.
- A queue of alerts past the cap loses none of them.
- The hold list stops at its bound, drops the oldest, and counts the drops.
