---
id: viewer-crowd-harness-copies-render-grey
title: Crowd harness copies render grey (snapshot the template before its bake)
topic: viewer
status: ready
origin: GPU-avatar Phase 5 crowd harness, OpenSim run (2026-08-14)
refs: [viewer-perf-gpu-avatar-phase5-lod-polish]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md); code
`sl-viewer-world-avatar/src/gpu_avatars/crowd.rs`.

`SL_VIEWER_CROWD=N` copies can render **grey / untextured** while the local
avatar is textured (a center-grid copy overlapping the avatar makes it "flicker"
grey/textured). Observed on **OpenSim** (client-bake path, P15.3 client-side
composite — not the aditi server bake). The harness settle-detection waits for
the **visible-submesh set** to stabilize, but the mesh becomes visible
**before** the client-side bake composite finishes applying the
material/texture, so the copies snapshot the template's material pre-bake →
grey. (Cosmetic only — it does **not** affect the frustum-culling verification,
which is bounds / `ViewVisibility`, independent of materials.)

## Since filed (audit 2026-10-15)

The settle detection described above is gone: commit 74d2f78f (2026-08-15)
replaced it with a **manual trigger** — while a crowd is armed a "Spawn crowd"
button on the bottom toolbar shows the live visible-part count, and nothing is
captured until the user clicks it (`sl-viewer-world-avatar/src/gpu_avatars/
crowd.rs`, module docs). That is the first fix option below done by hand: a
user who waits until the avatar looks complete captures the baked material.
What remains is the second option — copies still hold the handles captured at
the click, so a bake that lands (or re-lands) **after** the capture leaves them
grey — and the verification run below, which has not been done.

## Fix options

- Extend the settle condition to also wait for the **bake/material** to be
  applied (key off the client-bake completion signal, not just visibility),
  then snapshot.
- Or have copies **share the live-updating material handle** so a later bake
  update follows onto the copies instead of being a dead snapshot (verify
  whether the bake updates the material in place or swaps the handle — the grey
  persisting on copies while the original goes textured suggests a swap the
  copies don't follow).

## Verify

`CROWD=100` on OpenSim (client bake) and aditi (server bake): copies are
textured like the local avatar once its bake lands; no grey flicker on the
original from an overlapping copy.
