---
id: viewer-world-text-in-the-overlay-pass
title: Name tags and hover text belong in the overlay pass, not the world pass
topic: viewer
status: ready
origin: live check of viewer-render-resolution-divisor (2026-09-20)
refs: [viewer-render-resolution-divisor, viewer-name-tags-billboard-render, viewer-hover-text, viewer-360-snapshot]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

With [[viewer-render-resolution-divisor]] set to Half, avatar name plates come
out **soft along with the world**. Their apparent size is right — the billboard
shader derives metres-per-pixel from `view.viewport.w`, so a tag keeps the same
fraction of the frame at any divisor — but the glyphs are rasterised at their
full pixel size and then drawn into a `1/n` target, so what reaches the eye is a
magnified copy. `llSetText` hover text over prims rides the same renderer and
has the same problem.

That is a **parity gap the divisor merely exposed**, not something the divisor
introduced. The reference draws neither in its world pass:
`LLHUDObject::renderAll()` runs from `render_ui()` (`llviewerdisplay.cpp`)
*after* `gPipeline.renderFinalize()` — i.e. after the screen target has been
scaled back up — straight into the default framebuffer. That is also why
reference name tags are occluded by nothing (see the
`sl-client-hud-text-is-never-occluded` note: the one call that would put the
scene's depth in that framebuffer is commented out).

So the reference's world-anchored text is **always** full-resolution overlay
text, at every setting; ours is world geometry that happens to look right while
the world happens to be full-resolution.

A second consumer has since arrived: the 360° panorama ([[viewer-360-snapshot]])
photographs the world camera's six faces, so name tags and hover text land in
every one of them — floating captions inside an immersive photo. That capture
needs no toggle of its own; moving this text to the overlay pass takes it out of
a panorama for free.

## Scope

Both consumers already share one seam —
`sl_viewer_world_objects::name_tag_billboard::tag_render_layers()`, which
`hover_text` imports — so *what* moves is one function's return value. The work
is in giving it somewhere to move **to**:

- a render layer of its own, and an overlay camera that draws only that layer,
  mirroring the world camera's transform and projection the way
  `sl-viewer-edit`'s gizmo camera already does (`ChildOf(camera)`,
  `ClearColorConfig::None`, `Msaa::Sample4`, `Hdr`, output blend `REPLACE`);
- **an order slot**, which is the awkward part. The composited window frame is
  a fixed ladder — world 0, gizmos 1, HUD + UI 2 — and world text wants to be
  above the world and below the interface. Inserting it renumbers cameras
  spawned by three different crates, and `OverlayCamera` (in
  `sl-viewer-world-api`) is the enum the capture harness routes each layer by,
  so a fourth variant is part of the change rather than an afterthought;
- the divisor's upscale pass targets "the lowest-ordered `OverlayCamera` still
  on the window", so it follows the renumbering for free — but a test of that
  choice exists and will need the new ladder;
- occlusion is a **decision, not a detail**: an overlay camera clears its own
  depth, so text would stop being hidden by world geometry. That happens to be
  the reference's behaviour (readable through a wall), so this is a chance to
  land parity rather than a regression — but it should be landed deliberately
  and looked at, because it also means text in front of a wall no longer sorts
  against anything.

Fiddly enough to want its own live round: the tag renderer's known traps are
per-font-size atlas pages carried as `MeshTag`'d children, the anti-overlap
screen offset packed into that same `MeshTag`, the glow-mask alpha on a
transparent overlay, and the draw-order fight with the water surface that
`viewer-nametags-refracted-by-distant-water` already cost a round trip.
