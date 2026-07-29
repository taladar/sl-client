---
id: viewer-flexi-resettle-after-snapshot
title: Flexi prims re-settle after taking a snapshot
topic: viewer
status: bugs
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
`LLVolumeImplFlexible` (its own fixed-timestep accumulation, which is why the
reference does not show this).
