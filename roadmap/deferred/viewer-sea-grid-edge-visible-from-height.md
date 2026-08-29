---
id: viewer-sea-grid-edge-visible-from-height
title: The sea ends in a 256 m staircase when seen from high above the water
topic: viewer
status: deferred
origin: split from viewer-sea-distance-band-hard-seam (2026-08-29)
refs: [viewer-sea-distance-band-hard-seam]
---

Context: [context/viewer.md](../context/viewer.md).

The sea is a grid of 256 m squares — one per region cell within
`SEA_GRID_RADIUS_CELLS` (17) of the camera
(`sl-viewer-world-scene/src/water.rs`) — so it stops about 4.4 km out. From an
ordinary viewpoint that edge sits a fraction of a degree below the horizon and
nothing shows. From a camera a kilometre above the water it is ~13° below the
horizon, and the ocean visibly **ends** there, in a staircase of 256 m cell
boundaries with sky beyond it.

Reproduced on the local OpenSim with the screenshot harness, from `North Region`
looking west over the void:

```sh
SL_VIEWER_SKY_DAY_POSITION=0.35 ./target/release/sl-client-bevy-viewer \
  --credentials credentials.toml --grid localhost --avatar primary \
  --camera-position 30,128,1020 --camera-look-at -3000,128,1020 \
  --screenshot-dir <dir>
```

This is not new — the grid has always ended there — but it used to be hidden.
Until [[viewer-sea-distance-band-hard-seam]] was fixed, the whole outer sea was
*unfogged* and so looked exactly like the sky it was refracting, which made the
sea appear to end far nearer, at the fog seam, and hid its real edge behind that
much larger artifact. With the fog now reaching the far clip, the real edge is
what the eye lands on.

Two shapes overlap at that edge, both worth separating before changing anything:

- the **fog's** reach is radial (the camera's far clip, 4096 m), a smooth arc;
- the **sea grid's** reach is Chebyshev (a square of cells), 4382 m along the
  view axis and up to 6197 m into a corner, quantised to 256 m.

So there is a sliver of drawn-but-unfogged sea between the arc and the
staircase, which reads as sky. Whichever way this is fixed, those two extents
want to agree: candidates are a radial (disc) sea grid, a larger grid, or an
"endless" far skirt whose vertices run out to the horizon the way the
reference's edge-water patches do. Check what Firestorm shows from the same
altitude first — a finite ocean edge may simply be what a viewer looks like from
1 km up.

**Deferred, not open:** the user is not worried about seeing the edge of the
world / sea from an extremely high camera (2026-08-29). Recorded so the
geometry and the repro are not lost, but nothing is chasing it.
