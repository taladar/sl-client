---
id: viewer-clouds-horizon-waterline-contact
title: Check clouds vs the waterline at the horizon against Firestorm
topic: viewer
status: bugs
origin: split from viewer-clouds-sun-occlusion-horizon-contact (2026-08-03)
refs: [viewer-clouds-sun-occlusion-horizon-contact]
---

Context: [context/viewer.md](../context/viewer.md).

Split out of [[viewer-clouds-sun-occlusion-horizon-contact]] (the "clouds
touch the water in the distance" half). The other half of that ticket
turned out to be a sky colour/HDR problem, not a cloud-geometry one — see
that task for the `srgb_to_linear` fix and the bloom follow-up.

**Status: could not reproduce this session.** On a fresh look (local
OpenSim + aditi, 2026-08-03) the clouds **faded out above the waterline**
rather than touching it — the *opposite* of the original report. The cloud
dome geometry, `cam_height` (`0.96 × 15000 = 14400`), and the
`altitude_blend_factor` horizon fade were all re-verified faithful to the
reference (`buildStripsBuffer` + `renderDome` + `getCamHeight`,
`cloudsF.glsl`): clouds fade to zero as `rel_pos.y → 0` (the horizon) and
`SL-11589` clamps them off below it, so a faithful port should not droop
clouds onto the water.

**What this task is:** confirm the correct behaviour against Firestorm on
the *same* sky and decide whether anything is actually wrong:

- If Firestorm also fades clouds out above the waterline → close this as
  faithful (our current behaviour already matches).
- If Firestorm's clouds visibly reach the waterline → our fade clamps too
  high; candidates are the `altitude_blend_factor` ramp
  (`(rel_pos.y + 512) / max_y`), the dome cap's lower rim angle at our
  camera heights, or fog-over-water hiding the reference's rim where ours
  shows it.

Now easy to reproduce: `SL_VIEWER_SKY_DAY_POSITION` moves the sun and
`--camera-position` / `--camera-look-at` aim correctly (both fixed
2026-08-03), so frame a low sun over open water (aim west/`−X` at sunset)
and A/B against Firestorm on the same frame before changing the port.
