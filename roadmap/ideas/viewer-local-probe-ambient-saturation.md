---
id: viewer-local-probe-ambient-saturation
title: A face inside a placed probe takes a more saturated ambient than Firestorm gives it
topic: viewer
status: ideas
origin: viewer-sky-irradiance-ignores-local-probes (2026-11-15)
refs: [viewer-sky-irradiance-ignores-local-probes]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

At `sl-crosscheck --scenario catalogue-eep --look-at probe-room`, the shaded
top face of the box inside the red room reads 32, 6, 9 in our viewer and 41,
29, 20 in Firestorm (mean sRGB). Both take the ambient from the room's probe
rather than the open sky, but ours is almost pure red. Firestorm's keeps much
of the box's own warm colour.

## Leads

- What each probe captured: ours may see the walls lit differently, or miss
  the sky and ground through the open south side.
- The reference's `sampleReflectionProbes` blends the local probe with the
  sky ambient over the probe's influence and `ambiance`. We do not model a
  probe's own `ambiance`; see `sl-viewer-world-scene/src/probes.rs`.
- The irradiance filtering of the captured cube against the reference's.
