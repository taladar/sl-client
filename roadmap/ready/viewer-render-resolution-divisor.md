---
id: viewer-render-resolution-divisor
title: Render at a reduced resolution (RenderResolutionDivisor)
topic: viewer
status: ready
origin: gap found wiring viewer-rlv-debug-settings-commands (2026-09-07)
refs: [viewer-rlv-debug-settings-commands, viewer-rlv-vision-render]
---

Context: [context/viewer.md](../context/viewer.md).

Render the 3D scene at `1/n` of the window's resolution and upscale it to fill
the view — the reference viewer's `RenderResolutionDivisor` debug setting.

The viewer has no reduced-resolution path at all today: every camera renders
straight to the window's swapchain image at its full size. What is wanted is
the ordinary offscreen-then-blit shape — the world camera renders to an image
of `size / n`, and that image is drawn to the window — with the UI drawn at
full resolution on top, because the point is a coarser *world*, not a coarser
interface.

Two callers want it, for opposite reasons:

- **performance.** It is the bluntest quality lever there is, and the one a
  user reaches for when a region is too heavy for their machine. It belongs in
  the graphics preferences beside the other `Render*` settings.
- **RLV vision impairment.** `@setdebug_renderresolutiondivisor:<n>=force` is
  the oldest and cheapest blur a collar can impose
  ([[viewer-rlv-debug-settings-commands]] wired the command up and left it
  answering nothing, precisely because there is no setting behind it that
  anything looks at). Landing this makes `ViewerRlvExt::debug_value` /
  `set_debug_value` able to answer that row, and turns the `@setdebug=n`
  settings-editor gate — already wired and unit-tested — from a rule over an
  empty set into one that hides a real row.

Scope: the render path, a `RenderResolutionDivisor` setting registered with the
rest of the render family, the graphics-preferences control for it, and the
two `ViewerRlvExt` arms that then have something to read and write. Note the
reference's rule that a value a script wrote is **never** persisted to disk
(its `DBG_PERSIST` flag) — with a real writable row that rule finally has
something to protect, and belongs at the write site.

The related but distinct RLVa effects — the `@setsphere` blur / darken /
chromatic system and `@setoverlay` — are [[viewer-rlv-vision-render]]; this is
the plain resolution lever, which is a graphics feature that RLV merely
happens to be able to drive.
