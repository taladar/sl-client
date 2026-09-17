---
id: viewer-texture-rotation-offset-t-in-flipped-uv-space
title: A rotated face texture turns the wrong way (upside down at 90°)
topic: viewer
status: bugs
origin: live aditi P-key probe during viewer-hover-tooltip-202ms-frame-spike (2026-09-17)
---

Context: [context/viewer.md](../context/viewer.md); code
`sl-client-bevy/src/textures.rs` (`texture_uv_transform`).

## Observation

On aditi (region Ahern), a sign in the tutorial area about the Second Life blog
rendered with its texture **upside down on both broad sides** (the face probed
and the one opposite it). The P-key probe on one side reported:

- object `b20f4f2a-158f-4215-7e79-1507f30546c0`, a thin box
  (1.51 × 1.42 × 0.03 m) at region position (31.6, 103.3, 26.7), fullbright,
  also a light;
- face 0, texture `fb4c5cd7-57c4-3149-b40a-a551d382e5f1` (512²),
  `repeats=(-1.000,-1.000) offset=(0.000,0.000) rot=1.571rad` (a quarter turn
  plus the usual "mirror both axes" repeats), default texgen.

The sign's text makes "upside down" unambiguous: the error is 180°.

## Suspected cause (derived, not yet confirmed against Firestorm)

`texture_uv_transform` is a faithful port of the reference `xform`
(`llface.cpp`), and its tests check it **in Second Life's bottom-up texture
space**. But it is applied as a Bevy `uv_transform` to mesh UVs that were
already flipped to `(s, 1 − t)` (`to_bevy_prim_mesh` / `to_bevy_mesh`, the
planar-texgen path), sampling a top-down image. With `F(s, t) = (s, 1 − t)`
and `X` the reference transform, the sample coordinate that matches the
reference is `F∘X∘F`, not `X`:

- the linear part becomes `D·M·D` (`D = diag(1, −1)`): the off-diagonal
  (rotation) terms change sign, so the **rotation turns the opposite way**;
- the translation picks up the flip: **`offset_t` changes sign**.

Repeats alone (a diagonal `M`) and `offset_s` are unaffected, and so is the
identity face, which is why ordinary content looks right. A 90° rotation
turned the wrong way is 180° off, which is exactly "upside down".

## Everything that uses the same placement

- `texture_face_uv_transform` callers: `textures.rs` (prim faces),
  `rigged_attachments.rs`, `edit_selection.rs`, `media_prim.rs`, the
  surface-UV inverse in `sl-viewer-world-api` (touch UVs — the reported
  `llDetectedTouchUV` must stay in Second Life space), `texture_anim.rs`;
- `legacy_materials.rs` (normal / specular map placement);
- the GPU animation path in `sl-viewer-kit/src/face_material.wgsl`
  (`sl_animated_uv`, fed `anim_static = (rotation, offset_s, offset_t,
  scale_s)`), which evaluates the placement in the shader and needs the same
  correction.

## Verify

Confirm before changing anything: build a fixture face (fake grid catalogue
or the local OpenSim) with an asymmetric texture at rotation 90° and a
non-zero `offset_t`, and compare with Firestorm (`sl-crosscheck`). Then fix
the transform in one place, with a test that composes it with the `1 − v`
mesh flip against the reference `xform` evaluated in Second Life space, and
re-check the probe's face on aditi.
