---
id: viewer-flexi-resettle-after-snapshot
title: Flexi prims re-settle after taking a snapshot
topic: viewer
status: done
origin: observed while testing viewer-snapshot-floater (2026-07)
refs: [viewer-snapshot-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Taking a snapshot (Refresh / Save in the snapshot floater
([[viewer-snapshot-floater]]), or the ``Ctrl+` `` quick key) makes nearby
**flexible prims** visibly **re-settle** — their flexi simulation swings back
out from rest and damps down again, as if it had been reset or handed one huge
time step.

Likely cause (to confirm): the capture is a several-frame stall — the UI is
hidden, a frame is skipped, and the window is **read back off the GPU**
(`Screenshot::primary_window`), which is not cheap. The next `Update` then
arrives with a large `Time::delta`, and the flexi integrator (`flexi.rs`, Phase
32) integrates that one big step as a spring impulse, so the chain overshoots
and settles again. A reset of the flexi state during the capture is the
alternative to rule out.

Investigation / fix directions:

- Confirm the trigger is the frame-time spike: log `Time::delta` around a
  capture and correlate with the swing.
- If so, **clamp the flexi integrator's per-step `dt`** (a max-substep, which a
  stable spring sim should have anyway), or skip flexi integration for the
  frame(s) the capture stalls, so a hitch never turns into a visible re-settle.
  A general dt clamp also protects flexi against any other frame spike
  (asset-decode bursts, alt-tab), not just snapshots.

Reference (Firestorm, read-only): `LLVOVolume::doFlexibleUpdate` /
`LLVolumeImplFlexible`.

## Resolution

Confirmed exactly as suspected: the in-world flexi prim sits perfectly still
for minutes, then after a snapshot swings back and forth a few times and
re-settles at the *same* rest pose — an off-equilibrium impulse, not a state
reset. The rest pose is a **per-`dt` equilibrium** (gravity / user force scale
linearly with the step, tension saturates), so a single big step after the
capture stall pushes the chain off it and it oscillates back.

Two fixes, both landed:

1. **Fixed-timestep accumulator (the robust, general fix).** The first cut
   sub-stepped a big frame into equal passes; that removed the large overshoot
   but left a *smaller* residual swing (confirmed live), because the flexi's
   rest pose is a per-**step-size** equilibrium and a sub-step size that tracked
   the frame drifted the equilibrium between the settled frame rate and the
   sub-step size. The final fix integrates the chain at a **constant**
   `FIXED_TIMESTEP` (60 FPS) via an accumulator in `FlexiChain`
   (`sl-prim/src/flexi.rs`): `step` banks the frame's `dt` and drains whole
   fixed steps, carrying the remainder. The equilibrium is then pinned at every
   frame rate and across any stall — a settled chain sees only more
   equilibrium-sized steps and stays put
   (`a_settled_chain_survives_a_frame_spike`), and a hitch integrates
   identically to the normal frames it replaced
   (`a_split_frame_equals_one_whole_frame`). This also makes flexi frame-rate
   independent and robust to any spike (asset-decode bursts, alt-tab), not just
   snapshots. Note the reference integrates the raw variable per-frame step (it
   just never feeds flexi a big delta); the accumulator is a deliberate
   improvement over that.
2. **Off-thread snapshot save (the root-cause hitch).** The snapshot's
   full-resolution PNG/JPEG encode + disk write ran synchronously in
   `process_shot` on the frame thread — a several-hundred-millisecond stall
   that spiked the next `Time::delta`. It now runs on Bevy's `IoTaskPool`
   (`spawn_save_task`), drained by `poll_snapshot_saves`, which removes the
   hitch itself (also sparing particles / physics / animation the same spike).
