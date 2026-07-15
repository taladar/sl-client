---
id: viewer-p35-3
title: HUD picking and clicking
topic: viewer
status: done
origin: found while doing viewer-p35-2 (rendering a HUD you cannot click is half a HUD)
refs: [viewer-p35-1, viewer-p35-2]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** A rendered HUD ([[viewer-p35-2]]) can now be **touched**: a click on a
worn HUD face is picked through the HUD camera and sent to the sim as a touch
carrying the surface it struck, so a scripted HUD's `touch_start` fires.

Shape of what landed:

- **A surface block on the wire.** `touch_object` / `grab_object` /
  `grab_object_update` / `degrab_object` (and their `ObjectGrab` /
  `ObjectGrabUpdate` / `ObjectDeGrab` messages) used to send an empty
  `SurfaceInfo` list. They now take an `Option<&SurfaceInfo>` — a new `sl-proto`
  value type (face index, texture `UV` and surface `ST` coords, intersection
  position / normal / binormal) — so a renderer that picked the object can give
  a script what `llDetectedTouchFace` / `llDetectedTouchST` /
  `llDetectedTouchUV` / `llDetectedTouchPos` / `llDetectedTouchNormal` /
  `llDetectedTouchBinormal` read back. The two 2-component coords go on the wire
  as vectors with a zero `z`, as the reference packs them. A caller with no pick
  (the `sl-repl` commands, a scripted touch by id) passes `None` and sends no
  block, exactly as before.
- **A second pick path, through the HUD camera** (`hud_pick.rs`). The crosshair
  pick (`objects::pick_object`) casts a *fly-camera* ray through the world
  scene; a HUD lives in the HUD camera's orthographic space on
  `HUD_RENDER_LAYER`, where a world ray never goes. `pick_and_touch` casts an
  orthographic ray through the HUD camera at the cursor
  (`Camera::viewport_to_world`), filtered to the HUD render layer, and — the
  reference's HUD-first order — only falls through to a perspective fly-camera
  ray against the *non*-HUD geometry when nothing HUD-ward is hit, so a HUD
  covering half the screen never leaks a click to the ground behind it. The
  surface block is built by `surface_info_from_hit` from the ray hit: face index
  from the hit face entity, `ST` from the mesh UV un-flipped back to Second
  Life's bottom-up space, `UV` from `ST` with the face's texture placement
  (`texture_face_uv_transform`) applied, and position / normal / binormal in the
  object's own Second Life frame (a HUD has no meaningful region position, so
  object-local is the sensible finite choice — a deliberate simplification from
  the reference's region / HUD-matrix frame; the binormal is derived
  geometrically from the hit triangle rather than a texture tangent the ray does
  not carry).
- **A cursor to click with** (`HudCursorMode`, `H`). The debug fly-camera grabs
  and hides the pointer for mouse-look at all times, so there was no free
  cursor. `H` toggles a mode that releases and shows the pointer and suppresses
  the fly camera's mouse-look (so moving the mouse aims the cursor, not the
  head). This is a workaround for our inverted-from-SL debug camera (SL frees
  the cursor by default and makes *mouselook* the mode); the camera-mode-machine
  task ([[viewer-camera-third-person-orbit]]) carries a note to delete
  `HudCursorMode` once the standard SL camera model exists and clicks pick the
  HUD directly.

Unit-tested: `hud_pick.rs` — the surface-info build (face index passthrough /
`-1` default, the `ST` un-flip and identity-placement `UV == ST`, a texture
repeat tiling `UV` away from `ST`, position/normal carried into the object
frame, a unit perpendicular binormal). `sl-proto` lifecycle — a picked touch
puts the face / `UV` (zero-`z`) / `ST` / position / normal / binormal on
**both** the `ObjectGrab` and the `ObjectDeGrab`.

**Verified live on OpenSim**, the end-to-end check the task asked for: a
scripted touch responder (`touch_start` → `llSay`) worn on the HUD (an
OAR-loaded scripted 0.5 m cube attached to HUD Center). In HUD-cursor mode,
clicking the cube six times each logged `P35.3 touch (HUD) object … face=4` with
a different surface position per click, and the script's chat came back in the
viewer — pick → grab → sim → script → chat, through the HUD camera's
orthographic pick.

Kept deliberately out of scope (each its own task if wanted, as [[viewer-p35-2]]
noted): HUD **zoom** (`mHUDCurZoom` / Ctrl+0/8/9) and HUD **particles**
([[viewer-p35-4]]). The press-drag-release grab (`ObjectGrabUpdate` with a
moving surface) now *accepts* a surface but no viewer gesture drives it yet — a
touch is still a grab+degrab with no drag between.
