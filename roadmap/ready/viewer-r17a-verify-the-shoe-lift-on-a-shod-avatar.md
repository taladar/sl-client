---
id: viewer-r17a
title: Verify the shoe lift live on a shod avatar
topic: viewer
status: ready
origin: found while doing viewer-p34-4 (a shoe question — does the collision-volume
  inheritance carry it?)
refs: [viewer-r17, viewer-p13-4, viewer-p34-4]
---

Context: [context/viewer.md](../context/viewer.md).

**R17a. Close R17's open loop: confirm the shoe lift *visually*, on an avatar
that actually wears shoes.**

[[viewer-r17]] plants a shod avatar taller so its feet rest on the ground rather
than sinking into it: a per-agent `pelvis_lift` of
`-offset(mFootLeft).z * (1 + scale(mAnkleLeft).z)`, clamped at zero, added to
the pelvis rest height (the reference's `computeBodySize` / `mPelvisToFoot`).
Its write-up is explicit that this was **unit-tested only**
(`shoe_offset_lifts_the_body`) and never seen — "not visually confirmed against
a shod avatar this session (the default own avatar wears no shoes and no second
avatar was in view)". So a `done/` file carries an unobserved claim about
avatar placement, which is exactly the kind of thing that is quietly wrong for a
year.

That excuse has expired: the aditi `primary` avatar now wears **heels**, and in
the [[viewer-p34-4]] session it stood planted rather than sunk — consistent with
the lift working, but never A/B'd, and "looks about right" is not a check. The
lift is a few centimetres, so it needs a deliberate comparison, not a glance.

Shape of the check:

- The two shoe params (`Shoe_Heels` id 197, `Shoe_Platform` id 502, both driven
  by the transmitted `Heel Height` id 198) offset `mFootLeft` / `mFootRight`
  downward in Z by up to `-0.08` / `-0.07`. Resolve the agent's appearance and
  **log the shoe lift it actually computes** — a zero lift on an avatar in heels
  is the failure this task exists to catch, and it would be invisible in-world.
- A/B it in one session, the way [[viewer-p34-4]] did the volume displacement:
  the honest comparison is the same avatar in the same frame sequence with the
  lift forced to zero and back, not two logins. There is no toggle for it yet —
  a debug gain / key in the spirit of `SL_VIEWER_VOLUME_MORPH_GAIN` and the `V`
  key would be the way, and is worth keeping afterwards.
- Watch the **feet against the ground plane**, not the head: the whole point is
  the plant. Note that a *fitted mesh* foot rigged to the `L_FOOT` / `R_FOOT`
  collision volumes does **not** follow the `mFoot*` offset (those volumes hang
  off `mAnkle*`, and [[viewer-p34-4]] inherits only *scale*, which the shoe
  params leave at zero) — that is reference-faithful, not a bug, but it means
  the shoe's own visible geometry and the body's foot can move differently, so
  do not read a discrepancy there as a failure of the lift.
- Flat ground only. A slope would confound the plant height with the terrain
  probe.

**Update (2026-07-23, [[viewer-r23]]):** the `pelvis_lift` mechanism this
task describes is gone — the shoe's foot offset now folds into the
`computeBodySize` port (`BevySkeleton::body_size_metrics`), as in the
reference: both `pelvis_to_foot` and `body_size_z` grow by the lift, so the
root rises by **half** the lift (the old additive term was full-lift and in
the wrong direction — subtracted, i.e. lowering). The check itself stands,
now on the R23 plant: log the resolved metrics for a shod avatar (a zero
foot-offset delta on an avatar in heels is the failure to catch) and A/B the
plant against Firestorm.
