---
id: viewer-mouselook-own-head-visible-from-inside
title: Mouselook — parts of the own avatar's head render from the inside
topic: viewer
status: bugs
origin: GPU-avatar Phase 4 increment-2 live check (2026-08-13)
refs: [viewer-perf-gpu-avatar-phase4-remove-scaffolding]
---

Context: [context/viewer.md](../context/viewer.md); the code is
`sl-client-bevy-viewer/src/camera.rs`.

In **mouselook** (first person), parts of the avatar's **own head** are
visible on screen, seen from the inside (the near-clip crops into the head
mesh so you look out through the face/skull geometry).

## Not a Phase 4 regression — pre-existing camera geometry

Surfaced during the Phase 4 increment-2 live check, but proven **not** caused
by the socket rework: `own_avatar_head` reads only the head socket's
`.translation()`, and that translation is byte-identical to the old `mHead`
joint entity global (same `deformed_world_chain[mHead]` × the same root
composition — golden-tested), so the mouselook eye
(`own_avatar_head + MOUSELOOK_EYE_OFFSET.x` forward nudge, `camera.rs` ~1168)
is exactly where it was before. The increment-2 feet-then-jump bug (a
rest-late socket) was a separate issue and is fixed; this head-clip is the
residual.

## Reference behaviour (checked in phoenix-firestorm)

The reference does **not** hide the own head *mesh* in mouselook — there is no
head-region hide feature to port:

- `LLVOAvatarSelf::updateAttachmentVisibility` (llvoavatarself.cpp:1478) hides
  only **non-first-person attachments** in `CAMERA_MODE_MOUSELOOK`.
- `updateJointLODs` (llvoavatar.cpp:8282) forces the self avatar to **full
  LOD** in mouselook (the opposite of hiding).

It keeps the own head out of view purely by **camera geometry**: the mouselook
camera sits at the **eye** position looking forward, and the near-clip plane
crops the face.

## Likely fix (camera-side, not a head-hide)

Our `own_avatar_head` anchors at **`mHead`** (head-bone centre) plus a fixed
forward nudge, not at the actual **eye** joints. Candidate fixes (pick after a
repro measurement, do not add a head-hide feature):

- Anchor the mouselook eye at the eye joints (`mEyeLeft`/`mEyeRight` midpoint)
  instead of `mHead` + a guessed forward nudge, matching the reference's eye
  placement.
- And/or widen the mouselook **near-clip margin** so the near plane clears the
  face at the true eye position.

Verify: enter mouselook on several body shapes / head sizes; no face/skull
geometry visible from the inside; the third-person → mouselook transition and
head tracking stay smooth.
