---
id: viewer-perf-probe-capture-content
title: Cheaper probe captures — layer exclusions (honour DYNAMIC), draw distance
topic: viewer
status: ready
origin: reflection-probe performance planning round (2026-07-22), Firestorm LLReflectionMapManager survey
refs: [viewer-perf-probe-instrumentation, viewer-p33-2, viewer-perf-probe-capture-shadows]
---

Context: [context/viewer.md](../context/viewer.md).

A probe capture camera currently renders the **full world layer** —
avatars, particles, terrain, water, every prim out to `far = 4096` — for
each face. The reference cuts its capture passes hard (`gCubeSnapshot`):
draw distance 64 m (`RenderReflectionProbeDrawDistance`), avatars and
particles excluded unless the probe is DYNAMIC, HUD off, SSAO/SSR off.
Two changes here; shadow reduction is split out to
[[viewer-perf-probe-capture-shadows]] because it likely needs upstream
work and must not stall this in-tree win.

**(a) `RenderLayers` exclusions — make the SL DYNAMIC flag real.** Put
avatars and particle effects on dedicated render layers (the
`HierarchyPropagatePlugin::<RenderLayers>` machinery in `lib.rs` already
propagates layers down hierarchies; the HUD layer is the existing
exclusion precedent). Non-DYNAMIC local rigs' cameras exclude those
layers; the default probe and DYNAMIC-flagged probes keep them — the
reference's behaviour. We parse the flag from extra-param `0x90` but
treat it as always-on today (`probes.rs` module doc), so this is protocol
fidelity as well as perf. Bonus: per-view `RenderLayers` also shrinks the
shadow-caster set Bevy considers for that view.

**(b) Probe draw distance.** Drop the capture cameras' far clip from
4096 toward a `PROBE_DRAW_DISTANCE` constant (~64 m, beside
`CAPTURE_PERIOD_FRAMES`). Sky interplay to resolve in-task: the sky dome
is 3000 m and camera-centred, but its shader forces depth to the far
plane (`sky.rs`), so it plausibly survives a short far clip — verify;
the sun/moon discs keep real depth (~2000 m) and would vanish from
captures; and frustum culling of the dome mesh AABB from a capture
camera far off the dome's centre needs checking. Documented fallbacks if
the far-plane route fails: capture-camera-specific distance culling in a
`PostUpdate` visibility system, or keep `far` and let (a) plus the
scheduler carry the savings.

Acceptance: measurable per-face capture-time drop in an avatar-dense
scene ([[viewer-perf-probe-instrumentation]] baseline); in a
screenshot-harness scene with an avatar beside a non-DYNAMIC probe prim,
the mirror test sphere shows no avatar — flag the prim DYNAMIC and it
appears; sky present in every capture (gallery golden of a reflective
water scene).
