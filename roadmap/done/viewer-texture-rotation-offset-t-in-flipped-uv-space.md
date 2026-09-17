---
id: viewer-texture-rotation-offset-t-in-flipped-uv-space
title: A rotated face texture turns the wrong way (upside down at 90°)
topic: viewer
status: done
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

## Confirmed against Firestorm (2026-09-17)

The suspicion above is exactly right, and a `sl-crosscheck` run proves it rather
than deriving it. The catalogue scene gained three prims for the purpose —
`placement-identity`, `placement-rotated` (a quarter turn) and
`placement-offset-t` (`offset_t` 0.25) — each wearing `QUADRANT_TEXTURE`, a new
`sl_test_assets::RgbaImage::quadrants` painting red / green / blue / yellow
quadrants. A checker cannot show this: it looks the same turned or flipped.

One camera, one scene, both viewers (`--camera-position 188,130,26
--camera-look-at 188,136,25.5`):

| prim | sl-client (before) | Firestorm |
| --- | --- | --- |
| identity | red, green / blue, yellow | the same |
| rotated 90° | blue, red / yellow, green | green, yellow / red, blue |
| `offset_t` +0.25 | slid one way | slid the other |

The rotated pair is the whole bug: the quadrants turn **opposite** ways, 180°
apart, which on the aditi sign read as "upside down".

## Fix

`texture_uv_transform` (`sl-client-bevy/src/textures.rs`) now returns
`F ∘ xform ∘ F`, the reference transform conjugated by this viewer's `v` flip
(`F(u, v) = (u, 1 − v)`), in closed form: the rotation terms of the linear part
change sign, and the translation becomes
`(offset_s + 0.5 + 0.5·scale_s·(sin − cos),
0.5 − offset_t − 0.5·scale_t·(sin + cos))`.
`sl_texture_uv_transform` in `sl-viewer-kit/src/face_material.wgsl` gets the
same change, so the GPU texture-animation path agrees.

Every listed caller goes through one of those two, so prim faces, rigged
attachments, edit selection, media faces, legacy normal / specular maps and the
texture-animation driver are all fixed at once. Two consequences worth naming:

- **Touch UVs** (`surface_info_from_hit`) come out right *because* of the
  change: the flip applied before the transform and again after now cancels to
  the reference's `xform` of `ST`, in Second Life space, which is what
  `llDetectedTouchUV` must report.
- **Flip-books** now start at the sprite sheet's top-left cell and step down the
  image, as the reference does. They used to start bottom-left and step up.

## Tests

- `matches_the_reference_xform_through_the_v_flip`: a verbatim port of the
  reference `xform` in Second Life space, composed with the flip, over six
  placements (both rotation directions, both offsets, mirrored and uneven
  repeats, and all of them at once) × four points.
- `a_quarter_turn_turns_the_way_the_reference_does` and
  `offset_t_slides_towards_the_top_of_the_image` pin the two directions the
  capture showed.
- `a_flipbook_starts_at_the_top_left_cell_of_the_image` (texture_anim).
- `uv_is_the_reference_xform_of_st` (hud_pick) pins the touch UV.

## Verification

Re-running the same shot after the fix, the three prims are pixel-consistent
with Firestorm's: sampling the rotated box's four quadrants classifies
green / yellow / red / blue in both, where sl-client had blue / red / yellow /
green. The book's [Textures](../../book/src/content/textures.md) chapter gained
a section on the placement and the flip, since the trap is invisible in all the
content that has neither a rotation nor a vertical offset.
