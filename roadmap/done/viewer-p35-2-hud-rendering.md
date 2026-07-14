---
id: viewer-p35-2
title: HUD rendering
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 35 — HUD attachments
refs: [viewer-p35-1, viewer-p27-4, viewer-p33-3]
blocked_by: [viewer-p35-1]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** The HUD layer [[viewer-p35-1]] routes an own HUD attachment onto is
now **drawn**: a worn HUD renders fixed to the screen, in screen space, over the
finished world frame — prims, sculpts and mesh alike, through the existing
geometry + texture build (nothing about the HUD path is asset-specific).

- **HUD camera** (`hud::setup_hud_screen` spawns it beside the screen):
  orthographic, `order: 1`, `ClearColorConfig::None`, `RenderLayers::layer(1)`,
  looking down the HUD's depth axis (Second Life `+x`, up `+z`), standing
  `HUD_CAMERA_DEPTH` back of the screen plane with `near = 0` so content
  *behind* the plane is still seen (the reference fits its near plane to the
  HUD's bounding box, to the same end). Drawing after the world camera — which
  is where the reference draws HUDs too, in `render_ui`, *after*
  `renderFinalize`'s tonemap — means the HUD is composited over the tonemapped
  frame and is not itself tonemapped or fogged ([[viewer-p33-3]]'s `SlTonemap`
  is on the world camera). It shares the window's view-target chain (same
  `Msaa`, same `Hdr`).
- **The projection is the reference's**, `get_hud_matrices`:
  `ortho(-0.5·aspect, +0.5·aspect, -0.5, +0.5, …)` — a fixed **1.0** vertical
  extent, so *one HUD metre is one viewport height*, at any resolution
  (`ScalingMode::FixedVertical { viewport_height: 1.0 }`). Geometry keeps its
  proportions: a HUD cube is square in pixels, never stretched by the aspect.
- **Aspect anchoring** (`fit_hud_points`): the reference scales the `mScreen`
  joint by `(1, aspect, 1)`, and a joint's scale multiplies its *children's
  position offsets* (`LLXformMatrix::update`'s `mScaleChildOffset`) but never
  their geometry (`updateMatrix` uses the child's **own** scale). A Bevy
  `Transform` scale would reach the geometry below, so the same arithmetic is
  applied to the point-node translations instead — the corner points sit in the
  viewport's corners whatever its shape, and the prims hanging off them stay
  square.
- **Fullbright** (`apply_hud_fullbright`): the reference forces
  `LLFace::FULLBRIGHT` on a HUD attachment's faces and skips atmospherics
  (`sRenderingHUDs`) — a screen overlay has no business darkening at dusk. Here
  it is doubly necessary: a Bevy light only lights the layers it is on, and the
  sun is on the world layer, so a lit HUD material would render black.
  [`face_material`](../../sl-client-bevy-viewer/src/textures.rs) builds an
  unshared material per face, so flipping `unlit` cannot leak into world
  geometry. It runs on faces whose material *or layer* changed, which catches a
  face spawned under an already-routed attachment and one whose material a later
  render-material pipeline swaps. Deviation: the reference exempts **PBR** faces
  (`isHUDAttachment() && !is_pbr`), leaving them lit; here that would render
  them black, so every HUD face goes fullbright.

Unit-tested in `hud.rs`: the corner anchoring (aspect-scaled across-screen,
never up-screen or depth; a no-op on a square viewport), and the camera framing
the screen upright and unmirrored (Second Life screen-up ⇒ view-space up,
screen-left ⇒ view-space left — get that wrong and every HUD is mirrored).

**Verified live on OpenSim**, not aditi as the task text expected — a HUD is not
SL-only, and OpenSim can wear one (`sl-repl-tokio`: `rez_object`, then
`attach_object <local_id> hudcenter`). The 0.5 m cube renders dead-centre,
fullbright, fixed to the screen while the camera flies, and measures **50.0 % of
the viewport height and 28.1 % of its width** — exactly the reference's
`ortho(-0.5·aspect, …, -0.5, 0.5)` (`0.5 / (16/9) = 0.281`), and exactly what
Second Life shows: one HUD metre is one window height.

Not carried over (worth a later task if wanted): the HUD **zoom** (`mHUDCurZoom`
/ `HUDScaleFactor`, the Ctrl+0/8/9 keys), which scales the whole HUD; HUD
particles (`RenderHUDParticles`); and HUD picking / clicking, which needs a
second pick path through this camera.
