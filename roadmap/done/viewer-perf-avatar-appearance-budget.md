---
id: viewer-perf-avatar-appearance-budget
title: Budget + debounce avatar appearance application
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: [viewer-perf-avatar-bake-apply-spikes]
---

Context: [context/viewer.md](../context/viewer.md).

`apply_avatar_appearance` resolved **every** avatar in `appearance_dirty` in
one frame: visual-param resolution, morph weights, skeletal + volume
deformations, then a full CPU re-morph and mesh re-upload of every base body
part. Logging into (or teleporting into) a crowded region marked the whole
crowd dirty at once — the worst remaining single-frame hitch after
[[viewer-perf-avatar-bake-apply-spikes]].

Two composing fixes:

- **Per-frame budget.** `AppearanceApplyBudget` (default 2 avatars/frame,
  env `SL_VIEWER_APPEARANCE_APPLY_BUDGET`) caps how many avatars resolve per
  frame, own avatar first so our own body never queues behind a crowd.
  Deferral is safe: a later pass re-reads the newest cached appearance
  vector, so a deferred avatar never applies stale data.
- **Re-dirty debounce.** An avatar's triggers land frames apart (appearance
  event → body parts spawn → masked bake decodes), and each used to force a
  full re-resolve + re-mesh. The dirty set now folds into a stamped pending
  ledger (`appearance_pending`): a never-shaped avatar (no recorded
  deformations) resolves immediately — first visibility wins — while a
  re-marked one waits 0.3 s of mark quiet, capped at a 1 s max wait so a
  steady re-mark stream (live appearance edit) still progresses. The cascade
  coalesces into ~2 rebuilds instead of one per trigger.

The pose gate is only bumped when avatars are actually processed, so pure
deferral frames no longer wake the skeleton re-pose. Verify with Tracy
(per-event max of `apply_avatar_appearance` during a crowd login) and the
`SL_VIEWER_LOG_POSE_GATE` re-apply meter, A/B via the budget env var.
