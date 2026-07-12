---
id: viewer-p28-1
title: Ingest per-object texture animation
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 28 — Animated textures
---

Context: [context/viewer.md](../context/viewer.md).

Prims animate their textures (`llSetTextureAnim`): UV scroll / rotate / scale,
or a sprite-sheet flipbook stepping through a grid of frames. The wire block is
already decoded — `sl-proto`'s `decode_texture_anim` → `TextureAnimation` (mode
flags, `face`, the `size_x` × `size_y` frame grid, `start`, `length`, `rate`) —
but nothing in the viewer consumes it, so every animated texture currently sits
static. This phase is the viewer-side driver. Reference: `LLViewerTextureAnim` /
`LLVOVolume::animateTextures`.

**P28.1. Ingest per-object texture animation.** Carry the decoded
`TextureAnimation` from each object's `texture_anim` update onto the object
(a component beside the geometry holder, like the P27 material holders),
resolving the target-face bitmask (`face == -1` = all faces). The decode
itself already lives in `sl-proto`; net-new is holding the state on the object
and clearing it when the animation stops (`ON` bit clear). **Done:** a new
`texture_anim.rs` module holds an `ObjectTextureAnimation` component — the
decoded `TextureAnimation` — on the object's **geometry holder** entity (the
parent of its face entities), exactly mirroring the P27.1
`ObjectRenderMaterials` holder. `apply_texture_animation` (in `objects.rs`,
beside `apply_render_materials`) refreshes it on every object update, gated by
`running_texture_animation` so the holder is present only while the `ON` bit
is set and removed otherwise — a prim whose animation is stopped in-world
reverts to static. The `-1` = all-faces resolution lives in
`ObjectTextureAnimation::applies_to_face` (taking a `u16` face index so it
also covers mesh faces past the prim range), which the P28.2 driver will use
to pick affected faces; unit-tested along with the `ON`-gate. Since a terse
update clones the session's cached full `Object`, the decoded animation
survives motion-only updates (no risk of a terse update wrongly clearing it,
which would flip the animation static every frame). No visual
change yet (that is P28.2) — the ingest is surfaced by a `debug!` on each
ingest and by the `P` pick tool, which reports the picked object's animation
params and whether it targets the face under the crosshair. **Live-confirmed
on both grids:** OpenSim drove the ingest `debug!` from a provisioned
`slclient-texanim.oar` prim (`mode=0x03` `ON|LOOP`, `2x2` flipbook grid), and
aditi's pick tool read a real scrolling prim (`mode=0x13` `ON|LOOP|SMOOTH`,
`1x1` grid, `rate=0.300`, `targets_picked_face=Some(true)`).
