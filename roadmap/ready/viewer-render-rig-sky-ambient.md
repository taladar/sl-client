---
id: viewer-render-rig-sky-ambient
title: Light that follows the sky reaches faces turned away from the sun
topic: viewer
status: ready
origin: viewer-render-context-matrix (2026-11-08) — the time-of-day plate
points: 3
refs: [viewer-render-context-matrix, viewer-audit-probe-ambient-multiply]
---

Context: [context/testing.md](../context/testing.md).

Measured in the context matrix's time-of-day sweep: a matte white plate
whose face is turned to the camera (vertical, with the sun overhead) read
luminance **0.149 at midday and 0.149 at midnight**, identical to three
places, with the real sky stack staged at each anchor. Only its sunlit top
face changed. The sweep now reads a plate tilted halfway to the zenith, so
the ordering check measures the direct sun and moon and says nothing about
ambient.

In the rig, `GlobalAmbientLight` is pinned to `0.0`
(`render_readback.rs`), since the viewer gets its ambient from the probes
(`probes::probe_ambient_scale`, `0.0` by default). Whatever lights the
shaded face is constant, so no probe there is capturing the staged sky.

- First find out whether the **viewer** has it too. In the live viewer at
  midday and at midnight (an EEP override, or the time-of-day presets), a
  shaded vertical face must be visibly darker at midnight. If it is not,
  this is a viewer bug, not a rig gap.
- In the rig: make the default probe capture the staged sky (or whatever
  the viewer's path is), so a cell's ambient follows its time of day.
- Then turn the plate in `render_stage.rs::stage` back to face the camera
  and add an ordering on a shaded face (midday brighter than midnight) next
  to the sunlit one.
