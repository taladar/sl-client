---
id: viewer-p35-3
title: HUD picking and clicking
topic: viewer
status: ready
origin: found while doing viewer-p35-2 (rendering a HUD you cannot click is half a HUD)
refs: [viewer-p35-1, viewer-p35-2]
---

Context: [context/viewer.md](../context/viewer.md).

**P35.3. Make a rendered HUD clickable**: pick the HUD face under the mouse
through the HUD camera, and touch it on the sim. [[viewer-p35-2]] draws a HUD
but nothing can be *used* — and a HUD exists to be clicked, so this is the half
of Phase 35 that makes the feature real.

Shape of the work:

- **A second pick path, through the HUD camera.** The existing crosshair pick
  (`objects::pick_object`, the `P` key) casts a ray from the *fly* camera
  through the world scene; a HUD lives in the HUD camera's orthographic space,
  on `HUD_RENDER_LAYER`, where a world ray never goes. The reference viewer runs
  the same split: `LLViewerWindow::cursorIntersect` re-runs its intersection
  with the HUD matrices (`setup_hud_matrices(screen_region)`, over the small
  screen region around the cursor) and records `mPickHUD` when the hit object
  `isHUDAttachment()`.
- **HUD before world.** The reference picks the HUD *first* and only falls
  through to the world when nothing HUD-ward is hit — a HUD covering half the
  screen must not let clicks through to the ground behind it. An orthographic
  pick is a ray parallel to the HUD depth axis through the cursor's HUD-space
  `(y, z)`, not a perspective ray from an eye point.
- **Touch on the wire.** A click must reach the object: `ObjectGrab` /
  `ObjectGrabUpdate` / `ObjectDeGrab` (`touch_object` / `grab_object` /
  `degrab_object` already exist as `sl-repl` commands, so the protocol side is
  done) — with the surface info the sim needs for `llDetectedTouchST` /
  `llDetectedTouchUV` / `llDetectedTouchFace`: the picked face index, its UV and
  ST coordinates, the intersection position, and the normal / binormal. That is
  the reference's `LLPickInfo::getSurfaceInfo`, and it is the part the current
  pick tool does not compute.
- **A cursor to click with.** The viewer grabs the cursor for the fly camera, so
  there is no free pointer today. Needs a mode (a held key, or a toggle) that
  releases the cursor and turns clicks into HUD touches — the smallest thing
  that makes this testable, not a whole UI.

Verify live: put a **scripted** touch-responder on a HUD point (the OpenSim
scripted-object recipe — `llSay` on `touch_start`), click it, and see the chat
come back. That is an end-to-end check of pick → grab → sim → script, which a
screenshot cannot give.

Related, and deliberately *not* in scope here (each its own task if wanted): HUD
**zoom** (`mHUDCurZoom` / `HUDScaleFactor`, Ctrl+0/8/9) and HUD **particles**
(`RENDER_TYPE_HUD_PARTICLES` — a particle source on a HUD emits into HUD space,
`LL_PART_HUD`, a separate `LLVOHUDPartGroup` partition whose camera sits at
`(-1, 0, 0)`; off by default behind `RenderHUDParticles`).
