---
id: viewer-avatar-falls-through-ground
title: Avatar falls through the ground (suspected perf-branch ground-probe change)
topic: viewer
status: bugs
origin: observed during incremental-shadow-cull verification on aditi (2026-08-12)
refs:
  - viewer-perf-avatar-ground-probe
  - viewer-crossing-movement-locks-up
  - viewer-r23
---

Context: [context/viewer.md](../context/viewer.md).

Observed on aditi (2026-08-12): the own avatar **fell through the terrain**
(dropped below the ground), not merely standing a little low. Distinct from
[[viewer-r23]] (feet sink ~ankle-deep — a cosmetic root-Z offset, in `done/`);
this is the avatar actually passing through the land surface.

Suspected cause (per the observer): the **ground-probe raycast optimization on
the performance branch**, i.e. `78ce5ac9 perf(viewer): avian-accelerated ground
probe (opt-in) + skip seated avatars` (and possibly `41609694 perf(viewer): pose
gate — skip settled avatar/animesh skeleton evaluation`). The probe lives in
`ground.rs` (`probe_avatar_ground` / `probe`); its result feeds
`locomotion_ik`, and a *missing* ground result reads as **airborne**, which
would drop the avatar.

Narrowed down (2026-08-12):

- **Ruled out — the avian fast path**: the run did not set
  `SL_VIEWER_GROUND_PROBE_SPATIAL`, so `spatial` was `None` and `probe()` ran
  the `MeshRayCast` path **exactly as before `78ce5ac9`** (that commit only adds
  a branch taken when `spatial` is `Some`).
- **Ruled out — skip-seated**: the observer has not sat down in a dozen-plus
  test runs, so `is_seated` is false and the probe is not skipped. So
  `78ce5ac9`'s two active-by-default changes both fail to apply here — it is
  very likely **not** the ground-probe commit.

**Leading suspect — the pose gate (`41609694`).** Despite its "skip settled
skeleton evaluation" title, that commit also modified
**`ground.rs`, `locomotion_ik.rs`, and `physics.rs`** — it reaches into the
ground / locomotion / physics path, not just skeleton posing. If its "settled"
gating skips the ground-snap / physics for an avatar that should still be
resolving its height (a walk onto lower terrain, a crossing, or a wrongly
detected "settled" while moving), the avatar can drop through or float — closely
related
to [[viewer-crossing-movement-locks-up]] (movement lockup after a crossing onto
lower terrain). Read the `physics.rs` / `locomotion_ik.rs` changes in that
commit for where "settled" bypasses ground resolution.

**Secondary — latent nearest-hit risk** (pre-perf-branch, from `3371f312`, so a
long shot for a *regression* but worth ruling out): the mesh path uses
`.always_early_exit()` then
`.iter().find(|hit| hit.distance <= PROBE_ABOVE + PROBE_BELOW)`.
`always_early_exit` returns the *first* hit found, not necessarily the nearest;
a farther surface traversed first can exceed the threshold and `find` returns
`None` → airborne. Confirm whether early-exit yields the nearest hit or a
traversal-order hit.

Details still needed (observer): was the avatar standing, walking, or crossing a
region boundary? Did it recover (pop back up) or stay fallen? Was the terrain
fully rezzed? Near a region edge?

The `ON_LAND_BAND` "standing on land → skip the object `MeshRayCast`"
optimization the observer recalled is real, but it lives inside the spatial
fast-path (gated on `SL_VIEWER_GROUND_PROBE_SPATIAL`, which is unset), so it was
not active — not the cause here.

Investigation — cheap A/B first: the pose gate ships a kill-switch,
**`SL_VIEWER_POSE_GATE=0`** (`animations.rs:1066`). Reproduce with it set; if
the fall-through stops, the pose gate is confirmed and the fix is in its
`locomotion_ik` settle/wake logic (a settled fold that skips re-applying ground,
or a probe `None` read as `fall`). The gate also has meters —
`SL_VIEWER_LOG_POSE_GATE` — for wake-reason tallies. If it still falls with the
gate off, instrument `probe()` to log when it returns `None` for a standing
avatar, and revisit the `always_early_exit` nearest-hit path. Because it is
intermittent, run several times each way.
