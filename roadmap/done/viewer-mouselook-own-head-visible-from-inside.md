---
id: viewer-mouselook-own-head-visible-from-inside
title: Mouselook — parts of the own avatar's head render from the inside
topic: viewer
status: done
origin: GPU-avatar Phase 4 increment-2 live check (2026-08-13)
refs: [viewer-perf-gpu-avatar-phase4-remove-scaffolding]
---

Context: [context/viewer.md](../context/viewer.md); the code is
`sl-viewer-world-avatar/src/first_person.rs` (was filed against
`sl-client-bevy-viewer/src/camera.rs`).

In **mouselook** (first person), parts of the avatar's **own head** are
visible on screen, seen from the inside (the near-clip crops into the head
mesh so you look out through the face/skull geometry).

## Not a Phase 4 regression — pre-existing

Surfaced during the Phase 4 increment-2 live check, but proven **not** caused
by the socket rework: `own_avatar_head` reads only the head socket's
`.translation()`, and that translation is byte-identical to the old `mHead`
joint entity global (same `deformed_world_chain[mHead]` × the same root
composition — golden-tested), so the mouselook eye is exactly where it was
before. The increment-2 feet-then-jump bug (a rest-late socket) was a separate
issue and is fixed; this head-clip is the residual.

## Reference behaviour (re-checked in phoenix-firestorm, 2026-09-15)

The first reading of the reference on this ticket was **wrong**: it said the
reference does not hide the own head and keeps it out of view by camera
geometry alone, so the planned fix was camera-side (anchor the eye at the eye
joints, widen the near clip). The reference **does** stop drawing the head:

- `LLAgent::needsRenderHead()` is false in mouselook (bar a mirror pass with
  `FirstPersonAvatarVisible` on). `LLVOAvatar::renderSkinned` and
  `renderTransparent` skip the head, eyelash and hair meshes when it is false —
  **except in the shadow pass** (`|| LLPipeline::sShadowRender`), so the
  avatar's shadow keeps its head.
- `renderRigid` skips the **eyeballs** with no shadow-pass exception.
- `LLAgent::needsRenderAvatar()` is false in mouselook when
  `FirstPersonAvatarVisible` is off (the reference default): nothing of the
  avatar is drawn.
- `LLVOAvatarSelf::updateAttachmentVisibility` removes every attachment on a
  point whose `avatar_lad.xml` `visible_in_first_person` is false — the head
  points (Skull, Mouth, Chin, ears, eyeballs, Nose, Jaw, the Alt ears/eyes,
  Tongue) — from **every** pass (`mDrawableType = 0`), shadows included.

So there is a head-hide to port, and the camera placement is not the defect.

## Fix (2026-09-15)

- `sl-avatar` parses `visible_in_first_person` (absent reads `false`, as in
  the reference); it rides `AttachmentPointInfo` onto each avatar's
  `AttachmentPointNode`. A vendored-assets test pins the 14 hidden points.
- New `sl-viewer-world-avatar::first_person` (`OwnAvatarView::{Whole,
  Headless, Hidden}` from `CameraMode` + a new world-api
  `FirstPersonAvatarVisible` resource the preferences tab refreshes):
  - base head / hair / eyelashes → a leaf `Propagate` onto a new
    `SUN_SHADOW_ONLY_LAYER` (the `SceneSun` is on it, no camera is), so they
    leave the view and keep their shadow; eyeballs → the probe layer only.
    Layers rather than `Visibility`, because `apply_avatar_part_visibility`
    owns the parts' visibility, and the caster cull drops hidden entities.
  - rigid attachments on a hidden point → their point node's `Visibility`
    (no other writer; a layer would not reach through the object's own
    `Propagate`).
  - rigged submeshes of an attachment on a hidden point (a mesh head) → the
    probe layer only, resolved through the new
    `ObjectState::attachment_point_of`.
- **A second defect on the same feature, fixed with it:** with the setting
  off, `apply_first_person_avatar_visibility` (preferences) hid the own
  anchor every frame while `derender::hide_suppressed_avatars` un-hid it every
  frame, unordered. The whole-body hide now lives in
  `hide_suppressed_avatars`, the anchor's only writer.
- The material-preview studios' first layer moved from 8 to 9 to make room.

The mouselook eye position is unchanged (head joint + 0.1 m along the look).

## Verify

Fixture-world tests (`world_test::first_person_tests`): mouselook layers
exactly the five head parts, hides a Skull prim but not a Chest prim, and
third person undoes all of it; with the body hidden the anchor stays hidden
across frames.

Live-verified 2026-09-15: nothing of the head visible from inside in mouselook
on the local OpenSim (system head) and on aditi with a worn **mesh head** (the
rigged-submesh path, which the fixture world cannot reach).
