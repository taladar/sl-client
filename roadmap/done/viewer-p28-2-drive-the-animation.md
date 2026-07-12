---
id: viewer-p28-2
title: Drive the animation
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 28 — Animated textures
---

Context: [context/viewer.md](../context/viewer.md).

**P28.2. Drive the animation.** Each frame advance every animated
object and update its affected faces: the `ROTATE` / `SCALE` / scroll modes
compose an extra UV transform onto the face's texture-entry placement
(`StandardMaterial::uv_transform`), while the flipbook mode selects the
current cell of the `size_x` × `size_y` sprite grid (a per-cell offset +
scale),
honouring the `LOOP` / `REVERSE` / `PING_PONG` / `SMOOTH` mode flags and the
`start` / `length` / `rate` timing. Mirrors the reference viewer's
`LLVOVolume::animateTextures` folding a per-face texture matrix each frame.
**Done:** `drive_texture_animations` (in `texture_anim.rs`) advances every
`ObjectTextureAnimation` holder each frame — an accumulated-elapsed
`TextureAnimationClock` beside the holder (restarted on a re-parameterised
`llSetTextureAnim`) feeds a faithful port of
`LLViewerTextureAnim::animateTextures` (`animate` → an `AnimatedPlacement` of
the driven offset / scale / rotation, the un-driven components falling back to
the face's static `TextureFace`), which is folded into each affected face's
`StandardMaterial::uv_transform` via the new param-based
`texture_uv_transform` (the factored-out core of `texture_face_uv_transform`,
now shared). The animation *replaces* the face's UV transform exactly as the
reference viewer uses `mTextureMatrix` instead of the static xform while
running (confirmed against `LLFace::getGeometryVolume`'s `do_tex_mat` path);
`restore_stopped_animations` resets each face back to its static placement
(and drops the clock) when the `ObjectTextureAnimation` holder is removed (the
`ON` bit cleared in-world, or the prim gone), via `RemovedComponents`. The
port's flipbook cell-selection / non-loop clamp / scroll / rotate paths are
unit-tested. **Live-confirmed on aditi:** the real scrolling /
animated-texture prims are visibly animated. On the local OpenSim the
provisioned
`slclient-texanim.oar` prim ingests and drives correctly (mode=0x03 ON|LOOP,
2×2, rate 1, length 4) but its default texture is the synthetic placeholder
`00000000-0000-1111-9999-000000000005` (no real asset), so the flipbook
cell-stepping has no image content to reveal and looks static — an
untextured-prim artifact of that fixture, not the driver.
