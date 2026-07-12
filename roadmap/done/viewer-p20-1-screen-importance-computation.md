---
id: viewer-p20-1
title: Screen-importance computation
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 20 — On-screen render priority
---

Context: [context/viewer.md](../context/viewer.md).

Everything is fetched at max fidelity in FIFO order today (textures at
`DiscardLevel::FULL`, meshes at `MeshLod::FINEST`), yet the schedulers already
support per-request priority (`sl-asset-sched` `Priority` +
`popularity_boost`, `TextureStore` / `MeshStore` `request(…, priority)` +
`.set_priority()`). This phase computes on-screen importance and feeds it, so
what the camera looks at loads first.

**P20.1. Screen-importance computation.** A Bevy-free helper computing
an object / face's approximate screen pixel area from its world bounding
radius, camera distance, viewport height, and vertical FOV. Port the
reference viewer's `LLFace::getPixelArea` / `LLPipeline::calcPixelArea` /
`LLVOVolume::getPixelArea`. **Done:** a new `screen` module in
`sl-asset-sched` (the domain-free scheduling crate, so it sits next to the
`Priority` P20.2 will map it onto) exposing `ScreenMetrics` — a per-frame
`pixels_per_radian` factor (`window_height / vertical_fov`, the reference
`LLDrawable::sCurPixelAngle`) built once and reused for every object, with
`pixel_area(bounding_radius, camera_distance)` returning
`(atan(radius/dist) * pixels_per_radian)² * π` (`LLPipeline::calcPixelArea`),
including the near-object distance ramp (`dist < 16 m → (dist/16)²·16`).
Guards a zero/degenerate FOV → 0 and a zero distance → the `pi/2` half-angle
(matching `atan(+inf)`) instead of dividing by zero. Unit-tested; re-exported
at the crate root.
