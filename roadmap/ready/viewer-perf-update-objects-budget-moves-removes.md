---
id: viewer-perf-update-objects-budget-moves-removes
title: update_objects 124 ms spike — identify cause, then budget moves/removes
topic: viewer
status: ready
origin: async shadow-cull frame decomposition on aditi (2026-08-11)
refs: [viewer-perf-frame-churn-cleanups, viewer-perf-pbr-shadow-cluster-rez]
---

Context: [context/viewer.md](../context/viewer.md).

`update_objects` (`objects.rs`) is cheap on average (mean 0.17 ms) but **spiked
to 124 ms** on a single steady-state frame on aditi — the largest single-frame
hitch in the trace and the driver of `Update`'s 134 ms `max`.

## Cause is NOT yet identified — measure first

The obvious structural gap: the per-frame [`SpawnBudget`] (default 16) only
counts **geometry builds** (`apply_object` returning `true`).
**Moves and removes are free** — the drain loop only decrements budget on a
build, so a flood of moves/removes runs inline for the whole batch in one frame.

But no bulk trigger was observed. The session log shows **zero**
`DisableSimulator` / `EnableSimulator` / `forget_sim_objects` / `ObjectRemoved`
/ circuit timeout / re-establish events, so the initial "mass-remove" hypothesis
is **unsupported**. And the scene was
**parked and static — no large-scale object moves either**. So on that frame
`update_objects` had essentially nothing to process (consistent with its 0.17 ms
mean), which means the 124 ms is very likely
**not real object-processing work at all**: a one-off transient — a thread stall
/ scheduler artifact (the Tracy zone capturing descheduled time), a large
allocation / hashmap rehash, or lock contention on one of its seven `ResMut`
resources. A genuine mass-remove would still only be expected on a real
`DisableSimulator`, or on a **circuit flap** (lost + immediately
re-established), which would `forget_sim_objects` then re-populate — and if
*that* ever shows up, the bug is the flap, not the drain.

**Deprioritized.** A single, unexplained, likely-transient 124 ms frame is not
worth chasing ahead of recurring costs like
[[viewer-perf-parcel-borders-rebuild-spread]] (44 ms every rebuild). Do not
implement the budgeting speculatively.

So, before any budgeting work, **instrument `update_objects`** to record, per
frame, how many spawns / position-only moves / removes it processed, and log
(once) any frame that exceeds a threshold (e.g. > 20 ms or > 200 events) with
the breakdown — the same measure-don't-guess approach the shadow cull used. Run
on aditi and catch the spike. Only then decide:

- **If it is a move burst** (bulk `ObjectUpdate`): budget moves too — cap total
  per-frame drain work, carry overflow in the existing `PendingObjectEvents`
  FIFO. A move applied a frame or two late is invisible.
- **If it is a mass-remove without a legitimate `DisableSimulator`**: find why —
  a circuit flap (lost + re-established), a redundant kill list, or a
  region-handoff path that reaps + re-adds. Fix the spurious churn at the
  source.
- **If it is a single expensive `apply_object`** (one huge object
  re-tessellating inline): that is a different lever (defer / budget
  re-tessellation).

Budgeting removes is still reasonable **defence-in-depth** for the genuine
`DisableSimulator` case (a retiring neighbour's objects are not urgent and can
drain over a few frames) — but do it *after* confirming it is the real cause of
this spike, not instead of.

Acceptance: the worst-frame breakdown is captured (what the 124 ms frame
actually did); the identified root cause is fixed (spread, or de-flap); no
>50 ms `update_objects` frame under the reproduced trigger (Tracy
`-f update_objects`); objects still clear/settle within a bounded number of
frames.
